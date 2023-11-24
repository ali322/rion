use crate::{middleware::JWT, pkg::State};
use axum::Router;
use std::collections::HashMap;
use tower_http::auth::RequireAuthorizationLayer;

mod token;
mod transceive;

pub fn apply_routes() -> Router {
    let mut unless = HashMap::new();
    unless.insert(r"^/public".to_string(), "get|post".to_string());
    unless.insert(r"/pub".to_string(), "post|get".to_string());
    unless.insert(r"/sub".to_string(), "post|get".to_string());
    let restrict_layer = RequireAuthorizationLayer::custom(JWT::new(unless));
    token::apply_routes()
        .merge(transceive::apply_routes())
        .layer(restrict_layer)
}
