use crate::middleware::JWT;
use axum::Router;
use std::collections::HashMap;
use tower_http::auth::RequireAuthorizationLayer;

mod room;
mod token;
mod transceive;
mod user;

pub fn apply_routes() -> Router {
    let mut unless = HashMap::new();
    unless.insert(r"^/".to_string(), "get|post|put|delete".to_string());
    let restrict_layer = RequireAuthorizationLayer::custom(JWT::new(unless));
    token::apply_routes()
        .merge(transceive::apply_routes())
        .merge(user::apply_routes())
        .merge(room::apply_routes())
        .layer(restrict_layer)
}
