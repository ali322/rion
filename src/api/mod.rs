use crate::pkg::State;
use axum::{routing::get, Router};

macro_rules! reject {
    ($e: expr) => {
        crate::util::APIError::Custom($e.to_string())
    };
}

macro_rules! reply {
  ($t:tt) => {
    axum::response::Json(serde_json::json!({"code":0, "data": $t}))
  };
}

mod v1;
async fn index() -> &'static str {
    tracing::info!("hi route");
    "pong"
}

pub fn apply_routes() -> Router {
    let prefix = "/api/v1";
    let router = Router::new().route("/ping", get(index));
    router.nest(prefix, v1::apply_routes())
}
