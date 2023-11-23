use crate::util::serde_format::utc_datetime;
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{
    decode, encode, errors::Result, DecodingKey, EncodingKey, Header, TokenData, Validation,
};
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Auth {
    pub id: String,
    pub username: String,
    pub is_admin: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Payload {
    #[serde(with = "utc_datetime")]
    pub iat: DateTime<Utc>,
    #[serde(with = "utc_datetime")]
    pub exp: DateTime<Utc>,
    pub auth: Auth,
}

impl Payload {
    pub fn new(auth: Auth, iat: DateTime<Utc>, exp: DateTime<Utc>) -> Self {
        Self { auth, iat, exp }
    }
}

pub fn generate_token(auth: Auth) -> String {
    let iat = Utc::now();
    let exp = iat + Duration::days(30);
    let payload = Payload::new(auth, iat, exp);
    let jwt_key = env::var("JWT_KEY").expect("environment variable JWT_KEY must be set");
    encode(
        &Header::default(),
        &payload,
        &EncodingKey::from_secret(jwt_key.as_ref()),
    )
    .unwrap()
}

pub fn decode_token(token: &str) -> Result<TokenData<Payload>> {
    let jwt_key = env::var("JWT_KEY").expect("environment variable JWT_KEY must be set");
    decode::<Payload>(
        token,
        &DecodingKey::from_secret(jwt_key.as_ref()),
        &Validation::default(),
    )
}
