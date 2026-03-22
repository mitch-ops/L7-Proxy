use std::sync::Arc;

use arc_swap::ArcSwap;
use hyper::Client;
use hyper::client::HttpConnector;
use hyper_tls::HttpsConnector;

use crate::config::Config;
use crate::health::HealthTracker;
use crate::metrics::Metrics;
use crate::rate_limiter::RateLimiter;
use crate::retry_budget::RetryBudget;
use crate::router::Router;

/// HTTP/HTTPS client type used throughout the proxy.
pub type HttpClient = Client<HttpsConnector<HttpConnector>>;

pub struct AppState {
    pub router: ArcSwap<Router>,
    pub client: HttpClient,
    pub config: ArcSwap<Config>,
    pub health: Arc<HealthTracker>,
    pub retry_budget: Arc<RetryBudget>,
    pub metrics: Arc<Metrics>,
    pub rate_limiter: Arc<RateLimiter>,
}
