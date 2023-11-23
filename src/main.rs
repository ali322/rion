use axum::{error_handling::HandleErrorLayer, http::Method, Server};
use rion::{init_logger, middleware::handle_error, util::config};
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
    println!("Hello, world!");
}
