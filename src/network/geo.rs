use std::net::IpAddr;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::network::client::build_aux_client;
use crate::network::dns::normalize_country_code;

#[derive(Debug, Deserialize)]
struct IpWhoIsResponse {
    success: bool,
    country: Option<String>,
    #[serde(rename = "country_code")]
    country_code: Option<String>,
    region: Option<String>,
    city: Option<String>,
    connection: Option<IpWhoIsConnection>,
}

#[derive(Debug, Deserialize)]
struct IpWhoIsConnection {
    asn: Option<u32>,
    isp: Option<String>,
}

/// Fallback GeoIP provider response format for api.ip.sb/geoip/{ip}.
#[derive(Debug, Deserialize)]
struct IpSbResponse {
    country: Option<String>,
    country_code: Option<String>,
    region: Option<String>,
    city: Option<String>,
    isp: Option<String>,
}

/// Assemble a human-readable location string from optional components.
fn build_location_string(
    country: Option<String>,
    region: Option<String>,
    city: Option<String>,
    meta: Option<String>,
) -> String {
    let mut parts = Vec::new();
    if let Some(c) = country.filter(|v| !v.trim().is_empty()) {
        parts.push(c);
    }
    if let Some(r) = region.filter(|v| !v.trim().is_empty()) {
        parts.push(r);
    }
    if let Some(c) = city.filter(|v| !v.trim().is_empty()) {
        parts.push(c);
    }
    let location = if parts.is_empty() {
        "Unknown".to_string()
    } else {
        parts.join("/")
    };
    if let Some(m) = meta {
        format!("{location} ({m})")
    } else {
        location
    }
}

pub async fn detect_country_by_ip(
    ip: IpAddr,
    timeout_secs: u64,
    proxy: Option<&str>,
) -> Option<String> {
    // Primary: ipwho.is
    if let Some(client) = build_aux_client(timeout_secs.max(1).min(5), proxy).ok() {
        if let Ok(resp) = client.get(format!("https://ipwho.is/{ip}")).send().await {
            if resp.status().is_success() {
                if let Ok(body) = resp.json::<IpWhoIsResponse>().await {
                    if body.success {
                        if let Some(cc) =
                            body.country_code.and_then(|cc| normalize_country_code(&cc))
                        {
                            return Some(cc);
                        }
                    }
                }
            }
        }
    }

    // Fallback: api.ip.sb
    let client = build_aux_client(timeout_secs.max(1).min(5), proxy).ok()?;
    let resp = client
        .get(format!("https://api.ip.sb/geoip/{ip}"))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: IpSbResponse = resp.json().await.ok()?;
    body.country_code.and_then(|cc| normalize_country_code(&cc))
}

/// Query ip.hhh.sd/all/{ip} — returns location string on success, None on any failure.
/// This function is intentionally infallible: all errors are silently swallowed.
async fn lookup_hhhsd(ip: IpAddr, timeout_secs: u64, proxy: Option<&str>) -> Option<String> {
    let client = build_aux_client(timeout_secs.max(1).min(5), proxy).ok()?;
    let resp = client
        .get(format!("https://ip.hhh.sd/all/{ip}"))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;

    // Extract country + city from the "common" block
    let common = body.get("common")?;
    let country = common.get("country").and_then(|v| v.as_str()).map(|s| s.to_string());
    let city    = common.get("city").and_then(|v| v.as_str()).map(|s| s.to_string());

    // Extract ASN + ISP from the first source that has them
    let (asn, isp) = if let Some(sources) = body.get("sources").and_then(|v| v.as_object()) {
        let mut asn_out: Option<u64>   = None;
        let mut isp_out: Option<String> = None;
        for (_name, src) in sources {
            if asn_out.is_none() {
                asn_out = src.get("asn").and_then(|v| v.as_u64());
            }
            if isp_out.is_none() {
                isp_out = src.get("isp")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.to_string());
            }
            if asn_out.is_some() && isp_out.is_some() { break; }
        }
        (asn_out, isp_out)
    } else {
        (None, None)
    };

    let meta = match (asn, isp) {
        (Some(a), Some(i)) => Some(format!("AS{a} | {i}")),
        (Some(a), None)    => Some(format!("AS{a}")),
        (None,    Some(i)) => Some(i),
        _                  => None,
    };

    Some(build_location_string(country, None, city, meta))
}

pub async fn lookup_ip_location(
    ip: IpAddr,
    timeout_secs: u64,
    proxy: Option<&str>,
) -> Result<Option<String>> {
    // Primary: ip.hhh.sd (infallible — failure drops through to next provider)
    if let Some(loc) = lookup_hhhsd(ip, timeout_secs, proxy).await {
        return Ok(Some(loc));
    }

    // Secondary: ipwho.is
    let client =
        build_aux_client(timeout_secs.max(1).min(6), proxy).context("build geo client failed")?;
    let url = format!("https://ipwho.is/{ip}");
    if let Ok(resp) = client.get(url).send().await {
        if resp.status().is_success() {
            if let Ok(body) = resp.json::<IpWhoIsResponse>().await {
                if body.success {
                    let meta = body.connection.map(|conn| {
                        let mut parts = Vec::new();
                        if let Some(asn) = conn.asn {
                            parts.push(format!("AS{asn}"));
                        }
                        if let Some(isp) = conn.isp.filter(|v| !v.trim().is_empty()) {
                            parts.push(isp);
                        }
                        parts.join(" | ")
                    });
                    let meta = meta.filter(|v| !v.is_empty());
                    return Ok(Some(build_location_string(
                        body.country,
                        body.region,
                        body.city,
                        meta,
                    )));
                }
            }
        }
    }

    // Fallback: api.ip.sb/geoip/{ip}
    let fb_client = build_aux_client(timeout_secs.max(1).min(6), proxy)
        .context("build geo fallback client failed")?;
    let resp = fb_client
        .get(format!("https://api.ip.sb/geoip/{ip}"))
        .send()
        .await
        .context("ip.sb geo request failed")?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let body: IpSbResponse = resp
        .json()
        .await
        .context("decode ip.sb geo response failed")?;
    Ok(Some(build_location_string(
        body.country,
        body.region,
        body.city,
        body.isp.filter(|v| !v.trim().is_empty()),
    )))
}
