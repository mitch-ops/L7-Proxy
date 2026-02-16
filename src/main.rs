mod state;
mod proxy;

use hyper::client::HttpConnector;
use hyper::{Body, Client, Server};
use hyper::service::{make_service_fn, service_fn};
use std::convert::Infallible;
use tracing::{info};

use crate::state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initilize structured logging
    tracing_subscriber::fmt::init();

    let addr = ([127, 0, 0, 1], 8080).into();

    info!("Starting HTTP server on {}", addr);

//    let listener = TcpListener::bind(addr).await?;
    
    // Hyper client with conneciton pooling built in
    let client: Client<HttpConnector, Body> = Client::new();

    let state = AppState {
        upstream_base: "http://localhost:3000".to_string(),
        client,
    };

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