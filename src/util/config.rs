use once_cell::sync::Lazy;
use serde::Deserialize;
use std::fs::File;
use std::io::Read;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub app: AppConfig,
}

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub port: u16,
    pub log_dir: String,
    pub redis: String,
    pub nats: String,
    pub stun: String,
    pub turn: Option<String>,
    pub turn_user: Option<String>,
    pub turn_password: Option<String>,
    pub public_ip: Option<String>,
    pub auth: bool,
    pub debug: bool,
}

pub fn read_config() -> Config {
    let mut file = File::open("config.toml").expect("Failed to open config file");
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .expect("Failed to read config file");

    let config: Config = toml::from_str(&contents).expect("Failed to parse TOML");
    return config;
}

pub static CONFIG: Lazy<Config> = Lazy::new(|| {
    let conf = read_config();
    conf
});
