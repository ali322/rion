use crate::{
    pkg::{SharedState, SHARED_STATE},
    util::config::CONFIG,
};
use anyhow::{anyhow, Context, Result};
use std::{
    pin::Pin,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::SystemTime,
};
use tokio::{
    sync::oneshot,
    time::{timeout, Duration},
};
use tracing::{error, info, Instrument};
use webrtc::{
    api::{
        interceptor_registry::register_default_interceptors,
        media_engine::{MediaEngine, MIME_TYPE_OPUS, MIME_TYPE_VP8},
        setting_engine::SettingEngine,
        APIBuilder,
    },
    data_channel::{data_channel_message::DataChannelMessage, OnMessageHdlrFn, RTCDataChannel},
    ice_transport::{
        ice_candidate_type::RTCIceCandidateType, ice_connection_state::RTCIceConnectionState,
        ice_server::RTCIceServer,
    },
    interceptor::registry::Registry,
    peer_connection::{
        configuration::RTCConfiguration, peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription, OnDataChannelHdlrFn,
        OnICEConnectionStateChangeHdlrFn, OnPeerConnectionStateChangeHdlrFn, OnTrackHdlrFn,
        RTCPeerConnection,
    },
    rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication,
    rtp_transceiver::{
        rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType},
        rtp_receiver::RTCRtpReceiver,
        RTCRtpTransceiver,
    },
    track::track_remote::TrackRemote,
};

#[derive(Debug, Clone)]
struct WeakPeerConnection(std::sync::Weak<RTCPeerConnection>);

impl WeakPeerConnection {
    fn upgrade(&self) -> Option<Arc<RTCPeerConnection>> {
        self.0.upgrade()
    }
}

struct PublisherDetails {
    user: String,
    room: String,
    pc: Arc<RTCPeerConnection>,
    nats: nats::asynk::Connection,
    notify_close: Arc<tokio::sync::Notify>,
    created: SystemTime,
}

impl Drop for PublisherDetails {
    fn drop(&mut self) {
        info!(
            "dropping publisherDetails for user {} of room {}",
            self.user, self.room
        );
    }
}

impl PublisherDetails {
    fn pc_downgrade(&self) -> WeakPeerConnection {
        WeakPeerConnection(Arc::downgrade(&self.pc))
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
        registry = register_default_interceptors(registry, &mut m)?;
        let mut setting = SettingEngine::default();
        setting.set_ice_timeouts(
            Some(Duration::from_secs(3)), // disconnect timeout
            Some(Duration::from_secs(6)), // failed timeout
            Some(Duration::from_secs(1)), // keepalive timeout
        );
        if let Some(ip) = public_ip {
            // setting.set_nat_1to1_ips(vec![ip], RTCIceCandidateType::Srflx);
        }
        let api = APIBuilder::new()
            .with_setting_engine(setting)
            .with_media_engine(m)
            .with_interceptor_registry(registry)
            .build();
        info!("prepare RtcConfiguration");
        let mut servers = vec![];
        // servers.push(RTCIceServer {
        //     // e.g.: stun:stun.l.google.com:19302
        //     urls: vec![_stun],
        //     ..Default::default()
        // });
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
        info!("create peerconnection");
        api.new_peer_connection(config)
            .await
            .map_err(|e| anyhow!(e))
    }

    fn on_ice_connection_state_change(&self) -> OnICEConnectionStateChangeHdlrFn {
        let span = tracing::Span::current();
        Box::new(move |connection_state: RTCIceConnectionState| {
            let _enter = span.enter();
            info!("ice connection state has changed: {}", connection_state);
            Box::pin(async {})
        })
    }

