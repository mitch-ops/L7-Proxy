use hyper::Client;
use hyper::client::HttpConnector;

#[derive(Clone)]
pub struct AppState {
    pub upstream_base: String,
    pub client: Client<HttpConnector>,
    /*
    later hold:
    routes
    balancers
    rate limiters
    config
    metrics
     */
}