use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use reqwest::{Client, Url};
use reqwest::header::{HeaderMap, HeaderValue, HOST};

pub fn build_aux_client(timeout_secs: u64, proxy: Option<&str>) -> Result<Client> {
    let mut builder = Client::builder().timeout(Duration::from_secs(timeout_secs.max(1)));
    if let Some(proxy_url) = proxy {
        builder =
            builder.proxy(reqwest::Proxy::all(proxy_url).context("invalid proxy configuration")?);
    } else {
        // Explicitly disable system environment-variable proxies (ALL_PROXY / HTTPS_PROXY, etc.),
        // to prevent CN-path DoH requests from accidentally going through a proxy and causing
        // the resolver to return nodes near the proxy instead of near the client.
        builder = builder.no_proxy();
    }
    builder.build().context("build client failed")
}

/// Auxiliary client with IPv4 egress forced (bound to 0.0.0.0), used for egress IPv4 detection
pub fn build_aux_client_v4(timeout_secs: u64, proxy: Option<&str>) -> Result<Client> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(timeout_secs.max(1)));
    if let Some(proxy_url) = proxy {
        builder =
            builder.proxy(reqwest::Proxy::all(proxy_url).context("invalid proxy configuration")?);
    } else {
        builder = builder.no_proxy();
        builder = builder.local_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    }
    builder.build().context("build ipv4 client failed")
}

/// Auxiliary client for egress IPv6 detection.
/// Does not force-bind local_address(::): v6-only probe endpoints (ipv6.icanhazip.com, etc.)
/// have only AAAA records, so the OS automatically uses IPv6; force-binding actually causes
/// socket creation to fail on macOS.
pub fn build_aux_client_v6(timeout_secs: u64, proxy: Option<&str>) -> Result<Client> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(timeout_secs.max(1)));
    if let Some(proxy_url) = proxy {
        builder =
            builder.proxy(reqwest::Proxy::all(proxy_url).context("invalid proxy configuration")?);
    } else {
        builder = builder.no_proxy();
    }
    // Do not bind local_address(::): v6-only endpoints have only AAAA records, OS uses v6 automatically;
    // force-binding actually causes socket creation failures on macOS.
    builder.build().context("build ipv6 client failed")
}


pub fn build_client(
    base_url: &Url,
    ip: IpAddr,
    timeout_secs: u64,
    proxy: Option<&str>,
) -> Result<Client> {
    let host = base_url
        .host_str()
        .ok_or_else(|| anyhow!("base_url host missing"))?;
    let port = base_url.port_or_known_default().unwrap_or(443);

    // Always set the Host default header: in proxy scenarios the URL host is an IP address,
    // so the Host header is required to carry the domain name;
    // without it the CDN cannot match SNI/vhost and return the correct content.
    let mut default_headers = HeaderMap::new();
    if let Ok(hv) = HeaderValue::from_str(host) {
        default_headers.insert(HOST, hv);
    }

    let mut builder = Client::builder()
        .timeout(Duration::from_secs(timeout_secs.max(1)))
        .default_headers(default_headers);

    if let Some(proxy_url) = proxy {
        // With proxy: use the domain-name URL, delegating all DNS resolution and routing to the proxy.
        builder = builder.proxy(reqwest::Proxy::all(proxy_url).context("invalid proxy configuration")?);
    } else {
        // Without proxy: explicitly disable system env-var proxies, then use .resolve() to pin to the specified IP.
        builder = builder.no_proxy();
        builder = builder.resolve(host, SocketAddr::new(ip, port));
        match ip {
            IpAddr::V4(_) => {
                builder = builder.local_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
            }
            IpAddr::V6(_) => {
                builder = builder.local_address(IpAddr::V6(Ipv6Addr::UNSPECIFIED));
            }
        }
    }
    let client = builder.build().context("build reqwest client failed")?;
    Ok(client)
}