    fn on_peer_connection_state_change(&self) -> OnPeerConnectionStateChangeHdlrFn {
        let span = tracing::Span::current();
        let created = self.created;
        let room = self.room.clone();
        let user = self.user.clone();
        let notify_close = self.notify_close.clone();
        let mut is_closed = false;
        Box::new(move |state: RTCPeerConnectionState| {
            let _enter = span.enter();
            info!("pub peer connection state has changed: {}", state);
            match state {
                RTCPeerConnectionState::Connected => {
                    let now = std::time::SystemTime::now();
                    let duration = match now.duration_since(created) {
                        Ok(d) => d,
                        Err(e) => {
                            error!("system time error: {}", e);
                            Duration::from_secs(42)
                        }
                    }
                    .as_millis();
                    info!(
                        "pub peer connection connected! spent {} ms from created",
                        duration
                    );
                    let room = room.clone();
                    let user = user.clone();
                    return Box::pin(async move {}.instrument(tracing::Span::current()));
                }
                RTCPeerConnectionState::Failed
                | RTCPeerConnectionState::Disconnected
                | RTCPeerConnectionState::Closed => {
                    // a quick hack to avoid sending duplicate PUB_LEFT
                    // when state goes to disconnected, and then goes to closed in very short time
                    if is_closed {
                        return Box::pin(async {});
                    }
                    is_closed = true;
                    // NOTE:
                    // In disconnected state, PeerConnection may still come back, e.g. reconnect using an ICE Restart.
                    // But let's cleanup everything for now.
                    info!("pub send close notification");
                    notify_close.notify_waiters();
                    let room = room.clone();
                    let user = user.clone();
                    return Box::pin(async move {}.instrument(tracing::Span::current()));
                }
                _ => {}
            }
            Box::pin(async {})
        })
    }

    fn on_track(&self) -> OnTrackHdlrFn {
        let span = tracing::Span::current();
        let nc = self.nats.clone();
        let wpc = self.pc_downgrade();
        let user = self.user.clone();
        let room = self.room.clone();
        let track_count = Arc::new(AtomicU8::new(0));
        let video_count = Arc::new(AtomicU8::new(0));
        let audio_count = Arc::new(AtomicU8::new(0));
        Box::new(
            move |track: Arc<TrackRemote>,
                  _receiver: Arc<RTCRtpReceiver>,
                  _transceiver: Arc<RTCRtpTransceiver>| {
                let _enter = span.enter();
                info!("receive new track");
                let wpc = wpc.clone();
                let user = user.clone();
                let room = room.clone();
                let nc = nc.clone();
                let track_count = track_count.clone();
                let video_count = track_count.clone();
                let audio_count = track_count.clone();
                return Box::pin(async move {
                    let tid = track.tid();
                    let kind = track.kind().to_string();
                    let stream_id = track.stream_id();
                    let msid = track.msid();
                    info!(
                        "new track: tid {}, kind {}, pt {}, ssrc {}, stream_id {}, msid {}",
                        tid,
                        kind,
                        track.payload_type(),
                        track.ssrc(),
                        stream_id, // the stream_id here generated from browser might be "{xxx}"
                        msid,      // the msid here generated from browser might be "{xxx} {ooo}"
                    );
                    let count = track_count.fetch_add(1, Ordering::SeqCst) + 1;
                    // app_id will become like "video0", "audio0"
                    let app_id = match kind.as_str() {
                        "video" => {
                            let c = video_count.fetch_add(1, Ordering::SeqCst);
                            format!("video{}", c)
                        }
                        "audio" => {
                            let c = audio_count.fetch_add(1, Ordering::SeqCst);
                            format!("audio{}", c)
                        }
                        _ => unreachable!(),
                    };
                    // catch(SHARED_STATE.add_user_media_count(&room, &user, &kind)).await;
                    // if all the tranceivers have active track
                    // let's fire the publisher join notify to all subscribers
                    {
                        let pc = match wpc.upgrade() {
                            None => return,
                            Some(pc) => pc,
                        };
                        let total = pc.get_transceivers().await.len();
                        if count as usize >= total {
                            info!("we got {} active remote tracks, all ready", total);
                            // Self::notify_subs_for_join(&room, &user).await;
                        } else {
                            info!("we got {} active remote tracks, target is {}", count, total);
                        }
                    }
                    // Send a PLI on an interval so that the publisher is pushing a keyframe every rtcpPLIInterval
                    let media_ssrc = track.ssrc();
                    Self::spawn_periodic_pli(wpc.clone(), media_ssrc);
                    // push RTP to NATS
                    // Self::spawn_rtp_to_nats(room, user, app_id.to_string(), track, nc.clone());
                });
            },
        )
    }

