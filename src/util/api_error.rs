// use crate::repository::dao::DBError;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::HashMap, io::Error as ioError, num::ParseIntError};
use thiserror::Error;
use validator::ValidationErrors;

pub type APIResult = Result<Json<Value>, APIError>;

#[derive(Debug, Error)]
#[error("{}", .0)]
pub enum APIError {
    Custom(String),
    IO(#[from] ioError),
    ParseInt(#[from] ParseIntError),
    Validator(#[from] ValidationErrors),
    // DB(#[from] DBError),
}

impl Serialize for APIError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Validator(e) => {
                let map = format_validator_errors(e);
                serializer.collect_map(map)
            }
            _ => {
                let s = format!("{}", self);
                serializer.collect_str(&s)
            }
        }
    }
}

fn format_validator_errors(e: &ValidationErrors) -> HashMap<String, String> {
    let errors = e
        .field_errors()
        .into_iter()
        .map(|(k, v)| {
            let errors = v
                .iter()
                .map(|e| match &e.message {
                    Some(msg) => msg.to_string(),
                    None => format!("{} is invalid", e.code),
                })
                .collect::<String>();
            (k.to_string(), errors)
        })
        .collect::<HashMap<_, _>>();
    errors
}

impl IntoResponse for APIError {
    fn into_response(self) -> axum::response::Response {
        let (code, message) = (-2, json!(self));
        let json_body = json!({"code": code, "msg": message}).to_string();
        (StatusCode::OK, json_body).into_response()
        // let body = body::boxed(body::Full::from(json_body));
        // Response::builder()
        //     .status(StatusCode::OK)
        //     .body(body)
        //     .unwrap()
    }
}
