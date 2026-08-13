use http::header::{CONNECTION, HeaderMap, HeaderName};

const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-connection",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// Strip RFC 9110 hop-by-hop headers, including names listed in `Connection`.
pub fn strip_hop_by_hop(headers: &mut HeaderMap) {
    let extra: Vec<HeaderName> = headers
        .get_all(CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|token| {
            let token = token.trim();
            if token.is_empty() {
                None
            } else {
                HeaderName::from_bytes(token.as_bytes()).ok()
            }
        })
        .collect();

    for name in extra {
        headers.remove(name);
    }
    for name in HOP_BY_HOP {
        headers.remove(*name);
    }
}

#[cfg(test)]
mod tests {
    use http::header::{AUTHORIZATION, CONTENT_TYPE, TRANSFER_ENCODING, UPGRADE};

    use super::*;

    #[test]
    fn strips_standard_hop_by_hop_keeps_end_to_end() {
        let mut headers = HeaderMap::new();
        headers.insert(CONNECTION, "close".parse().unwrap());
        headers.insert("keep-alive", "timeout=5".parse().unwrap());
        headers.insert(TRANSFER_ENCODING, "chunked".parse().unwrap());
        headers.insert(UPGRADE, "websocket".parse().unwrap());
        headers.insert(CONTENT_TYPE, "text/plain".parse().unwrap());
        headers.insert(AUTHORIZATION, "Bearer x".parse().unwrap());

        strip_hop_by_hop(&mut headers);

        assert!(headers.get(CONNECTION).is_none());
        assert!(headers.get("keep-alive").is_none());
        assert!(headers.get(TRANSFER_ENCODING).is_none());
        assert!(headers.get(UPGRADE).is_none());
        assert_eq!(headers[CONTENT_TYPE], "text/plain");
        assert_eq!(headers[AUTHORIZATION], "Bearer x");
    }

    #[test]
    fn strips_headers_named_in_connection() {
        let mut headers = HeaderMap::new();
        headers.insert(CONNECTION, "close, x-foo".parse().unwrap());
        headers.insert("x-foo", "1".parse().unwrap());
        headers.insert("x-bar", "2".parse().unwrap());

        strip_hop_by_hop(&mut headers);

        assert!(headers.get("x-foo").is_none());
        assert_eq!(headers["x-bar"], "2");
    }
}
