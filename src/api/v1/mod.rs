use crate::util::jwt::Auth;

use axum::{middleware::from_extractor, Router};

mod room;
mod token;
mod transceive;
mod user;

pub fn apply_routes() -> Router {
    token::apply_routes()
        .merge(transceive::apply_routes())
        .merge(user::apply_routes())
        .merge(room::apply_routes())
        .layer(from_extractor::<Auth>())
}
