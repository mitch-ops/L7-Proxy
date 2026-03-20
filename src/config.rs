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
    pub health_check: Option<HealthCheckConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct HealthCheckConfig {
    #[serde(default = "default_health_check_path")]
    pub path: String,
    #[serde(default = "default_health_check_interval")]
    pub interval_secs: u64,
    #[serde(default = "default_health_check_timeout")]
    pub timeout_secs: u64,
}

fn default_failure_threshold() -> u32 {
    3
}

fn default_health_cooldown_secs() -> u64 {
    30
}

fn default_health_check_path() -> String {
    "/health".to_string()
}

fn default_health_check_interval() -> u64 {
    10
}

fn default_health_check_timeout() -> u64 {
    3
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
