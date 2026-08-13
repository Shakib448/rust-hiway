use std::convert::Infallible;
use std::time::Duration;

use http_body_util::{Either, Full};
use hyper::body::{Bytes, Incoming};
use hyper::{Request, Response, StatusCode, Uri};
use hyper_util::client::legacy::{Client, connect::HttpConnector};

use crate::headers::strip_hop_by_hop;

const UPSTREAM_ADDR: &str = "http://127.0.0.1:8001";
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(30);

pub type HttpClient = Client<HttpConnector, Incoming>;
type ProxyBody = Either<Incoming, Full<Bytes>>;

#[derive(Clone)]
pub struct AppState {
    pub client: HttpClient,
}

pub async fn proxy_handler(
    req: Request<Incoming>,
    state: AppState,
) -> Result<Response<ProxyBody>, Infallible> {
    let method = req.method().clone();
    let path = req
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");

    tracing::info!(%method, %path, "Proxying request");

    let upstream_uri = match format!("{UPSTREAM_ADDR}{path}").parse::<Uri>() {
        Ok(uri) => uri,
        Err(err) => {
            tracing::error!(?err, "Failed to build upstream URI");
            return Ok(error_response(
                StatusCode::BAD_GATEWAY,
                "Invalid upstream URI",
            ));
        }
    };

    let (mut parts, body) = req.into_parts();
    parts.uri = upstream_uri;
    strip_hop_by_hop(&mut parts.headers);
    parts.headers.remove("host");

    let upstream_request = Request::from_parts(parts, body);

    let upstream_response = match tokio::time::timeout(
        UPSTREAM_TIMEOUT,
        state.client.request(upstream_request),
    )
    .await
    {
        Ok(Ok(response)) => response,
        Ok(Err(err)) => {
            tracing::error!(?err, "Upstream request failed");
            return Ok(error_response(
                StatusCode::BAD_GATEWAY,
                "Upstream service unavailable",
            ));
        }
        Err(_) => {
            tracing::error!(timeout = ?UPSTREAM_TIMEOUT, "Upstream request timed out");
            return Ok(error_response(
                StatusCode::GATEWAY_TIMEOUT,
                "Upstream request timed out",
            ));
        }
    };

    tracing::info!(status = %upstream_response.status(), "Upstream response received");

    let mut response = upstream_response.map(Either::Left);
    strip_hop_by_hop(response.headers_mut());
    Ok(response)
}

fn error_response(status: StatusCode, body: impl Into<Bytes>) -> Response<ProxyBody> {
    let mut response = Response::new(Either::Right(Full::new(body.into())));
    *response.status_mut() = status;
    response
}

#[cfg(test)]
mod tests {
    use http::header::{CONNECTION, CONTENT_TYPE, HOST, TRANSFER_ENCODING};
    use http_body_util::BodyExt;

    use super::*;
    use crate::headers::strip_hop_by_hop;

    #[tokio::test]
    async fn error_response_sets_status_and_body() {
        let response = error_response(StatusCode::BAD_GATEWAY, "nope");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&bytes[..], b"nope");
    }

    #[test]
    fn outbound_request_drops_hop_by_hop_and_host() {
        let mut headers = http::HeaderMap::new();
        headers.insert(HOST, "127.0.0.1:3000".parse().unwrap());
        headers.insert(CONNECTION, "close".parse().unwrap());
        headers.insert(TRANSFER_ENCODING, "chunked".parse().unwrap());
        headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());

        strip_hop_by_hop(&mut headers);
        headers.remove(HOST);

        assert!(headers.get(HOST).is_none());
        assert!(headers.get(CONNECTION).is_none());
        assert!(headers.get(TRANSFER_ENCODING).is_none());
        assert_eq!(headers[CONTENT_TYPE], "application/json");
    }
}
