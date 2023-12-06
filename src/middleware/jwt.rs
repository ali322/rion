use std::collections::HashMap;

use crate::util::jwt::{decode_token, Auth};
use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    // body::{self, BoxBody},
    Json,
    RequestPartsExt,
};
use axum_extra::{
    headers::{authorization::Bearer, Authorization},
    TypedHeader,
};
use regex::Regex;
use serde_json::json;

fn is_method_allowed(method: &str, methods: &str) -> bool {
    for m in methods.split('|') {
        if method == m.to_uppercase() {
            return true;
        }
    }
    false
}

fn match_rules(rules: HashMap<String, String>, uri: String, method: String) -> bool {
    for (rule, methods) in rules {
        // println!("{} {}", rule, &uri);
        let reg = Regex::new(&rule).unwrap();
        if reg.is_match(&uri) {
            return is_method_allowed(&method, &methods);
        }
    }
    false
}

#[async_trait]
impl<S> FromRequestParts<S> for Auth
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let url = parts.uri.to_string();
        let method = parts.method.to_string();
        let mut unless = HashMap::new();
        unless.insert(r"^/".to_string(), "get|post|put|delete".to_string());
        if match_rules(unless, url, method) {
            return Ok(Default::default());
        }
        let TypedHeader(Authorization(bearer)) = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|_| AuthError::InvalidHeader)?;
        if let Ok(token_data) = decode_token(bearer.token()) {
            let auth: Auth = token_data.claims.auth;
            Ok(auth)
        } else {
            Err(AuthError::InvalidToken)
        }
    }
}

#[derive(Debug)]
pub enum AuthError {
    InvalidHeader,
    InvalidToken,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            AuthError::InvalidHeader => (StatusCode::FORBIDDEN, "请求头 Authorization 不合法"),
            AuthError::InvalidToken => (StatusCode::FORBIDDEN, "请求头 Authorization 解析错误"),
        };
        let body = Json(json!({
            "error": error_message,
        }));
        (status, body).into_response()
    }
}
