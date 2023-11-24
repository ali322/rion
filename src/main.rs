use axum::{error_handling::HandleErrorLayer, http::Method, Server};
use rion::{
    api::apply_routes, init_logger, middleware::handle_error, pkg::SharedState, util::config,
};
use std::{net::SocketAddr, time::Duration};
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
};

#[tokio::main]
async fn main() {
    let config = config::read_config();
    let log_dir = config.app.log_dir;
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
    let app_state = SharedState::default();
    let routes = apply_routes().layer(middlewares).with_state(app_state);
    let addr = SocketAddr::from(([0, 0, 0, 0], config.app.port));
    tracing::info!("app started at {}", config.app.port);
    Server::bind(&addr)
        .serve(routes.into_make_service())
        .await
        .expect("app started failed")
}
