use hyper::client::HttpConnector;
use hyper::{Body, Client, Request, Response, Server, Uri};
use hyper::service::{make_service_fn, service_fn};
use std::convert::Infallible;
use tracing::{info, error};
use tokio::time::{timeout, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initilize structured logging
    tracing_subscriber::fmt::init();

    let addr = ([127, 0, 0, 1], 8080).into();

    info!("Starting HTTP server on {}", addr);

//    let listener = TcpListener::bind(addr).await?;
    
    // Hyper client with conneciton pooling built in
    let client: Client<HttpConnector, Body> = Client::new();

    let make_svc = make_service_fn(move |_conn| {
        let client = client.clone();

        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                proxy_request(req, client.clone())
            }))
        }
    });

    let server = Server::bind(&addr).serve(make_svc);

    server.await?;

    Ok(())
}

async fn proxy_request(
    mut req: Request<Body>,
    client: Client<HttpConnector, Body>,
) -> Result<Response<Body>, Infallible> {
    let upstream = "http://127.0.0.1:9001";

    info!(
        method = ?req.method(),
        path = ?req.uri().path(),
        "Proxying request"
    );

    let new_uri = format!(
        "{}{}",
        upstream,
        req.uri()
            .path_and_query()
            .map(|x| x.as_str())
            .unwrap_or("/")
    );

    match new_uri.parse::<Uri>() {
        Ok(parsed) => {
            *req.uri_mut() = parsed;
        }
        Err(_) => {
            return Ok(
                Response::builder()
                    .status(500)
                    .body(Body::from("Invalid upstream URI"))
                    .unwrap()
            );
        }
    }

    // 5 second timeout
    let upstream_call = client.request(req);

    match timeout(Duration::from_secs(5), upstream_call).await {
        Ok(result) => {
            match result {
                Ok(response) => {
                    info!("Upstream responded with {}", response.status());
                    Ok(response)
                }
                Err(e) => {
                    error!("Upstream connection error: {:?}", e);
                    Ok(
                        Response::builder()
                            .status(502)
                            .body(Body::from("Bad Gateway"))
                            .unwrap()
                    )
                }
            }
        }
        Err(_) => {
            error!("Upstream request timed out");
            Ok(
                Response::builder()
                    .status(504)
                    .body(Body::from("Gateway Timeout"))
                    .unwrap()
            )
        }
    }
}