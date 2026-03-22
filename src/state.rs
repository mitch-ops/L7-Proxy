use std::sync::Arc;

use arc_swap::ArcSwap;
use hyper::Client;
use hyper::client::HttpConnector;

use crate::config::Config;
use crate::health::HealthTracker;
use crate::metrics::Metrics;
use crate::rate_limiter::RateLimiter;
use crate::retry_budget::RetryBudget;
use crate::router::Router;

pub struct AppState {
    pub router: ArcSwap<Router>,
    pub client: Client<HttpConnector>,
    pub config: ArcSwap<Config>,
    pub health: Arc<HealthTracker>,
    pub retry_budget: Arc<RetryBudget>,
    pub metrics: Arc<Metrics>,
    pub rate_limiter: Arc<RateLimiter>,
}
