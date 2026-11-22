//! HTTPS requests through a Songbird proxy (PROTOCOL §1.1, D-0164).
//!
//! The same route as the connection: TCP and TLS to the proxy (its own name), then TLS to the
//! Songbird endpoint inside it, with the endpoint's name as SNI. The proxy routes by that inner
//! SNI to its allowlist and sees nothing after the inner `ClientHello`, so it learns which
//! Songbird service is reached and how much crosses, never what. A host the proxy does not
//! carry is refused by the proxy; nothing ever falls back to a direct connection.

use std::sync::Arc;
use std::time::Duration;

use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Bytes;
use tokio::net::TcpStream;

use crate::ProxyRoute;

/// The most a response body may hold (an attachment's largest padded ciphertext and then some).
pub const MAX_BODY: usize = 1 << 31;
/// Redirects followed (to HTTPS only, each through the proxy again).
const MAX_REDIRECTS: usize = 5;

/// A response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    /// Header names lowercase, in order.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Why a request through the proxy failed.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProxyHttpError {
    #[error("not an https URL")]
    Url,
    #[error("the proxy could not be reached")]
    Proxy,
    #[error("the proxy refused the destination or the server could not be reached through it")]
    Upstream,
    #[error("the request failed")]
    Request,
    #[error("the response was too large")]
    TooLarge,
    #[error("timed out")]
    Timeout,
}

/// (host, authority, path and query) of an `https://` URL.
fn split(url: &str) -> Result<(String, String, String), ProxyHttpError> {
    let rest = url.strip_prefix("https://").ok_or(ProxyHttpError::Url)?;
    let (authority, path) = match rest.find(['/', '?']) {
        Some(i) => (&rest[..i], rest[i..].to_owned()),
        None => (rest, "/".to_owned()),
    };
    let path = if path.starts_with('?') {
        format!("/{path}")
    } else {
        path
    };
    if authority.is_empty() || authority.contains('@') {
        return Err(ProxyHttpError::Url);
    }
    let host = match authority.rsplit_once(':') {
        Some((h, p)) if !h.contains(']') && p.parse::<u16>().is_ok() => h,
        Some((_, _)) if authority.starts_with('[') => return Err(ProxyHttpError::Url),
        _ => authority,
    };
    if host.is_empty() {
        return Err(ProxyHttpError::Url);
    }
    Ok((host.to_ascii_lowercase(), authority.to_owned(), path))
}

/// The proxy route for HTTPS: the proxy, the TLS settings inside it and a per-request timeout.
#[derive(Clone, Debug)]
pub struct Tunnel {
    pub proxy: ProxyRoute,
    /// TLS settings for the session with the Songbird endpoint, inside the proxy's.
    pub inner: Arc<rustls::ClientConfig>,
    /// Each request (and each redirect) must finish within this.
    pub timeout: Duration,
}

impl Tunnel {
    /// One request through the proxy; with `follow`, redirects (`GET`/`HEAD` only, to HTTPS)
    /// are followed through it too.
    ///
    /// # Errors
    /// [`ProxyHttpError`] when the URL is not HTTPS, the proxy or the server can't be reached,
    /// or the response is too large.
    pub async fn request(
        &self,
        method: &str,
        url: &str,
        headers: &[(String, String)],
        body: Vec<u8>,
        follow: bool,
    ) -> Result<Reply, ProxyHttpError> {
        let mut url = url.to_owned();
        for _ in 0..=MAX_REDIRECTS {
            let (status, location, reply) = tokio::time::timeout(
                self.timeout,
                once(
                    &self.proxy,
                    Arc::clone(&self.inner),
                    method,
                    &url,
                    headers,
                    body.clone(),
                ),
            )
            .await
            .map_err(|_| ProxyHttpError::Timeout)??;
            let redirect =
                follow && matches!(method, "GET" | "HEAD") && (301..=308).contains(&status);
            match location.filter(|_| redirect) {
                Some(loc) if loc.starts_with("https://") => url = loc,
                Some(loc) if loc.starts_with('/') => {
                    let (_, authority, _) = split(&url)?;
                    url = format!("https://{authority}{loc}");
                }
                _ => return Ok(reply),
            }
        }
        Err(ProxyHttpError::Request)
    }
}

async fn once(
    proxy: &ProxyRoute,
    inner: Arc<rustls::ClientConfig>,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Vec<u8>,
) -> Result<(u16, Option<String>, Reply), ProxyHttpError> {
    let (host, authority, path) = split(url)?;
    let tcp = TcpStream::connect(&proxy.addr)
        .await
        .map_err(|_| ProxyHttpError::Proxy)?;
    tcp.set_nodelay(true).map_err(|_| ProxyHttpError::Proxy)?;
    let outer_name = rustls_pki_types::ServerName::try_from(proxy.host.clone())
        .map_err(|_| ProxyHttpError::Proxy)?;
    let outer = tokio_rustls::TlsConnector::from(Arc::clone(&proxy.tls))
        .connect(outer_name, tcp)
        .await
        .map_err(|_| ProxyHttpError::Proxy)?;
    let name = rustls_pki_types::ServerName::try_from(host).map_err(|_| ProxyHttpError::Url)?;
    // HTTP/1.1 only inside: the ALPN the inner config offers is left as configured.
    let stream = tokio_rustls::TlsConnector::from(inner)
        .connect(name, outer)
        .await
        .map_err(|_| ProxyHttpError::Upstream)?;
    let (mut sender, conn) =
        hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(stream))
            .await
            .map_err(|_| ProxyHttpError::Upstream)?;
    let driver = tokio::spawn(async move {
        let _ = conn.await;
    });
    let mut req = hyper::Request::builder()
        .method(method)
        .uri(path)
        .header(hyper::header::HOST, authority);
    for (k, v) in headers {
        if !k.eq_ignore_ascii_case("host") {
            req = req.header(k.as_str(), v.as_str());
        }
    }
    let req = req
        .body(Full::new(Bytes::from(body)))
        .map_err(|_| ProxyHttpError::Request)?;
    let res = sender
        .send_request(req)
        .await
        .map_err(|_| ProxyHttpError::Request)?;
    let status = res.status().as_u16();
    let headers: Vec<(String, String)> = res
        .headers()
        .iter()
        .filter_map(|(k, v)| Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned())))
        .collect();
    let location = headers
        .iter()
        .find(|(k, _)| k == "location")
        .map(|(_, v)| v.clone());
    let body = Limited::new(res.into_body(), MAX_BODY)
        .collect()
        .await
        .map_err(|e| {
            if e.downcast_ref::<http_body_util::LengthLimitError>()
                .is_some()
            {
                ProxyHttpError::TooLarge
            } else {
                ProxyHttpError::Request
            }
        })?
        .to_bytes()
        .to_vec();
    driver.abort();
    Ok((
        status,
        location,
        Reply {
            status,
            headers,
            body,
        },
    ))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]
    use super::*;

    #[test]
    fn urls_split_strictly() {
        assert_eq!(
            split("https://API.example.org/v1/x?y=1").unwrap(),
            (
                "api.example.org".into(),
                "API.example.org".into(),
                "/v1/x?y=1".into()
            )
        );
        assert_eq!(
            split("https://api.example.org:8443").unwrap(),
            (
                "api.example.org".into(),
                "api.example.org:8443".into(),
                "/".into()
            )
        );
        assert_eq!(split("http://api.example.org/"), Err(ProxyHttpError::Url));
        assert_eq!(
            split("https://u@api.example.org/"),
            Err(ProxyHttpError::Url)
        );
        assert_eq!(split("https:///x"), Err(ProxyHttpError::Url));
    }
}
