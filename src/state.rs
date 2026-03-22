use std::sync::Arc;

use hyper::Client;
use hyper::client::HttpConnector;

use crate::config::Config;
use crate::health::HealthTracker;
use crate::metrics::Metrics;
use crate::rate_limiter::RateLimiter;
use crate::retry_budget::RetryBudget;
use crate::router::Router;

#[derive(Clone)]
pub struct AppState {
    pub router: Arc<Router>,
    pub client: Client<HttpConnector>,
    pub config: Arc<Config>,
    pub health: Arc<HealthTracker>,
    pub retry_budget: Arc<RetryBudget>,
    pub metrics: Arc<Metrics>,
    pub rate_limiter: Arc<RateLimiter>,
}
