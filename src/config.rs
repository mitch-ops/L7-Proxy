use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub bind: String,
    pub request_timeout_secs: u64,
    pub max_retries: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RouteConfig {
    pub prefix: String,
    pub upstream: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub routes: Vec<RouteConfig>,
}
