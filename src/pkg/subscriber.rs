use super::{catch, Command};
use crate::{
    pkg::{SharedState, SHARED_STATE},
    util::config::CONFIG,
};
use anyhow::{anyhow, bail, Context, Error, Result};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, RwLock, Weak};
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};
use tracing::Instrument;
use tracing::{debug, error, info, warn};
use webrtc::{
    api::{
        interceptor_registry::register_default_interceptors,
        media_engine::{MediaEngine, MIME_TYPE_OPUS, MIME_TYPE_VP8},
        setting_engine::SettingEngine,
        APIBuilder,
    },
    data_channel::{
        data_channel_message::DataChannelMessage, OnMessageHdlrFn, OnOpenHdlrFn, RTCDataChannel,
    },
    ice_transport::{
        ice_candidate_type::RTCIceCandidateType, ice_connection_state::RTCIceConnectionState,
        ice_credential_type::RTCIceCredentialType, ice_server::RTCIceServer,
    },
    interceptor::registry::Registry,
    peer_connection::{
        configuration::RTCConfiguration, peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription, OnDataChannelHdlrFn,
        OnICEConnectionStateChangeHdlrFn, OnNegotiationNeededHdlrFn,
        OnPeerConnectionStateChangeHdlrFn, RTCPeerConnection,
    },
    rtp_transceiver::{
        rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType},
        rtp_sender::{self, RTCRtpSender},
    },
    track::{
        track_local::track_local_static_rtp::TrackLocalStaticRTP,
        track_local::{TrackLocal, TrackLocalWriter},
    },
};

struct SubscriberDetails {
    user: String,
    room: String,
    pc: Arc<RTCPeerConnection>,
    nats: nats::asynk::Connection,
    notify_close: Arc<tokio::sync::Notify>,
    rtp_tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
    rtp_senders: Arc<RwLock<HashMap<(String, String), Arc<RTCRtpSender>>>>,
    rtp_forward_tasks: Arc<RwLock<HashMap<(String, String), JoinHandle<()>>>>,
    notify_sender: RwLock<Option<mpsc::Sender<Command>>>,
    notify_receiver: RwLock<Option<mpsc::Receiver<Command>>>,
    created: std::time::SystemTime,
    tokio_tasks: Arc<RwLock<Vec<JoinHandle<()>>>>,
    // guard for WebRTC flow
    // so we will handle only one renegotiation at once
    // in case there is many publisher come and go in very short interval
    is_doing_renegotiation: Arc<tokio::sync::Mutex<bool>>,
    need_another_renegotiation: Arc<tokio::sync::Mutex<bool>>,
}

struct Subscriber(Arc<SubscriberDetails>);
struct SubscriberWeek(Weak<SubscriberDetails>);

impl SubscriberWeek {
    fn upgrade(&self) -> Option<Subscriber> {
        self.0.upgrade().map(Subscriber)
    }
}

impl std::ops::Deref for Subscriber {
    type Target = SubscriberDetails;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for SubscriberDetails {
    fn drop(&mut self) {
        info!(
            "dropping subscribeDetails for user {} of room {}",
            self.user, self.room
        );
    }
}

impl Subscriber {
    fn downgrade(&self) -> SubscriberWeek {
        SubscriberWeek(Arc::downgrade(&self.0))
    }

    fn get_nats_subject(&self, user: &str, mime: &str, app_id: &str) -> String {
        format!("rtc.{}.{}.{}.{}", self.room, user, mime, app_id)
    }

