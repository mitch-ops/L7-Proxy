use hyper::client::HttpConnector;
use hyper::{Body, Client, Request, Response, Server, Uri};
use hyper::service::{make_service_fn, service_fn};
use std::convert::Infallible;
use tracing::{info, error};

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

    // Build new URI pointing to upstream
    let new_uri = format!(
        "{}{}",
        upstream,
        req.uri().path_and_query()
            .map(|x| x.as_str())
            .unwrap_or("/")
    );

    *req.uri_mut() = new_uri.parse::<Uri>().unwrap();

    match client.request(req).await {
        Ok(res) => {
            info!("Upstream responded with {}", res.status());
            Ok(res)
        }
        Err(err) => {
            error!("Upstream error: {:?}", err);

            let response = Response::builder()
                .status(502)
                .body(Body::from("Bad Gateway"))
                .unwrap();

            Ok(response)
        }
    }
}
