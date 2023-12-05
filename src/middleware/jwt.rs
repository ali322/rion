use std::collections::HashMap;

use crate::util::jwt::{decode_token, Auth};
use axum::{
    async_trait,
    extract::{FromRequestParts, Request},
    http::{request::Parts, StatusCode},
    middleware::Next,
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

const AUTH_HEADER: &str = "Authorization";

#[derive(Debug, Default, Clone)]
pub struct JWT {
    unless: HashMap<String, String>,
    auth: Option<Auth>,
}

impl JWT {
    pub fn new(unless: HashMap<String, String>) -> Self {
        Self { unless, auth: None }
    }
}

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
impl<S> FromRequestParts<S> for JWT
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let url = parts.uri.to_string();
        let method = parts.method.to_string();
        // if match_rules(
        //     parts.
        //     parts.method.to_string(),
        // ) {
        //     return Ok(());
        // }
        let TypedHeader(Authorization(bearer)) = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|_| AuthError::InvalidHeader)?;
        if let Ok(token_data) = decode_token(bearer.token()) {
            let auth: Auth = token_data.claims.auth;
            Ok(JWT {
                auth: Some(auth),
                ..Default::default()
            })
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

// async fn jwt(
//     bearer: Option<TypedHeader<Authorization<Bearer>>>,
//     req: &mut Request,
//     next: Next,
// ) -> Response {
//     let mut reject_reason: Option<String> = None;
//     if let Some(TypedHeader(Authorization(bearer))) = bearer {
//         let decoded = decode_token(&bearer.token());
//         match decoded {
//             Ok(token_data) => {
//                 let auth: Auth = token_data.claims.auth;
//                 req.extensions_mut().insert(auth);
//             }
//             Err(e) => reject_reason = Some(format!("请求头 Authorization 解析错误: {:?}", e)),
//         }
//     } else {
//         reject_reason = Some("请求头 Authorization 解析错误".to_string());
//     }
//     let response = next.run(req.clone()).await;
//     response
// }

// impl<B> AuthorizeRequest<B> for JWT {
//     type ResponseBody = BoxBody;
//     fn authorize(&mut self, req: &mut Request<B>) -> Result<(), Response<Self::ResponseBody>> {
//         if match_rules(
//             self.unless.to_owned(),
//             req.uri().to_string(),
//             req.method().to_string(),
//         ) {
//             return Ok(());
//         }
//         if let Some(auth_string) = req.headers().get(AUTH_HEADER) {
//             let auth_str = auth_string.to_str().unwrap();
//             if auth_str.starts_with("Bearer ") {
//                 let auth_str = auth_str.replace("Bearer ", "");
//                 let decoded = decode_token(&auth_str);
//                 match decoded {
//                     Ok(token_data) => {
//                         let auth: Auth = token_data.claims.auth;
//                         req.extensions_mut().insert(auth);
//                     }
//                     Err(e) => {
//                         self.reject_reason = Some(format!("请求头 Authorization 解析错误: {:?}", e))
//                     }
//                 }
//             } else {
//                 self.reject_reason = Some("请求头 Authorization 不合法".to_string());
//             }
//         } else {
//             self.reject_reason = Some("请求头 Authorization 不能为空".to_string());
//         }
//         if let Some(reject_reason) = self.reject_reason.clone() {
//             let json_body = json!({"code":-1, "message": reject_reason}).to_string();
//             let body = body::boxed(body::Full::from(json_body));
//             let unauthorized_response = Response::builder()
//                 .status(StatusCode::OK)
//                 .body(body)
//                 .unwrap();
//             Err(unauthorized_response)
//         } else {
//             Ok(())
//         }
//     }
// }
