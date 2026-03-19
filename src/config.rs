use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub bind: String,
    pub request_timeout_secs: u64,
    pub max_retries: usize,
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,
    #[serde(default = "default_health_cooldown_secs")]
    pub health_cooldown_secs: u64,
}

fn default_failure_threshold() -> u32 {
    3
}

fn default_health_cooldown_secs() -> u64 {
    30
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
