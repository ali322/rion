use serde::Deserialize;
use std::fs::File;
use std::io::Read;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub app: AppConfig,
}

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub port: u32,
    pub log_dir: String,
}

pub fn read_config() -> Config {
    let mut file = File::open("config.toml").expect("Failed to open config file");
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .expect("Failed to read config file");

    let config: Config = toml::from_str(&contents).expect("Failed to parse TOML");
    return config;
}
