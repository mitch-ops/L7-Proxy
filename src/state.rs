use std::sync::Arc;

use hyper::Client;
use hyper::client::HttpConnector;

use crate::config::Config;
use crate::router::Router;

#[derive(Clone)]
pub struct AppState {
    pub router: Arc<Router>,
    pub client: Client<HttpConnector>,
    pub config: Arc<Config>,
    /*
    later hold:
    routes
    balancers
    rate limiters
    config
    metrics
     */
}