    async fn create_pc(
        _stun: String,
        turn: Option<String>,
        turn_username: Option<String>,
        turn_password: Option<String>,
        public_ip: Option<String>,
    ) -> Result<RTCPeerConnection> {
        info!("create mediaengine");
        let mut m = MediaEngine::default();
        m.register_codec(
            RTCRtpCodecParameters {
                capability: RTCRtpCodecCapability {
                    mime_type: MIME_TYPE_VP8.to_owned(),
                    clock_rate: 90000,
                    channels: 0,
                    sdp_fmtp_line: "".to_owned(),
                    rtcp_feedback: vec![],
                },
                payload_type: 96,
                ..Default::default()
            },
            RTPCodecType::Video,
        )?;

        m.register_codec(
            RTCRtpCodecParameters {
                capability: RTCRtpCodecCapability {
                    mime_type: MIME_TYPE_OPUS.to_owned(),
                    clock_rate: 48000,
                    channels: 2,
                    sdp_fmtp_line: "".to_owned(),
                    rtcp_feedback: vec![],
                },
                payload_type: 111,
                ..Default::default()
            },
            RTPCodecType::Audio,
        )?;

        // Create a InterceptorRegistry. This is the user configurable RTP/RTCP Pipeline.
        // This provides NACKs, RTCP Reports and other features. If you use `webrtc.NewPeerConnection`
        // this is enabled by default. If you are manually managing You MUST create a InterceptorRegistry
        // for each PeerConnection.
        let mut registry = Registry::new();

        // Use the default set of Interceptors
        registry = register_default_interceptors(registry, &mut m)?;

        let mut setting = SettingEngine::default();
        setting.set_ice_timeouts(
            Some(Duration::from_secs(3)), // disconnected timeout
            Some(Duration::from_secs(6)), // failed timeout
            Some(Duration::from_secs(1)), // keep alive interval
        );

        if let Some(ip) = public_ip {
            info!("set public_ip {}", ip);
            // setting.set_nat_1to1_ips(vec![ip], RTCIceCandidateType::Host);
            setting.set_nat_1to1_ips(vec![ip], RTCIceCandidateType::Srflx);
        }

        // Create the API object with the MediaEngine
        let api = APIBuilder::new()
            .with_setting_engine(setting)
            .with_media_engine(m)
            .with_interceptor_registry(registry)
            .build();

        info!("preparing RTCConfiguration");

        let mut servers = vec![];
        if let Some(turn) = turn {
            let username = turn_username.context("TURN username not preset")?;
            let password = turn_password.context("TURN password not preset")?;
            info!("add turn {} {}:{}", turn, username, password);
            servers.push(RTCIceServer {
                urls: vec![turn],
                username,
                credential: password,
                credential_type: RTCIceCredentialType::Password,
                ..Default::default()
            });
        }
        let config = RTCConfiguration {
            ice_servers: servers,
            ..Default::default()
        };

        info!("creating PeerConnection");
        // Create a new RTCPeerConnection
        api.new_peer_connection(config)
            .await
            .map_err(|e| anyhow!(e))
    }

    fn register_notify_message(&mut self) {
        let (sender, receiver) = mpsc::channel(10);
        let result = SHARED_STATE.add_sub_notify(&self.room, &self.user, sender.clone());
        if let Err(e) = result {
            error!("add_sub_notify failed: {:?}", e);
        }
        *self.notify_sender.write().unwrap() = Some(sender);
        *self.notify_receiver.write().unwrap() = Some(receiver);
    }

    fn deregister_notify_message(&mut self) {
        let result = SHARED_STATE.remove_sub_notify(&self.room, &self.user);
        if let Err(err) = result {
            error!("remove_sub_notify failed: {:?}", err);
        }
    }

    fn on_ice_connection_state_change(&self) -> OnICEConnectionStateChangeHdlrFn {
        let span = tracing::Span::current();
        Box::new(move |connection_state: RTCIceConnectionState| {
            let _enter = span.enter(); // populate user & room info in following logs
            info!("ICE Connection State has changed: {}", connection_state);
            Box::pin(async {})
        })
    }

