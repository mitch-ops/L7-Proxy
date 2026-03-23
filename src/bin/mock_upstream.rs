use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::any;
use axum::Router;
use std::sync::Arc;
use std::time::Duration;

struct AppConfig {
    delay_ms: u64,
    body: Vec<u8>,
    fail_rate: f64,
}

async fn handler(State(config): State<Arc<AppConfig>>) -> impl IntoResponse {
    // Probabilistic failure
    if config.fail_rate > 0.0 {
        let roll: f64 = rand::random();
        if roll < config.fail_rate {
            return (StatusCode::INTERNAL_SERVER_ERROR, "error".to_string());
        }
    }

    // Artificial delay
    if config.delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(config.delay_ms)).await;
    }

    (StatusCode::OK, String::from_utf8_lossy(&config.body).to_string())
}

fn parse_arg(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    let port: u16 = parse_arg(&args, "--port")
        .expect("--port is required")
        .parse()
        .expect("--port must be a number");

    let delay_ms: u64 = parse_arg(&args, "--delay-ms")
        .unwrap_or_else(|| "0".to_string())
        .parse()
        .expect("--delay-ms must be a number");

    let body_size: usize = parse_arg(&args, "--body-size")
        .unwrap_or_else(|| "256".to_string())
        .parse()
        .expect("--body-size must be a number");

    let fail_rate: f64 = parse_arg(&args, "--fail-rate")
        .unwrap_or_else(|| "0.0".to_string())
        .parse()
        .expect("--fail-rate must be a float");

    let config = Arc::new(AppConfig {
        delay_ms,
        body: vec![b'x'; body_size],
        fail_rate,
    });

    let app = Router::new()
        .route("/health", any(|| async { "ok" }))
        .fallback(any(handler))
        .with_state(config);

    let addr = format!("127.0.0.1:{}", port);
    eprintln!("Mock upstream listening on {}", addr);
    eprintln!("  delay_ms={}, body_size={}, fail_rate={}", delay_ms, body_size, fail_rate);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
