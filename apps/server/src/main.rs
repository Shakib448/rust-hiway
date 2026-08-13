use std::net::SocketAddr;
use std::time::Duration;

use hiway_http::{AppState, Logger, proxy_handler};
use hiway_observability::init_tracing;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use hyper_util::service::TowerToHyperService;
use tokio::net::{TcpListener, TcpStream};
use tokio::signal;
use tower::{ServiceBuilder, service_fn};

const GATEWAY_ADDR: &str = "127.0.0.1:3000";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    init_tracing();

    let gateway_addr: SocketAddr = GATEWAY_ADDR.parse()?;

    let mut connector = HttpConnector::new();
    connector.set_connect_timeout(Some(Duration::from_secs(5)));
    let client = Client::builder(TokioExecutor::new()).build(connector);
    let state = AppState { client };

    let listener = TcpListener::bind(gateway_addr).await?;
    tracing::info!(address = %gateway_addr, "Hiway API Gateway started");

    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, peer_addr)) => {
                        tracing::debug!(%peer_addr, "Accepted connection");
                        let state = state.clone();
                        tokio::spawn(async move {
                            serve_connection(stream, peer_addr, state).await;
                        });
                    }
                    Err(err) => {
                        tracing::error!(?err, "Failed to accept connection");
                    }
                }
            }
            _ = &mut shutdown => {
                tracing::info!("Shutdown signal received");
                break;
            }
        }
    }

    tracing::info!("Hiway stopped accepting new connections");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

async fn serve_connection(stream: TcpStream, peer_addr: SocketAddr, state: AppState) {
    if let Err(err) = stream.set_nodelay(true) {
        tracing::debug!(%peer_addr, ?err, "Failed to enable TCP_NODELAY");
    }

    let io = TokioIo::new(stream);
    let svc = service_fn(move |req| {
        let state = state.clone();
        async move { proxy_handler(req, state).await }
    });
    let svc = ServiceBuilder::new().layer_fn(Logger::new).service(svc);
    let svc = TowerToHyperService::new(svc);

    if let Err(err) = auto::Builder::new(TokioExecutor::new())
        .serve_connection(io, svc)
        .await
    {
        tracing::debug!(%peer_addr, ?err, "HTTP connection closed");
    }
}
