use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub bind: String,
    pub request_timeout_secs: u64,
}

#[derive(Debug, Deserialize)]
pub struct UpstreamConfig {
    pub base_url: String,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub upstream: UpstreamConfig,
}
