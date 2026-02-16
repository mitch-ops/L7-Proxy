use std::sync::Arc;

use hyper::Client;
use hyper::client::HttpConnector;

use crate::router::Router;

#[derive(Clone)]
pub struct AppState {
    pub router: Arc<Router>,
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