use crate::{middleware::JWT, pkg::SharedState};
use axum::Router;
use std::collections::HashMap;
use tower_http::auth::RequireAuthorizationLayer;

mod token;

pub fn apply_routes() -> Router<SharedState> {
    let mut unless = HashMap::new();
    unless.insert(r"^/public".to_string(), "get|post".to_string());
    unless.insert(r"/register".to_string(), "post".to_string());
    unless.insert(r"/login".to_string(), "post".to_string());
    let restrict_layer = RequireAuthorizationLayer::custom(JWT::new(unless));
    token::apply_routes()
        // .merge(user::apply_routes())
        .layer(restrict_layer)
}
