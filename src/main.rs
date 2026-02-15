use hyper::{Body, Request, Response, Server};
use hyper::service::{make_service_fn, service_fn};
use std::convert::Infallible;
use tracing::{info};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initilize structured logging
    tracing_subscriber::fmt::init();

    let addr = ([127, 0, 0, 1], 8080).into();

    info!("Starting HTTP server on {}", addr);

//    let listener = TcpListener::bind(addr).await?;
    let make_svc = make_service_fn(|_conn| async {
        Ok::<_, Infallible>(service_fn(handle_request))
    });
    
    let server = Server::bind(&addr).serve(make_svc);

    server.await?;

    Ok(())
}

async fn handle_request(
    req: Request<Body>,
) -> Result<Response<Body>, Infallible> {
    // For now, do nothing
    info!(
        method = ?req.method(),
        path = ?req.uri().path(),
        "Received Request"
    );
    
    let response = Response::new(Body::from("Hello from Rust L6 Proxy"));

    Ok(response)
}
