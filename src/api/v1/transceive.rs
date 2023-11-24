use crate::{
    pkg::{catch, publisher, subscriber, SharedState, State, SHARED_STATE},
    util::APIResult,
};
use axum::{
    body::{Body, Bytes},
    extract::{Path, Query},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};
use validator::Validate;

async fn create_pub(Path((room, full_id)): Path<(String, String)>, sdp: Bytes) -> APIResult {
    // body.validate()?;
    let id = full_id.splitn(2, '+').take(1).next().unwrap_or("");
    // special screen share "-screen" suffix
    let id = id.trim_end_matches("-screen").to_string();
    // info!("room {} node {}", room, id);
    // check if there is another publisher in the room with same id
    match SHARED_STATE.exist_publisher(&room, &full_id).await {
        Ok(true) => {
            return Err(reject!("duplicate publisher"));
        }
        Err(_) => {
            return Err(reject!("publisher check error"));
        }
        _ => {}
    }
    let sdp = match String::from_utf8(sdp.to_vec()) {
        Ok(s) => s,
        Err(e) => {
            error!("SDP parsed error: {}", e);
            return Err(reject!("bad sdp"));
        }
    };
    debug!("pub: auth {} sdp {:.20?}", "user_token", sdp);
    let (tx, rx) = tokio::sync::oneshot::channel();
    // get a time based id to represent following Tokio task for this user
    // if user call it again later
    // we will be able to identify in logs
    let now = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d,
        Err(e) => {
            error!("system time error: {}", e);
            return Err(reject!("time error"));
        }
    }
    .as_micros();
    let tid = now.wrapping_div(10000) as u16;
    tokio::spawn(catch(publisher::webrtc_to_nats(
        room.clone(),
        full_id.clone(),
        sdp,
        tx,
        tid,
    )));

    let sdp_answer = match rx.await {
        Ok(s) => s,
        Err(e) => {
            error!("sdp answer failed: {}", e);
            return Err(reject!("sdp answer failed"));
        }
    };
    debug!("sdp answer: {:.20}", sdp_answer);
    Ok(reply!({"sdp": sdp_answer}))
}

async fn create_sub(Path((room, full_id)): Path<(String, String)>, sdp: Bytes) -> APIResult {
    // body.validate()?;
    // info!("room {} node {} body {:?}", room, node, body);
    // <user>+<random> => <user>
    let id = full_id.splitn(2, '+').take(1).next().unwrap_or("");
    // special screen share "-screen" suffix
    let id = id.trim_end_matches("-screen").to_string();

    // check if there is another publisher in the room with same id
    match SHARED_STATE.exist_subscriber(&room, &full_id).await {
        Ok(true) => {
            return Err(reject!("duplicate subscriber"));
        }
        Err(_) => {
            return Err(reject!("subscriber check error"));
        }
        _ => {}
    }
    let sdp = match String::from_utf8(sdp.to_vec()) {
        Ok(s) => s,
        Err(e) => {
            error!("SDP parsed error: {}", e);
            return Err(reject!("bad sdp"));
        }
    };
    debug!("sub: auth {} sdp {:.20?}", "user_token", sdp);
    let (tx, rx) = tokio::sync::oneshot::channel();
    // get a time based id to represent following Tokio task for this user
    // if user call it again later
    // we will be able to identify in logs
    let now = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d,
        Err(e) => {
            error!("system time error: {}", e);
            return Err(reject!("time error"));
        }
    }
    .as_micros();
    let tid = now.wrapping_div(10000) as u16;
    tokio::spawn(catch(subscriber::nats_to_webrtc(
        room.clone(),
        full_id.clone(),
        sdp,
        tx,
        tid,
    )));
    let sdp_answer = match rx.await {
        Ok(s) => s,
        Err(e) => {
            error!("sdp answer failed: {}", e);
            return Err(reject!("sdp answer failed"));
        }
    };
    debug!("sdp answer: {:.20}", sdp_answer);
    Ok(reply!({"sdp": "sdp_answer"}))
}

async fn pubs() -> APIResult {
    Ok(reply!("pubs"))
}

pub fn apply_routes() -> Router {
    let router = Router::new();
    router
        .route("/pub", get(pubs))
        .route("/pub/:room/:node", post(create_pub))
        .route("/sub/:room/:node", post(create_sub))
}
