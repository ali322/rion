use bincode::{Decode, Encode};
use once_cell::sync::Lazy;
use redis::aio::MultiplexedConnection;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};
use tokio::sync::mpsc;
use webrtc::peer_connection::RTCPeerConnection;

#[derive(Debug, Default)]
pub struct Room {
    pub subs: HashMap<String, PeerConnectionInfo>,
    pub pubs: HashSet<String>,
    pub conns: Vec<Arc<RTCPeerConnection>>,
}
#[derive(Debug, Default)]
pub struct PeerConnectionInfo {
    pub notify_message: Option<Arc<mpsc::Sender<Command>>>,
}

#[derive(Decode, Encode, Debug, Clone)]
pub enum Command {
    PubJoin(String),
    PubLeft(String),
}

impl Command {
    /// convert into messages that we forward to frontend
    pub fn to_user_msg(&self) -> String {
        match &self {
            Self::PubJoin(user) => format!("PUB_JOIN {}", user),
            Self::PubLeft(user) => format!("PUB_LEFT {}", user),
        }
    }
}

#[derive(Debug, Default)]
pub struct AppState {
    pub rooms: HashMap<String, Room>,
    pub nats: Option<nats::asynk::Connection>,
    pub redis: Option<MultiplexedConnection>,
}

impl AppState {
    pub fn change(&mut self, _: &str) {
        // self.version = new.to_string();
    }
}

pub type State = Lazy<RwLock<AppState>>;
pub static SHARED_STATE: State = Lazy::new(Default::default);
