use axum::{extract::Path, routing::get, Router};

use crate::{
    pkg::{SharedState, SHARED_STATE},
    util::APIResult,
};
use tracing::info;

async fn list_pub(Path(room): Path<String>) -> APIResult {
    info!("listing publishers for room {}", room);
    let users = SHARED_STATE
        .list_publishers(&room)
        .await
        .unwrap_or_default()
        .into_iter()
        .reduce(|s, p| s + "," + &p)
        .unwrap_or_default();
    Ok(reply!(users))
}

async fn list_sub(Path(room): Path<String>) -> APIResult {
    info!("listing subscribers for room {}", room);
    let users = SHARED_STATE
        .list_subscribers(&room)
        .await
        .unwrap_or_default()
        .into_iter()
        .reduce(|s, p| s + "," + &p)
        .unwrap_or_default();
    Ok(reply!(users))
}

pub fn apply_routes() -> Router {
    let router = Router::new();
    router
        .route("/list/pub/:room", get(list_pub))
        .route("/list/sub/:room", get(list_sub))
}
