use axum::{error_handling::HandleErrorLayer, extract::Request, http::Method, Router};
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use rion::{
    api::apply_routes,
    init_logger,
    middleware::handle_error,
    pkg::{SharedState, SHARED_STATE},
    util::config::CONFIG,
};
use std::{net::SocketAddr, time::Duration};
use tokio::{net::TcpListener, signal, sync::watch, time::sleep};
use tower::{Service, ServiceBuilder};
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
};
use tracing::{debug, info};

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
    let (close_tx, close_rx) = watch::channel(());
    // Continuously accept new connections.
    loop {
        let (socket, remote_addr) = tokio::select! {
            // Either accept a new connection...
            result = listener.accept() => {
                result.unwrap()
            }
            // ...or wait to receive a shutdown signal and stop the accept loop.
            _ = shutdown_signal() => {
                debug!("signal received, not accepting new connections");
                break;
            }
        };

        debug!("connection {remote_addr} accepted");

        // We don't need to call `poll_ready` because `Router` is always ready.
        let tower_service = app.clone();

        // Clone the watch receiver and move it into the task.
        let close_rx = close_rx.clone();

        // Spawn a task to handle the connection. That way we can serve multiple connections
        // concurrently.
        tokio::spawn(async move {
            // Hyper has its own `AsyncRead` and `AsyncWrite` traits and doesn't use tokio.
            // `TokioIo` converts between them.
            let socket = TokioIo::new(socket);

            // Hyper also has its own `Service` trait and doesn't use tower. We can use
            // `hyper::service::service_fn` to create a hyper `Service` that calls our app through
            // `tower::Service::call`.
            let hyper_service = hyper::service::service_fn(move |request: Request<Incoming>| {
                // We have to clone `tower_service` because hyper's `Service` uses `&self` whereas
                // tower's `Service` requires `&mut self`.
                //
                // We don't need to call `poll_ready` since `Router` is always ready.
                tower_service.clone().call(request)
            });

            // `hyper_util::server::conn::auto::Builder` supports both http1 and http2 but doesn't
            // support graceful so we have to use hyper directly and unfortunately pick between
            // http1 and http2.
            let conn = hyper::server::conn::http1::Builder::new()
                .serve_connection(socket, hyper_service)
                // `with_upgrades` is required for websockets.
                .with_upgrades();

            // `graceful_shutdown` requires a pinned connection.
            let mut conn = std::pin::pin!(conn);

            loop {
                tokio::select! {
                    // Poll the connection. This completes when the client has closed the
                    // connection, graceful shutdown has completed, or we encounter a TCP error.
                    result = conn.as_mut() => {
                        if let Err(err) = result {
                            debug!("failed to serve connection: {err:#}");
                        }
                        break;
                    }
                    // Start graceful shutdown when we receive a shutdown signal.
                    //
                    // We use a loop to continue polling the connection to allow requests to finish
                    // after starting graceful shutdown. Our `Router` has `TimeoutLayer` so
                    // requests will finish after at most 10 seconds.
                    _ = shutdown_signal() => {
                        debug!("signal received, starting graceful shutdown");
                        conn.as_mut().graceful_shutdown();
                    }
                }
            }

            debug!("connection {remote_addr} closed");

            // Drop the watch receiver to signal to `main` that this task is done.
            drop(close_rx);
        });
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
