//! Requests to Songbird's own servers through the proxy (PROTOCOL §1.1, D-0164).
//!
//! With a proxy set, every request to Songbird's own hosts (the api, which also serves media,
//! GIFs and the backup vault, and billing) goes through it, as the connection does: TLS to the
//! proxy, the server's own TLS inside. Nothing falls back to a direct connection: a proxy that
//! can't be reached, or one that doesn't carry a host, is an error the user sees. Destinations
//! the user chose themselves (their own WebDAV, S3 or cloud drive for backups) are reached
//! directly: a proxy carries only Songbird's hosts.

use std::{sync::Arc, time::Duration};

use songbird_backup_storage::{
    StoreError,
    http::{Http, HyperHttp, Request, Response},
};
use songbird_client::https::{ProxyHttpError, Reply, Tunnel};

use super::{Problem, endpoint, problem};

/// How long one request through the proxy may take (attachments included).
const TIMEOUT: Duration = Duration::from_secs(300);

/// The host of an `http(s)://` URL, lowercase.
fn host_of(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = match authority.rsplit_once(':') {
        Some((h, p)) if p.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => authority,
    };
    Some(host.to_ascii_lowercase())
}

/// Songbird's own hosts, as this device is set up to reach them.
fn own_hosts() -> Vec<String> {
    let ep = endpoint();
    [ep.api.as_str(), ep.billing.as_str()]
        .into_iter()
        .filter_map(host_of)
        .collect()
}

/// The proxy route, if a proxy is set (`Some(Err)` when it is set but unusable).
#[must_use]
pub fn tunnel() -> Option<Result<Tunnel, Problem>> {
    let host =
        crate::store::try_store().and_then(|s| s.settings.with_untracked(|x| x.proxy.clone()))?;
    let route =
        songbird_types::proxy_link::ProxyLink::parse(&format!("songbird://proxy#{host}"), &[]).ok();
    Some(match (route, songbird_client::web_tls()) {
        (Some(link), Some(tls)) => Ok(Tunnel {
            proxy: songbird_client::ProxyRoute::from_link(&link, Arc::clone(&tls)),
            inner: tls,
            timeout: TIMEOUT,
        }),
        _ => Err(problem("The proxy address isn't valid. Check it in Proxy.")),
    })
}

/// The tunnel for `url`: `None` when no proxy is set or `url` is not one of Songbird's hosts.
#[must_use]
pub fn for_url(url: &str) -> Option<Result<Tunnel, Problem>> {
    let host = host_of(url)?;
    if !own_hosts().contains(&host) {
        return None;
    }
    tunnel()
}

fn explain(e: &ProxyHttpError) -> Problem {
    problem(match e {
        ProxyHttpError::Url => {
            "A proxy carries only secure (https) addresses. Check the server address."
        }
        ProxyHttpError::Proxy => "Can't reach the proxy. Check it in Proxy, or turn it off.",
        ProxyHttpError::Upstream => {
            "The proxy didn't reach Songbird. It may not carry this server; ask whoever runs it."
        }
        ProxyHttpError::TooLarge => "The server's answer was too large.",
        ProxyHttpError::Timeout => "The proxy took too long to answer.",
        ProxyHttpError::Request => "The request through the proxy failed.",
    })
}

/// One request through the proxy, on NSH's tokio runtime.
///
/// # Errors
/// A [`Problem`] saying what failed.
pub async fn through(
    t: Tunnel,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    follow: bool,
) -> Result<Reply, Problem> {
    nsh_core::prelude::background(
        async move { t.request(&method, &url, &headers, body, follow).await },
    )
    .await
    .map_err(|_| problem("The request through the proxy stopped."))?
    .map_err(|e| explain(&e))
}

/// Backup stores' HTTP: Songbird's vault through the proxy when one is set, every other
/// destination directly. The route is taken when the store is made (on the UI thread).
pub struct AppHttp {
    direct: HyperHttp,
    tunnel: Option<Tunnel>,
    own: Vec<String>,
}

impl AppHttp {
    /// # Errors
    /// A [`Problem`] when TLS can't start or the proxy setting is unusable.
    pub fn new() -> Result<Self, Problem> {
        Ok(Self {
            direct: HyperHttp::new(&[])
                .map_err(|_| problem("This device couldn't start a secure connection."))?,
            tunnel: tunnel().transpose()?,
            own: own_hosts(),
        })
    }
}

#[async_trait::async_trait]
impl Http for AppHttp {
    async fn send(&self, req: Request) -> Result<Response, StoreError> {
        let own = host_of(&req.url).is_some_and(|h| self.own.contains(&h));
        match (&self.tunnel, own) {
            (Some(t), true) => {
                let r = t
                    .request(&req.method, &req.url, &req.headers, req.body, false)
                    .await
                    .map_err(|_| StoreError::Unavailable { status: 0 })?;
                Ok(Response {
                    status: r.status,
                    headers: r.headers,
                    body: r.body,
                })
            }
            _ => self.direct.send(req).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_read_from_urls() {
        assert_eq!(
            host_of("https://API.example.org:8443/v1").as_deref(),
            Some("api.example.org")
        );
        assert_eq!(
            host_of("http://127.0.0.1:8080").as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(host_of("ftp://x"), None);
    }
}
