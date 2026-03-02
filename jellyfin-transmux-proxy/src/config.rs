use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Clone, Default)]
pub struct Config {
    pub server: ServerConfig,
    pub safari: SafariConfig,
    pub jellyfin: JellyfinConfig,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ServerConfig {
    pub http_listen_port: Option<u16>,
    pub https_listen_port: Option<u16>,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct SafariConfig {
    pub force_transcoding: bool,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct JellyfinConfig {
    pub jellyfin: String,
    pub mediaroot: Option<String>,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
}
