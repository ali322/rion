use crate::{
    pkg::{catch, SharedState, SHARED_STATE},
    util::APIResult,
};
use axum::{
    extract::{Path, Query},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Serialize, Deserialize, Validate)]
pub struct CreateToken {
    #[validate(length(min = 1, max = 100))]
    room: String,
    #[validate(length(min = 1, max = 50))]
    id: String,
    token: Option<String>,
}

async fn create_pub_token(Json(body): Json<CreateToken>) -> APIResult {
    body.validate()?;
    if let Some(token) = body.token.clone() {
        catch(SHARED_STATE.set_pub_token(body.room.clone(), body.id.clone(), token)).await;
    }
    Ok(reply!("pub token set"))
}

async fn create_sub_token(Json(body): Json<CreateToken>) -> APIResult {
    body.validate()?;
    if let Some(token) = body.token.clone() {
        catch(SHARED_STATE.set_sub_token(body.room.clone(), body.id.clone(), token)).await;
    }
    Ok(reply!("sub token set"))
}

pub fn apply_routes() -> Router {
    let router = Router::new();
    router
        .route("/pub/token", post(create_pub_token))
        .route("/sub/token", post(create_sub_token))
}