    fn on_peer_connection_state_change(&self) -> OnPeerConnectionStateChangeHdlrFn {
        let span = tracing::Span::current();
        let weak = self.downgrade();
        Box::new(move |s: RTCPeerConnectionState| {
            let _enter = span.enter();
            info!("peer connection state has changed: {}", s);
            let sub = match weak.upgrade() {
                Some(sub) => sub,
                _ => return Box::pin(async {}),
            };

            match s {
                RTCPeerConnectionState::Connected => {
                    let now = std::time::SystemTime::now();
                    let duration = match now.duration_since(sub.created) {
                        Ok(d) => d,
                        Err(e) => {
                            error!("systemtime error: {}", e);
                            Duration::from_secs(42)
                        }
                    }
                    .as_millis();
                    info!(
                        "peer connection connected! spent {} ms from created",
                        duration
                    );
                    return Box::pin(
                        async move {
                            catch(SHARED_STATE.add_subscriber(
                                &sub.room,
                                &sub.user,
                                Arc::clone(&sub.pc),
                            ))
                            .await;
                        }
                        .instrument(tracing::Span::current()),
                    );
                }
                RTCPeerConnectionState::Failed
                | RTCPeerConnectionState::Disconnected
                | RTCPeerConnectionState::Closed => {
                    // NOTE:
                    // In disconnected state, PeerConnection may still come back, e.g. reconnect using an ICE Restart.
                    // But let's cleanup everything for now.
                    info!("send close notification");
                    sub.notify_close.notify_waiters();

                    return Box::pin(
                        async move {
                            catch(SHARED_STATE.remove_subscriber(&sub.room, &sub.user)).await;
                        }
                        .instrument(tracing::Span::current()),
                    );
                }
                _ => {}
            }
            Box::pin(async {})
        })
    }
    fn on_negotiation_needed(&self) -> OnNegotiationNeededHdlrFn {
        Box::new(move || {
            info!("on_negotiation_needed");
            Box::pin(async {})
        })
    }
    // this function should handle follow cases:
    // case1: add user
    // case2: remove user
    // case3: (not cover yet) existing user add media
    // case4: (not cover yet) existing user remove media
    async fn update_transceivers_of_room(&self) -> Result<()> {
        let room = &self.room;
        let sub_user = &self.user;
        let pc = &self.pc;
        let rtp_tracks = &self.rtp_tracks;
        let rtp_senders = &self.rtp_senders;
        let rtp_forward_tasks = &self.rtp_forward_tasks;
        let sub = self;
        let media = SHARED_STATE.get_users_media_count(room).await?;
        let sub_id = sub_user.splitn(2, "+").take(1).next().unwrap_or(""); // "ID+RANDOM" -> "ID"

        // 1. add user
        for ((pub_user, raw_mime), count) in media.clone() {
            // skip it if publisher is the same as subscriber
            // so we won't pull back our own media streams and cause echo effect
            // TODO: remove the hardcode "-screen" case
            let pub_id = pub_user.splitn(2, '+').take(1).next().unwrap_or(""); // "ID+RANDOM" -> "ID"
            if pub_id == sub_id || pub_id == format!("{}-screen", sub_id) {
                continue;
            }

            for index in 0..count {
                let track_id = format!("{}{}", raw_mime, index);
                if rtp_tracks
                    .read()
                    .map_err(|e| anyhow!("get rtp_tracks as read failed: {}", e))?
                    .contains_key(&(pub_user.clone(), track_id.clone()))
                {
                    continue;
                }
                let mime = match raw_mime.as_ref() {
                    "video" => MIME_TYPE_VP8,
                    "audio" => MIME_TYPE_OPUS,
                    _ => unreachable!(),
                };
                info!("add new track from {} {}", pub_user, track_id);
                let track = Arc::new(TrackLocalStaticRTP::new(
                    RTCRtpCodecCapability {
                        mime_type: mime.to_owned(),
                        ..Default::default()
                    },
                    // id is the unique identifier for this Track.
                    // This should be unique for the stream, but doesn’t have to globally unique.
                    // A common example would be 'audio' or 'video' or 'desktop' or 'webcam'
                    track_id.clone(),
                    // msid, group id part
                    pub_user.clone(),
                ));
                // for later dyanmic RTP dispatch from NATS
                rtp_tracks
                    .write()
                    .map_err(|e| anyhow!("get rtp_tracks as writer failed: {}", e))?
                    .entry((pub_user.to_string(), track_id.to_string()))
                    .and_modify(|e| *e = track.clone())
                    .or_insert_with(|| track.clone());
                let rtp_sender = pc
                    .add_track(Arc::clone(&track) as Arc<dyn TrackLocal + Send + Sync>)
                    .await
                    .context("add track to peer connection failed")?;
                sub.spawn_rtp_forward_task(track, &pub_user, &raw_mime, &track_id)
                    .await?;
                rtp_senders
                    .write()
                    .map_err(|e| anyhow!("get rtp_senders as writer failed: {}", e))?
                    .entry((pub_user.clone(), track_id.clone()))
                    .and_modify(|e| *e = rtp_sender.clone())
                    .or_insert_with(|| rtp_sender.clone());
            }
        }

        let mut remove_targets = vec![];
        // 2. remove user
        for ((pub_user, track_id), rtp_sender) in rtp_senders
            .read()
            .map_err(|e| anyhow!("get rtp_tracks as reader failed: {}", e))?
            .iter()
        {
            let (mime, _count) = track_id.split_at(5);
            if media
                .get(&(pub_user.to_string(), mime.to_string()))
                .is_some()
            {
                continue;
            }
            // current (pub_user, mime) pair does not show up in room shared media metadata
            // pub_user might be left or the media is removed
            remove_targets.push((pub_user.clone(), track_id.clone(), rtp_sender.clone()));
        }
        for (pub_user, track_id, rtp_sender) in remove_targets {
            info!("removing user {} track {}", pub_user, track_id);
            if let Err(e) = pc.remove_track(&rtp_sender).await {
                error!("peer connection remove track failed: {}", e);
            }
            let mut rtp_tracks = rtp_tracks
                .write()
                .map_err(|e| anyhow!("get rtp_tracks as writer failed: {}", e))?;
            let mut rtp_senders = rtp_senders
                .write()
                .map_err(|e| anyhow!("get rtp_senders as writer failed: {}", e))?;
            let mut rtp_forward_tasks = rtp_forward_tasks
                .write()
                .map_err(|e| anyhow!("get rtp_forward_tasks as writer failed: {}", e))?;
            rtp_tracks.remove(&(pub_user.clone(), track_id.clone()));
            rtp_senders.remove(&(pub_user.clone(), track_id.clone()));
            if let Some(task) = rtp_forward_tasks.remove(&(pub_user, track_id)) {
                task.abort();
            }
        }
        Ok(())
    }

