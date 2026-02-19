use crate::state::AppState;
use hyper::header::HeaderName;
use hyper::{Body, Request, Response, Uri};
use std::{convert::Infallible, sync::Arc};
use tokio::time::{Duration, timeout};
use tracing::{error, info};
use uuid::Uuid;
use crate::errors::ProxyError;

pub async fn proxy_request(
    mut req: Request<Body>,
    state: Arc<AppState>,
) -> Result<Response<Body>, ProxyError> {
    let path = req.uri().path();
    let request_id = uuid::Uuid::new_v4().to_string();

    info!(
        request_id = %request_id,
        method = ?req.method(),
        path = ?req.uri().path(),
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

    let index = route.balancer.next_index(route.upstreams.len());
    let selected_upstream = &route.upstreams[index];

    let new_uri = format!("{}{}", selected_upstream, final_path);

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

    req.headers_mut().insert(
        HeaderName::from_static("x-request-id"),
        request_id.parse().unwrap(),
    );

    // Forward request
    let upstream_call = state.client.request(req);

    // match timeout(Duration::from_secs(5), upstream_call).await {
    //     Ok(result) => match result {
    //         Ok(response) => {
    //             info!(
    //                 request_id = %request_id,
    //                 status = %response.status(),
    //                 "Upstream responded"
    //             );
    //             Ok(response)
    //         }
    //         Err(e) => {
    //             error!(
    //                 request_id = %request_id,
    //                 error = ?e,
    //                 "Upstream connection error"
    //             );
    //             Ok(Response::builder()
    //                 .status(502)
    //                 .body(Body::from("Bad Gateway"))
    //                 .unwrap())
    //         }
    //     },
    //     Err(_) => {
    //         error!(
    //             request_id = %request_id,
    //             "Upstream request timed out"
    //         );
    //         Ok(Response::builder()
    //             .status(504)
    //             .body(Body::from("Gateway Timeout"))
    //             .unwrap())
    //     }
    // }
    let response = match timeout(Duration::from_secs(5), upstream_call).await {
        Ok(Ok(resp)) => {
            info!(
                request_id = %request_id,
                status = %resp.status(),
                "Upstream responded"
            );
            resp
        }
        Ok(Err(e)) => {
            error!(
                request_id = %request_id,
                error = ?e,
                "Upstream connection error"
            );
            return Err(ProxyError::UpstreamFailure);
        }
        Err(_) => {
            error!(
                request_id = %request_id,
                "Upstream request timed out"
            );
            return Err(ProxyError::UpstreamTimeout);
        }
    };

    Ok(response)
}
