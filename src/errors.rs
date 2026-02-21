use std::fmt;

use hyper::{Body, Response, StatusCode};

#[derive(Debug)]
pub enum ProxyError {
    UpstreamFailure,
    UpstreamTimeout,
    NoRoute,
    UpstreamFailed,
    InvalidUri,
    NoUpstream,
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A simple textual representation; adjust messages if you want more detail
        let s = match self {
            ProxyError::UpstreamFailure => "Upstream failure",
            ProxyError::UpstreamTimeout => "Upstream timeout",
            ProxyError::NoRoute => "No route found",
            ProxyError::UpstreamFailed => "Upstream request failed",
            ProxyError::InvalidUri => "Invalid URI",
            ProxyError::NoUpstream => "No upstream available",
        };
        write!(f, "{}", s)
    }
}

impl std::error::Error for ProxyError {}

impl ProxyError {
    pub fn into_response(self) -> Response<Body> {
        match self {
            ProxyError::UpstreamFailure => Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::from("Bad Gateway"))
                .unwrap(),

            ProxyError::UpstreamTimeout => Response::builder()
                .status(StatusCode::GATEWAY_TIMEOUT)
                .body(Body::from("Gateway Timeout"))
                .unwrap(),

            ProxyError::NoRoute => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("No route found"))
                .unwrap(),

            ProxyError::UpstreamFailed => Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::from("Upstream request failed"))
                .unwrap(),

            ProxyError::InvalidUri => Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("Invalid URI"))
                .unwrap(),

            ProxyError::NoUpstream => Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(Body::from("No upstream available"))
                .unwrap(),
        }
    }
}