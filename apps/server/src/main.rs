use std::convert::Infallible;
use std::net::SocketAddr;

use hiway_config::Logger;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use hyper_util::service::TowerToHyperService;
use tokio::net::TcpListener;
use tower::ServiceBuilder;

async fn hello(_: Request<hyper::body::Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    Ok(Response::new(Full::new(Bytes::from("Hello, World!"))))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("listening on http://{addr}");

    let listener = TcpListener::bind(addr).await?;

    loop {
        let (stream, _) = listener.accept().await?;

        let io = TokioIo::new(stream);

        tokio::task::spawn(async move {
            let svc = tower::service_fn(hello);
            let svc = ServiceBuilder::new().layer_fn(Logger::new).service(svc);
            let svc = TowerToHyperService::new(svc);

            if let Err(err) = auto::Builder::new(TokioExecutor::new())
                .serve_connection(io, svc)
                .await
            {
                eprintln!("Error serving connection: {:?}", err);
            }
        });
    }
}