    fn spawn_periodic_pli(wpc: WeakPeerConnection, media_ssrc: u32) {
        tokio::spawn(
            async move {
                let mut result = Ok(0);
                while result.is_ok() {
                    let timeout = tokio::time::sleep(Duration::from_secs(2));
                    tokio::pin!(timeout);
                    tokio::select! {
                      _ = timeout.as_mut() => {
                        let pc = match wpc.upgrade() {
                          None => break,
                          Some(pc) => pc
                        };
                        result = pc.write_rtcp(&[Box::new(PictureLossIndication{
                          sender_ssrc:0, media_ssrc,
                        })]).await;
                      }
                    }
                }
            }
            .instrument(tracing::Span::current()),
        );
    }
}

pub async fn webrtc_to_nats(
    room: String,
    user: String,
    offer: String,
    answer_tx: oneshot::Sender<String>,
    tid: u16,
) -> Result<()> {
    // NATS
    info!("getting NATS");
    let nc = SHARED_STATE.get_nats().context("get NATS client failed")?;
    let peer_connection = Arc::new(
        PublisherDetails::create_pc(
            CONFIG.app.stun.clone(),
            CONFIG.app.turn.clone(),
            CONFIG.app.turn_user.clone(),
            CONFIG.app.turn_password.clone(),
            CONFIG.app.public_ip.clone(),
        )
        .await
        .context("failed to create pc")?,
    );
    let publisher = PublisherDetails {
        user: user.clone(),
        room: room.clone(),
        pc: peer_connection.clone(),
        nats: nc.clone(),
        notify_close: Default::default(),
        created: std::time::SystemTime::now(),
    };

    let offer = RTCSessionDescription::offer(offer.to_string()).unwrap();
    // Set a handler for when a new remote track starts, this handler will forward data to our UDP listeners.
    // In your application this is where you would handle/process audio/video
    peer_connection.on_track(publisher.on_track());

    // Set the handler for ICE connection state
    // This will notify you when the peer has connected/disconnected
    peer_connection.on_ice_connection_state_change(publisher.on_ice_connection_state_change());

    // Set the handler for Peer connection state
    // This will notify you when the peer has connected/disconnected
    peer_connection.on_peer_connection_state_change(publisher.on_peer_connection_state_change());

    // Register data channel creation handling
    // peer_connection
    //     .on_data_channel(publisher.on_data_channel())
    //     .await;

    // Set the remote SessionDescription
    // this will trigger tranceivers creation underneath
    info!("PC set remote SDP");
    peer_connection.set_remote_description(offer).await?;

    // Create an answer
    info!("PC create local SDP");
    let answer = peer_connection.create_answer(None).await?;

    // // Create channel that is blocked until ICE Gathering is complete
    let mut gather_complete = peer_connection.gathering_complete_promise().await;

    // // Sets the LocalDescription, and starts our UDP listeners
    peer_connection
        .set_local_description(answer)
        .await
        .context("set local SDP failed")?;

    // // Block until ICE Gathering is complete, disabling trickle ICE
    // // we do this because we only can exchange one signaling message
    // // in a production application you should exchange ICE Candidates via OnICECandidate
    let _ = gather_complete.recv().await;

    // // Send out the SDP answer via Sender
    if let Some(local_desc) = peer_connection.local_description().await {
        info!("PC send local SDP");
        answer_tx
            .send(local_desc.sdp)
            .map_err(|s| anyhow!(s).context("SDP answer send error"))?;
    } else {
        // TODO: when will this happen?
        error!("generate local_description failed!");
    }

    // // limit a publisher to 24 hours for now
    // // after 24 hours, we close the connection
    let max_time = Duration::from_secs(24 * 60 * 60);
    timeout(max_time, publisher.notify_close.notified()).await?;
    // peer_connection.close().await;
    // peer_connection.close().await?;
    info!("leaving publisher main");
    Ok(())
}