    async fn spawn_rtp_forward_task(
        &self,
        track: Arc<TrackLocalStaticRTP>,
        user: &str,
        mime: &str,
        app_id: &str,
    ) -> Result<()> {
        // get rtp from nats
        let subject = self.get_nats_subject(user, mime, app_id);
        info!("subscribe nats: {}", subject);
        let sub = self.nats.subscribe(&subject).await?;
        let task = tokio::spawn(
            async move {
                use webrtc::Error;
                while let Some(msg) = sub.next().await {
                    // TODO: make sure we leave the loop when subscriber/publisher leave
                    let raw_rtp = msg.data;
                    if let Err(e) = track.write(&raw_rtp).await {
                        error!("nats forward failed: {:?}", e);
                        if Error::ErrClosedPipe == e {
                            // peer connection has been closed
                            return;
                        } else {
                            error!("track write err: {}", e);
                            // TODO: cleanup
                        }
                    }
                }
                info!("leaving NATS to RTP pull: {}", subject);
            }
            .instrument(tracing::Span::current()),
        );
        let _ = self
            .rtp_forward_tasks
            .write()
            .map_err(|e| anyhow!("get rtp_forward_tasks as writer failed: {}", e))?
            .insert((user.to_string(), app_id.to_string()), task);
        Ok(())
    }
    /// Read incoming RTCP packets
    /// Before these packets are returned they are processed by interceptors.
    /// For things like NACK this needs to be called.
    fn spawn_rtcp_reader(&self, rtp_sender: Arc<RTCRtpSender>) -> Result<()> {
        let task = tokio::spawn(async move {
            let mut rtcp_buf = vec![0u8; 1500];
            info!("running rtp sender read");
            while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {}
            info!("leaving rtp sender read");
        });
        self.tokio_tasks
            .write()
            .map_err(|e| anyhow!("get tokio_tasks as writer failed: {}", e))?
            .push(task);
        Ok(())
    }

