use axum::http::{Method, StatusCode, Uri};
use tower::{timeout, BoxError};

pub async fn handle_error(method: Method, uri: Uri, e: BoxError) -> (StatusCode, String) {
    if e.is::<timeout::error::Elapsed>() {
        (
            StatusCode::REQUEST_TIMEOUT,
            format!("{} {} timeout", method, uri),
        )
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("`{} {}` failed with {}", method, uri, e),
        )
    }
}
