use crate::state::AppState;
use hyper::{Body, Request, Response, Uri};
use std::{convert::Infallible, sync::Arc};
use tokio::time::{Duration, timeout};
use tracing::{error, info};

pub async fn proxy_request(
    mut req: Request<Body>,
    state: Arc<AppState>,
) -> Result<Response<Body>, Infallible> {
    let path = req.uri().path();

    info!(
        method = ?req.method(),
        path = ?path,
        "Proxying request"
    );

    // Match route
    let route = match state.router.match_route(path) {
        Some(route) => route,
        None => {
            return Ok(Response::builder()
                .status(404)
                .body(Body::from("No matching route"))
                .unwrap());
        }
    };

    // Build upstream URI
    let original_path = req
        .uri()
        .path_and_query()
        .map(|x| x.as_str())
        .unwrap_or("/");

    let stripped_path = original_path
        .strip_prefix(&route.prefix)
        .unwrap_or(original_path);

    let final_path = if stripped_path.is_empty() {
        "/"
    } else {
        stripped_path
    };

    let new_uri = format!("{}{}", route.upstream, final_path);

    match new_uri.parse::<Uri>() {
        Ok(parsed) => {
            *req.uri_mut() = parsed;
        }
        Err(_) => {
            return Ok(Response::builder()
                .status(500)
                .body(Body::from("Invalid upstream URI"))
                .unwrap());
        }
    }

    // Forward request
    let upstream_call = state.client.request(req);

    match timeout(Duration::from_secs(5), upstream_call).await {
        Ok(result) => match result {
            Ok(response) => {
                info!("Upstream responded with {}", response.status());
                Ok(response)
            }
            Err(e) => {
                error!("Upstream connection error: {:?}", e);
                Ok(Response::builder()
                    .status(502)
                    .body(Body::from("Bad Gateway"))
                    .unwrap())
            }
        },
        Err(_) => {
            error!("Upstream request timed out");
            Ok(Response::builder()
                .status(504)
                .body(Body::from("Gateway Timeout"))
                .unwrap())
        }
    }
}
