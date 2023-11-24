use super::{catch, Command};
use crate::{
    pkg::{SharedState, SHARED_STATE},
    util::config::CONFIG,
};
use anyhow::{anyhow, bail, Context, Result};
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
        ice_server::RTCIceServer,
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
        rtp_sender::RTCRtpSender,
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
            // setting.set_nat_1to1_ips(vec![ip], RTCIceCandidateType::Host);
            // setting.set_nat_1to1_ips(vec![ip], RTCIceCandidateType::Srflx);
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
            // let username = turn_username.context("TURN username not preset")?;
            // let password = turn_password.context("TURN password not preset")?;
            // servers.push(RTCIceServer {
            //     urls: vec![turn],
            //     username,
            //     credential: password,
            //     ..Default::default()
            // });
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
                            catch(SHARED_STATE.add_subscriber(&sub.room, &sub.user)).await;
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

    fn deregister_notify_message(&mut self) {
        let result = SHARED_STATE.remove_sub_notify(&self.room, &self.user);
        if let Err(err) = result {
            error!("remove_sub_notify failed: {:?}", err);
        }
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
    // peer_connection
    //     .on_data_channel(subscriber.on_data_channel())
    //     .await;

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
    // peer_connection.close().await?;
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
