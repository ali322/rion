use crate::{pkg::SharedState, util::APIResult};
use axum::{
    extract::{Path, Query},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Serialize, Deserialize, Validate)]
pub struct NewPub {
    #[validate(length(min = 1, max = 100))]
    room: String,
    id: String,
    token: Option<String>,
}

async fn create_pub(Json(body): Json<NewPub>) -> APIResult {
    body.validate()?;
    Ok(reply!("created"))
}
pub fn apply_routes() -> Router<SharedState> {
    let router = Router::new();
    router.route("/public/org", post(create_pub))
}
