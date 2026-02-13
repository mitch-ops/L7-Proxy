use tokio::net::TcpListener;
use tracing::{info, error};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initilize structured logging
    tracing_subscriber::fmt::init();

    let addr = "127.0.0.1:8080";

    info!("Starting TCP port listener on {}", addr);

    let listener = TcpListener::bind(addr).await?;

    loop {
        match listener.accept().await {
            Ok((socket, peer_addr)) => {
                info!("New connection from {}", peer_addr);

                // Spawn a task per connection
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(socket).await {
                        error!("Connection error: {:?}", e);
                    }
                });
            }
            Err(e) => {
                error!("Failed to accept connection {:?}", e);
            }
        }
    }
}

async fn handle_connection(
    _socket: tokio::net::TcpStream,
) -> Result<(), Box<dyn std::error::Error>> {
    // For now, do nothing
    Ok(())
}
