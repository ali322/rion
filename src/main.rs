use axum::{error_handling::HandleErrorLayer, http::Method};
use rion::{
    api::apply_routes,
    init_logger,
    middleware::handle_error,
    pkg::{SharedState, SHARED_STATE},
    util::{config::CONFIG, grace_shutdown},
};
use std::time::Duration;
use tokio::net::TcpListener;
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
        .allow_methods([Method::POST, Method::GET, Method::PUT, Method::DELETE])
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
    SHARED_STATE
        .listen_on_commands()
        .await
        .expect("listen commands failed");

    let app = apply_routes().layer(middlewares);
    let listener = TcpListener::bind(format!("0.0.0.0:{}", CONFIG.app.port))
        .await
        .expect("create tcp listener failed");
    tracing::info!("app started at {}", CONFIG.app.port);
    grace_shutdown(listener, app).await
}
