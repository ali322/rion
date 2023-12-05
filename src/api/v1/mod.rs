use crate::middleware::JWT;
use axum::{middleware::from_extractor, Router};
use std::collections::HashMap;

mod room;
mod token;
mod transceive;
mod user;

pub fn apply_routes() -> Router {
    let mut unless = HashMap::new();
    unless.insert(r"^/".to_string(), "get|post|put|delete".to_string());
    token::apply_routes()
        .merge(transceive::apply_routes())
        .merge(user::apply_routes())
        .merge(room::apply_routes())
        .layer(from_extractor::<JWT>())
}