    fn on_data_channel(&mut self) -> OnDataChannelHdlrFn {
        let span = tracing::Span::current();
        let weak = self.downgrade();
        Box::new(move |dc: Arc<RTCDataChannel>| {
            let _enter = span.enter();
            let dc_label = dc.label().to_owned();
            if dc_label != "control" {
                return Box::pin(async {});
            }
            let dc_id = dc.id();
            info!("new datachannel {} {}", dc_label, dc_id);
            let sub = match weak.upgrade() {
                Some(v) => v,
                None => return Box::pin(async {}),
            };
            sub.on_data_channel_open(dc, dc_label, dc_id.to_string())
        })
    }

    fn on_data_channel_open(
        &self,
        dc: Arc<RTCDataChannel>,
        dc_label: String,
        dc_id: String,
    ) -> Pin<Box<tracing::instrument::Instrumented<impl std::future::Future<Output = ()>>>> {
        let weak = self.downgrade();
        Box::pin(
            async move {
                let sub = match weak.upgrade() {
                    Some(v) => v,
                    _ => return,
                };
                dc.on_open(sub.on_data_channel_sender(dc.clone(), dc_label.clone(), dc_id))
                    .instrument(tracing::Span::current());
                dc.on_message(sub.on_data_channel_msg(dc.clone(), dc_label.clone()))
                    .instrument(tracing::Span::current());
            }
            .instrument(tracing::Span::current()),
        )
    }

    fn on_data_channel_sender(
        &self,
        dc: Arc<RTCDataChannel>,
        dc_label: String,
        dc_id: String,
    ) -> OnOpenHdlrFn {
        let span = tracing::Span::current();
        let weak = self.downgrade();
        Box::new(move || {
            let _enter = span.enter();
            info!("Data channel '{}'-'{}' open", dc_label, dc_id);
            Box::pin(
                async move {
                    let sub = match weak.upgrade() {
                        Some(v) => v,
                        _ => return,
                    };
                    let pc = &sub.pc;
                    let sub_id = sub.user.splitn(2, '+').take(1).next().unwrap_or("");
                    // "ID+RANDOM" -> "ID"
                    let is_doing_renegotiation = &sub.is_doing_renegotiation;
                    let need_another_renegotiation = &sub.need_another_renegotiation;

                    // wrapping for increase lifetime, cause we will have await later
                    let notify_message = sub.notify_receiver.write().unwrap().take().unwrap();
                    let notify_message = Arc::new(tokio::sync::Mutex::new(notify_message));
                    let mut notify_message = notify_message.lock().await; // TODO: avoid this?
                    let notify_close = sub.notify_close.clone();

                    // ask for renegotiation immediately after datachannel is connected
                    // TODO: detect if there is media?
                    {
                        let mut is_doing_renegotiation = is_doing_renegotiation.lock().await;
                        *is_doing_renegotiation = true;
                        catch(sub.update_transceivers_of_room()).await;
                        catch(Self::send_data_sdp_offer(dc.clone(), pc.clone())).await;
                    }
                    let mut result = Ok(0);
                    while result.is_ok() {
                        // use a timeout to make sure we have chance to leave the waiting task even it's closed
                        tokio::select! {
                          _ = notify_close.notified() => {
                            info!("notified closed, leaving data channel");
                          },
                          msg = notify_message.recv() => {
                            let cmd = match msg {
                              Some(cmd) => cmd,
                              _ => break, // already close
                            };
                            info!("cmd from internal sender: {:?}", cmd);
                            let msg = cmd.to_user_msg();
                            match cmd {
                              Command::PubJoin(pub_user) | Command::PubLeft(pub_user) => {
                                let pub_id = pub_user.splitn(2, '+').take(1).next().unwrap();  // "ID+RANDOM" -> "ID"
                                // don't send PUB_JOIN/PUB_LEFT if current subscriber is the publisher
                                if pub_id == sub_id {
                                  continue;
                                }
                                result = Self::send_data(dc.clone(), msg).await;
                                // don't send SDP_OFFER if we are not done with previous round
                                // it means frotend is still on going another renegotiation
                                let mut is_doing_renegotiation = is_doing_renegotiation.lock().await;
                                if !*is_doing_renegotiation {
                                  info!("trigger renegotiation");
                                  *is_doing_renegotiation = true;
                                  catch(sub.update_transceivers_of_room()).await;
                                  catch(Self::send_data_sdp_offer(dc.clone(), pc.clone())).await;
                                } else {
                                  info!("mark as need renegotiation");
                                  let mut need_another_renegotiation = need_another_renegotiation.lock().await;
                                  *need_another_renegotiation = true;
                                }
                              }
                            }
                            info!("cmd from internal sender handle done");
                          }
                        }
                    }
                    if let Err(err) = result {
                        error!("data channel error: {}", err);
                    }
                    info!("leaving data channel loop for '{}'-'{}'", dc_label, dc_id);
                }
                .instrument(span.clone()),
            )
        })
    }

