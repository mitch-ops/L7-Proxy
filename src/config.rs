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
    #[serde(default = "default_retry_backoff_ms")]
    pub retry_backoff_ms: u64,
    #[serde(default = "default_retry_budget_percent")]
    pub retry_budget_percent: f64,
    #[serde(default = "default_retry_budget_window_secs")]
    pub retry_budget_window_secs: u64,
    pub metrics_bind: Option<String>,
    #[serde(default = "default_overall_timeout_secs")]
    pub overall_timeout_secs: u64,
    pub rate_limit: Option<RateLimitConfig>,
    #[serde(default = "default_drain_timeout_secs")]
    pub drain_timeout_secs: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RateLimitConfig {
    pub requests_per_second: f64,
    pub burst_size: u32,
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

fn default_retry_backoff_ms() -> u64 {
    100
}

fn default_retry_budget_percent() -> f64 {
    20.0
}

fn default_retry_budget_window_secs() -> u64 {
    10
}

fn default_overall_timeout_secs() -> u64 {
    30
}

fn default_drain_timeout_secs() -> u64 {
    30
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub enum BalancerStrategy {
    #[default]
    RoundRobin,
    LeastConnections,
    WeightedRoundRobin,
    ConsistentHash,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RouteConfig {
    pub prefix: String,
    pub upstream: Vec<String>,
    #[serde(default)]
    pub strategy: BalancerStrategy,
    pub weights: Option<Vec<u32>>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub routes: Vec<RouteConfig>,
}
