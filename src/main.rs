mod state;
mod proxy;
mod config;
mod router;
mod balancer;

use hyper::client::HttpConnector;
use hyper::{Body, Client, Server};
use hyper::service::{make_service_fn, service_fn};
use std::convert::Infallible;
use tracing::{info};
use std::fs;
use config::Config;
use router::{Route, Router};
use std::sync::Arc;
use balancer::RoundRobin;

use crate::state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // let addr = ([127, 0, 0, 1], 8080).into();
    let config_str = fs::read_to_string("config.yaml")?;
    let config: Config = serde_yaml::from_str(&config_str)?;

    let addr: std::net::SocketAddr = config.server.bind.parse()?;

    let routes = config.routes
        .iter()
        .map(|r| Route {
            prefix: r.prefix.clone(),
            upstreams: r.upstream.clone(),
            balancer: RoundRobin::new(),
        })
        .collect::<Vec<_>>();

    let router = Arc::new(Router::new(routes));

    info!("Starting HTTP server on {}", addr);

//    let listener = TcpListener::bind(addr).await?;
    
    // Hyper client with conneciton pooling built in
    let client: Client<HttpConnector, Body> = Client::new();

    let state = Arc::new(AppState {
        router,
        client,
    });

    let make_svc = make_service_fn(move |_conn| {
        let state = state.clone();

        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                proxy::proxy_request(req, state.clone())
            }))
        }
    });

    let server = Server::bind(&addr).serve(make_svc);

    server.await?;

    Ok(())
}