    fn on_data_channel_msg(&self, dc: Arc<RTCDataChannel>, dc_label: String) -> OnMessageHdlrFn {
        let span = tracing::Span::current();
        let weak = self.downgrade();
        Box::new(move |msg: DataChannelMessage| {
            let _enter = span.enter();
            let sub = match weak.upgrade() {
                Some(sub) => sub,
                _ => return Box::pin(async {}),
            };

            let pc = sub.pc.clone();

            let dc = dc.clone();
            let msg_str = String::from_utf8(msg.data.to_vec()).unwrap_or_default();
            info!("Message from DataChannel '{}': '{:.20}'", dc_label, msg_str);
            if msg_str.starts_with("SDP_ANSWER") {
                let answer = match msg_str.split_once(' ').map(|x| x.1) {
                    Some(o) => o,
                    _ => return Box::pin(async {}),
                };
                debug!("got new SDP answer: {}", answer);
                // build SDP Offer type
                let answer = RTCSessionDescription::answer(answer.to_string()).unwrap();
                let need_another_renegotiation = sub.need_another_renegotiation.clone();
                let is_doing_renegotiation = sub.is_doing_renegotiation.clone();
                return Box::pin(
                    async move {
                        let dc = dc.clone();
                        if let Err(e) = pc.set_remote_description(answer).await {
                            error!("SDP_ANSWER set error: {}", e);
                            return;
                        }
                        let mut need_another_renegotiation =
                            need_another_renegotiation.lock().await;
                        if *need_another_renegotiation {
                            info!("trigger another round of renegotiation");
                            catch(sub.update_transceivers_of_room()).await; // update the transceivers now
                            *need_another_renegotiation = false;
                            catch(Self::send_data_sdp_offer(dc.clone(), pc.clone())).await;
                        } else {
                            info!("mark renegotiation as done");
                            let mut is_doing_renegotiation = is_doing_renegotiation.lock().await;
                            *is_doing_renegotiation = false;
                        }
                    }
                    .instrument(span.clone()),
                );
            } else if msg_str == "STOP" {
                info!("actively close peer connection");
                return Box::pin(
                    async move {
                        // let _ = pc.close().await;
                    }
                    .instrument(span.clone()),
                );
            }
            info!(
                "DataChannel {} message handle done: '{:.20}'",
                dc_label, msg_str
            );

            // still send something back even if we don't do special things
            // so browser knows server received the messages
            Box::pin(
                async move {
                    if let Err(err) = dc.send_text("OK".to_string()).await {
                        error!("send OK to data channel error: {}", err);
                    };
                }
                .instrument(span.clone()),
            )
        })
    }

    async fn send_data(dc: Arc<RTCDataChannel>, msg: String) -> Result<usize, webrtc::Error> {
        info!("data channel sending: {}", &msg);
        let result = dc.send_text(msg).await;
        info!("data channel sent done");
        result
    }

