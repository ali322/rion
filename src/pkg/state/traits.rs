use super::state::Command;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use redis::aio::MultiplexedConnection;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::mpsc;
use webrtc::peer_connection::RTCPeerConnection;

/// Interface for global state
///
/// NOTE:
/// Current implementation use NATS & Redis.
/// But we can have different implementation.
#[async_trait]
pub trait SharedState {
    /// Set Redis connection
    fn set_redis(&self, conn: MultiplexedConnection) -> Result<()>;
    /// Get Redis connection
    fn get_redis(&self) -> Result<MultiplexedConnection>;
    /// Set NATS connection
    fn set_nats(&self, nats: nats::asynk::Connection) -> Result<()>;
    /// Get NATS connection
    fn get_nats(&self) -> Result<nats::asynk::Connection>;

    async fn add_room(&self, domain: String) -> Result<String>;
    async fn remove_room(&self, domain: String, room: String) -> Result<()>;
    async fn list_room(&self, domain: String) -> Result<HashSet<String>>;
    fn list_own_room(&self) -> Result<Vec<String>>;
    /// Set publisher authorization token
    async fn set_pub_token(&self, room: String, user: String, token: String) -> Result<()>;
    /// Set subscriber authorization token
    async fn set_sub_token(&self, room: String, user: String, token: String) -> Result<()>;
    /// Get publisher authorization token
    async fn get_pub_token(&self, room: &str, user: &str) -> Result<String>;
    /// Get subscriber authorization token
    async fn get_sub_token(&self, room: &str, user: &str) -> Result<String>;

    async fn add_user_media_count(&self, room: &str, user: &str, mime: &str) -> Result<()>;
    async fn get_users_media_count(&self, room: &str) -> Result<HashMap<(String, String), u8>>;
    async fn remove_user_media_count(&self, room: &str, user: &str) -> Result<()>;
    async fn remove_room_media_count(&self, room: &str) -> Result<()>;

    /// Add publisher to room publishers list
    async fn add_publisher(&self, room: &str, user: &str, pc: Arc<RTCPeerConnection>)
        -> Result<()>;
    /// Remove publisher from room publishers list
    async fn remove_publisher(&self, room: &str, user: &str) -> Result<()>;
    /// Fetch room publishers list
    async fn list_publishers(&self, room: &str) -> Result<HashSet<String>>;
    /// Check if specific publisher is in room publishers list
    async fn exist_publisher(&self, room: &str, user: &str) -> Result<bool>;

    /// Add subscriber to room subscribers list
    async fn add_subscriber(
        &self,
        room: &str,
        user: &str,
        pc: Arc<RTCPeerConnection>,
    ) -> Result<()>;
    /// remove subscriber from room subscribers list
    async fn remove_subscriber(&self, room: &str, user: &str) -> Result<()>;
    /// Fetch room subscribers list
    async fn list_subscribers(&self, room: &str) -> Result<HashSet<String>>;
    /// Check if specific subscriber is in room subscribers list
    async fn exist_subscriber(&self, room: &str, user: &str) -> Result<bool>;

    /// Set Sender (for other people to talk to specific subscriber)
    fn add_sub_notify(&self, room: &str, user: &str, sender: mpsc::Sender<Command>) -> Result<()>;
    /// Remove Sender (for other people to talk to specific subscriber)
    fn remove_sub_notify(&self, room: &str, user: &str) -> Result<()>;

    /// send cross instances commands, e.g. via NATS
    async fn send_command(&self, room: &str, cmd: Command) -> Result<()>;
    /// handling cross instances commands, e.g. via NATS
    async fn on_command(&self, room: &str, cmd: &[u8]) -> Result<()>;
    /// listen on cross instances commands, e.g. via NATS
    async fn listen_on_commands(&self) -> Result<()>;
    /// forward commands to each subscriber handler
    async fn forward_to_all_subs(&self, room: &str, msg: Command) -> Result<()>;

    /// check if we still have clients
    async fn has_peers(&self) -> bool;
}
