use hyper::{Body, Response, StatusCode};

#[derive(Debug)]
pub enum ProxyError {
    UpstreamFailure,
    UpstreamTimeout,
    NoRoute,
}

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
        }
    }
}