    async fn send_data_sdp_offer(
        dc: Arc<RTCDataChannel>,
        pc: Arc<RTCPeerConnection>,
    ) -> Result<()> {
        info!("making SDP offer");
        let offer = match pc.create_offer(None).await {
            Ok(v) => v,
            Err(e) => {
                bail!("recreate offer failed: {}", e);
            }
        };
        if let Err(e) = pc.set_local_description(offer.clone()).await {
            bail!("set local sdp failed: {}", e);
        }
        if let Some(offer) = pc.local_description().await {
            info!("sent new sdp offer");
            if let Err(e) = dc.send_text(format!("SDP_OFFER {}", offer.sdp)).await {
                bail!("send SDP_OFFER to data channel error: {}", e);
            }
        } else {
            bail!("somehow didn't get local SDP?!");
        }
        Ok(())
    }
}

pub async fn nats_to_webrtc(
    room: String,
    user: String,
    offer: String,
    answer_tx: oneshot::Sender<String>,
    tid: u16,
) -> Result<()> {
    let offer = RTCSessionDescription::offer(offer.to_string()).unwrap();
    info!("get nats");
    let nc = SHARED_STATE.get_nats().context("get nats client failed")?;
    let peer_connection = Arc::new(
        Subscriber::create_pc(
            CONFIG.app.stun.clone(),
            CONFIG.app.turn.clone(),
            CONFIG.app.turn_user.clone(),
            CONFIG.app.turn_password.clone(),
            CONFIG.app.public_ip.clone(),
        )
        .await?,
    );
    let mut subscriber = Subscriber(Arc::new(SubscriberDetails {
        user: user.clone(),
        room: room.clone(),
        pc: peer_connection.clone(),
        nats: nc.clone(),
        notify_close: Default::default(),
        rtp_tracks: Default::default(),
        rtp_senders: Default::default(),
        rtp_forward_tasks: Default::default(),
        notify_sender: Default::default(),
        notify_receiver: Default::default(),
        created: std::time::SystemTime::now(),
        tokio_tasks: Default::default(),
        is_doing_renegotiation: Default::default(),
        need_another_renegotiation: Default::default(),
    }));

    subscriber.register_notify_message();

    // Set the handler for ICE connection state
    // This will notify you when the peer has connected/disconnected
    peer_connection.on_ice_connection_state_change(subscriber.on_ice_connection_state_change());

    // Register data channel creation handling
    peer_connection.on_data_channel(subscriber.on_data_channel());

    // Set the handler for Peer connection state
    // This will notify you when the peer has connected/disconnected
    peer_connection.on_peer_connection_state_change(subscriber.on_peer_connection_state_change());

    peer_connection.on_negotiation_needed(subscriber.on_negotiation_needed());

    // Set the remote SessionDescription
    peer_connection.set_remote_description(offer).await?;

    // Create an answer
    let answer = peer_connection.create_answer(None).await?;

    // Create channel that is blocked until ICE Gathering is complete
    let mut gather_complete = peer_connection.gathering_complete_promise().await;

    // Sets the LocalDescription, and starts our UDP listeners
    peer_connection.set_local_description(answer).await?;

    // Block until ICE Gathering is complete, disabling trickle ICE
    // we do this because we only can exchange one signaling message
    // in a production application you should exchange ICE Candidates via OnICECandidate
    let _ = gather_complete.recv().await;

    // Output the answer in base64 so we can paste it in browser
    if let Some(local_desc) = peer_connection.local_description().await {
        info!("PC send local SDP");
        answer_tx
            .send(local_desc.sdp)
            .map_err(|s| anyhow!(s).context("SDP answer send error"))?;
    } else {
        // TODO: when will this happen?
        warn!("generate local_description failed!");
    }

    // limit a subscriber to 24 hours for now
    // after 24 hours, we close the connection
    let max_time = Duration::from_secs(24 * 60 * 60);
    timeout(max_time, subscriber.notify_close.notified()).await?;
    peer_connection.close().await?;
    subscriber.deregister_notify_message();
    // remove all spawned tasks
    for task in subscriber
        .rtp_forward_tasks
        .read()
        .map_err(|e| anyhow!("get rtp_forward_tasks as reader failed: {}", e))?
        .values()
    {
        task.abort();
    }
    for task in subscriber
        .tokio_tasks
        .read()
        .map_err(|e| anyhow!("get tokio_tasks as reader failed: {}", e))?
        .iter()
    {
        task.abort();
    }
    info!("leaving subscriber main");
    Ok(())
}
