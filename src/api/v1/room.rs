use crate::{
    pkg::{SharedState, SHARED_STATE},
    util::{APIError, APIResult},
};
use axum::{
    extract::{Path, Query},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Serialize, Deserialize, Validate)]
pub struct OpRoom {
    #[validate(length(min = 1, max = 50))]
    domain: String,
}

async fn create_room(Json(body): Json<OpRoom>) -> APIResult {
    body.validate()?;
    let room = SHARED_STATE
        .add_room(body.domain)
        .await
        .map_err(|e| APIError::Custom(e.to_string()))?;
    Ok(reply!(room))
}

async fn remove_room(Path(id): Path<String>, Json(body): Json<OpRoom>) -> APIResult {
    // body.validate()?;
    SHARED_STATE
        .remove_room(body.domain, id)
        .await
        .map_err(|e| APIError::Custom(e.to_string()))?;
    Ok(reply!("remove room"))
}

async fn list_room(Query(query): Query<OpRoom>) -> APIResult {
    let rooms = SHARED_STATE
        .list_room(query.domain)
        .await
        .map_err(|e| APIError::Custom(e.to_string()))?;
    Ok(reply!(rooms))
}

pub fn apply_routes() -> Router {
    let router = Router::new();
    router
        .route("/room", post(create_room))
        .route("/room/:id", delete(remove_room))
        .route("/room", get(list_room))
}
