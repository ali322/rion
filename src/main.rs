use anyhow::Context;
use axum::{error_handling::HandleErrorLayer, http::Method, Server};
use once_cell::sync::Lazy;
use rion::{
    api::apply_routes,
    init_logger,
    middleware::handle_error,
    pkg::{SharedState, SHARED_STATE},
    util::config::CONFIG,
};
use std::{net::SocketAddr, time::Duration};
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
};
use tracing::info;

#[tokio::main]
async fn main() {
    let log_dir = CONFIG.app.log_dir.clone();
    if !std::path::Path::new(&log_dir).is_dir() {
        std::fs::create_dir(&log_dir).expect("failed to create log dir");
    }
    init_logger!(log_dir);

    let cors = CorsLayer::new()
        .allow_methods(vec![Method::POST, Method::GET, Method::PUT, Method::DELETE])
        .allow_headers(Any)
        .allow_origin(Any);
    let middlewares = ServiceBuilder::new()
        .layer(cors)
        .layer(HandleErrorLayer::new(handle_error))
        .layer(CompressionLayer::new())
        .timeout(Duration::from_secs(30));
    let redis_client = redis::Client::open(CONFIG.app.redis.clone()).expect("connect redis failed");
    let conn = redis_client
        .get_multiplexed_tokio_connection()
        .await
        .expect("get multiplexed tokio redis conn failed");
    info!("redis connected at {}", CONFIG.app.redis);
    SHARED_STATE.set_redis(conn).expect("set redis conn failed");
    let nats = nats::asynk::connect(&CONFIG.app.nats)
        .await
        .expect("connect nats failed");
    info!("nats connected at {}", CONFIG.app.nats);
    SHARED_STATE.set_nats(nats).expect("set nats conn failed");

    let routes = apply_routes().layer(middlewares);
    let addr = SocketAddr::from(([0, 0, 0, 0], CONFIG.app.port));
    tracing::info!("app started at {}", CONFIG.app.port);
    Server::bind(&addr)
        .serve(routes.into_make_service())
        .await
        .expect("app started failed")
}
