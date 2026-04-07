use std::time::Instant;

use reqwest::Client;

use crate::probe::types::{ProbeMethod, ProbeResult, ProbeTarget};

// Number of steady-state timing samples per probe (after warmup).
// The first request warms up the TCP/TLS connection and is discarded.
// After a short cooldown, PROBE_SAMPLES measurements are taken; the
// highest and lowest are dropped and the rest are averaged (trimmed mean).
const PROBE_SAMPLES: usize = 5;

// Brief pause after warmup to let kernel buffers and congestion window settle.
const WARMUP_COOLDOWN_MS: u64 = 50;

pub async fn execute_probe(
    client: &Client,
    target: &ProbeTarget,
    timeout_secs: u64,
) -> ProbeResult {
    match target.method {
        ProbeMethod::Trace     => probe_trace(client, target, timeout_secs).await,
        ProbeMethod::Http      => probe_http(client, target, timeout_secs).await,
        ProbeMethod::ApiDirect => probe_api_direct(client, target, timeout_secs).await,
        ProbeMethod::Header    => probe_header(client, target, timeout_secs).await,
    }
}

fn make_error_result(target: &ProbeTarget, error: String) -> ProbeResult {
    ProbeResult {
        name: target.name.to_string(),
        category: target.category.to_string(),
        url: target.url.to_string(),
        reachable: false,
        status_code: None,
        ttfb_ms: None,
        exit_ip: None,
        colo: None,
        loc: None,
        geo: None,
        error: Some(error),
    }
}

/// Send a single timed GET and return elapsed ms (to first byte).
/// Returns None on timeout or connection error.
/// Note: the reqwest client already has a global timeout configured via `build_aux_client`;
/// we do NOT wrap with an additional `tokio::time::timeout` to avoid double-timeout confusion.
/// Any status code that indicates the server responded (even 4xx) counts as a valid latency
/// sample, since we are measuring network round-trip, not application correctness.
async fn sample_ttfb(client: &Client, url: &str) -> Option<f64> {
    let t = Instant::now();
    match client.get(url).send().await {
        Ok(resp) if resp.status().as_u16() < 500 => {
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            // Consume the response body so the HTTP/1.1 connection can be
            // returned to the pool and reused by subsequent samples.
            // Without this, each sample re-establishes TCP+TLS (~100-300ms overhead).
            let _ = resp.bytes().await;
            Some(ms)
        }
        _ => None,
    }
}

/// Collect PROBE_SAMPLES timing samples for a reachable site and return the
/// trimmed mean (drop highest and lowest, average the rest).
/// The first request (warmup) has already been done by the caller; we sleep
/// briefly, then take PROBE_SAMPLES fresh measurements.
async fn avg_ttfb(client: &Client, url: &str, first_ms: f64) -> f64 {
    // Cooldown after warmup request
    tokio::time::sleep(std::time::Duration::from_millis(WARMUP_COOLDOWN_MS)).await;

    let mut samples = Vec::with_capacity(PROBE_SAMPLES);
    for _ in 0..PROBE_SAMPLES {
        if let Some(ms) = sample_ttfb(client, url).await {
            samples.push(ms);
        }
    }
    if samples.is_empty() {
        return first_ms; // fallback: no successful sample, use warmup value
    }
    if samples.len() <= 2 {
        // Not enough to trim; plain average
        return samples.iter().sum::<f64>() / samples.len() as f64;
    }
    // Trimmed mean: drop min and max, average the rest
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let trimmed = &samples[1..samples.len() - 1];
    trimmed.iter().sum::<f64>() / trimmed.len() as f64
}

async fn probe_trace(client: &Client, target: &ProbeTarget, timeout_secs: u64) -> ProbeResult {
    let start = Instant::now();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        client.get(target.url).send(),
    )
    .await;

    match result {
        Err(_) => {
            // timeout → report as unreachable (don't fallback to probe_http with the same /cdn-cgi/trace URL)
            make_error_result(target, "timeout".to_string())
        }
        Ok(Err(e)) => {
            // request error → report as unreachable
            make_error_result(target, format!("request failed: {e}"))
        }
        Ok(Ok(resp)) => {
            let first_ms = start.elapsed().as_secs_f64() * 1000.0;
            let status_code = resp.status().as_u16();
            if !resp.status().is_success() {
                // Non-2xx: the server responded, so the site IS reachable at the network level.
                // The /cdn-cgi/trace endpoint may be disabled (403/404) — that doesn't mean
                // the site is blocked. Only timeouts and connection failures indicate blocking.
                let ttfb_ms = avg_ttfb(client, target.url, first_ms).await;
                return ProbeResult {
                    name: target.name.to_string(),
                    category: target.category.to_string(),
                    url: target.url.to_string(),
                    reachable: true,
                    status_code: Some(status_code),
                    ttfb_ms: Some(ttfb_ms),
                    exit_ip: None,
                    colo: None,
                    loc: None,
                    geo: None,
                    error: None,
                };
            }
            let body = match resp.text().await {
                Ok(b) => b,
                Err(e) => {
                    return make_error_result(target, format!("read body failed: {e}"));
                }
            };

            let mut exit_ip: Option<String> = None;
            let mut colo: Option<String> = None;
            let mut loc: Option<String> = None;
            let mut fl: Option<String> = None;

            for line in body.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    let k = k.trim();
                    let v = v.trim();
                    if v.is_empty() { continue; }
                    match k {
                        "ip"   => exit_ip = Some(v.to_string()),
                        "colo" => colo    = Some(v.to_string()),
                        "loc"  => loc     = Some(v.to_string()),
                        "fl"   => fl      = Some(v.to_string()),
                        _ => {}
                    }
                }
            }

            // No ip field → not a valid trace response; return error with the first-request latency
            // instead of falling back to probe_http (which would re-request the same /cdn-cgi/trace URL)
            if exit_ip.is_none() {
                let _ = fl;
                return ProbeResult {
                    name: target.name.to_string(),
                    category: target.category.to_string(),
                    url: target.url.to_string(),
                    reachable: true,
                    status_code: Some(status_code),
                    ttfb_ms: Some(first_ms),
                    exit_ip: None,
                    colo,
                    loc,
                    geo: None,
                    error: Some("trace: no ip field".to_string()),
                };
            }

            // Warm averaged latency from additional samples
            let ttfb_ms = avg_ttfb(client, target.url, first_ms).await;

            ProbeResult {
                name: target.name.to_string(),
                category: target.category.to_string(),
                url: target.url.to_string(),
                reachable: true,
                status_code: Some(status_code),
                ttfb_ms: Some(ttfb_ms),
                exit_ip,
                colo,
                loc,
                geo: None,
                error: None,
            }
        }
    }
}

async fn probe_http(client: &Client, target: &ProbeTarget, timeout_secs: u64) -> ProbeResult {
    let start = Instant::now();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        client.get(target.url).send(),
    )
    .await;

    match result {
        Err(_)       => make_error_result(target, "timeout".to_string()),
        Ok(Err(e))   => make_error_result(target, format!("request failed: {e}")),
        Ok(Ok(resp)) => {
            let first_ms  = start.elapsed().as_secs_f64() * 1000.0;
            let status     = resp.status();
            let status_code = status.as_u16();
            let reachable   = status.is_success() || status_code < 500;

            let ttfb_ms = if reachable {
                avg_ttfb(client, target.url, first_ms).await
            } else {
                first_ms
            };

            ProbeResult {
                name: target.name.to_string(),
                category: target.category.to_string(),
                url: target.url.to_string(),
                reachable,
                status_code: Some(status_code),
                ttfb_ms: Some(ttfb_ms),
                exit_ip: None,
                colo: None,
                loc: None,
                geo: None,
                error: None,
            }
        }
    }
}

async fn probe_api_direct(client: &Client, target: &ProbeTarget, timeout_secs: u64) -> ProbeResult {
    let start = Instant::now();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        client.get(target.url).send(),
    )
    .await;

    match result {
        Err(_)       => make_error_result(target, "timeout".to_string()),
        Ok(Err(e))   => make_error_result(target, format!("request failed: {e}")),
        Ok(Ok(resp)) => {
            let first_ms    = start.elapsed().as_secs_f64() * 1000.0;
            let status       = resp.status();
            let status_code  = status.as_u16();
            let reachable    = status.is_success();

            if !reachable {
                return ProbeResult {
                    name: target.name.to_string(),
                    category: target.category.to_string(),
                    url: target.url.to_string(),
                    reachable: false,
                    status_code: Some(status_code),
                    ttfb_ms: Some(first_ms),
                    exit_ip: None,
                    colo: None,
                    loc: None,
                    geo: None,
                    error: Some(format!("HTTP {status_code}")),
                };
            }

            let exit_ip = resp.json::<serde_json::Value>().await.ok().and_then(|v| {
                // IPIP: {"ret":"ok","data":{"ip":"1.2.3.4","location":"..."}}
                // Try data.ip first, then top-level ip
                v.get("data")
                    .and_then(|d| d.get("ip"))
                    .and_then(|ip| ip.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        v.get("ip")
                            .and_then(|ip| ip.as_str())
                            .map(|s| s.to_string())
                    })
            });

            // Additional timing samples for averaged latency
            let ttfb_ms = avg_ttfb(client, target.url, first_ms).await;

            ProbeResult {
                name: target.name.to_string(),
                category: target.category.to_string(),
                url: target.url.to_string(),
                reachable: true,
                status_code: Some(status_code),
                ttfb_ms: Some(ttfb_ms),
                exit_ip,
                colo: None,
                loc: None,
                geo: None,
                error: None,
            }
        }
    }
}

async fn probe_header(client: &Client, target: &ProbeTarget, timeout_secs: u64) -> ProbeResult {
    let start = Instant::now();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        client.get(target.url).send(),
    )
    .await;

    match result {
        Err(_)       => make_error_result(target, "timeout".to_string()),
        Ok(Err(e))   => make_error_result(target, format!("request failed: {e}")),
        Ok(Ok(resp)) => {
            let first_ms   = start.elapsed().as_secs_f64() * 1000.0;
            let status      = resp.status();
            let status_code = status.as_u16();
            // Header method: reachable only on 2xx (server must actually respond with content)
            let reachable   = status.is_success();

            let exit_ip = target.header_key.and_then(|key| {
                resp.headers()
                    .get(key)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string())
            });

            let ttfb_ms = if reachable {
                avg_ttfb(client, target.url, first_ms).await
            } else {
                first_ms
            };

            ProbeResult {
                name: target.name.to_string(),
                category: target.category.to_string(),
                url: target.url.to_string(),
                reachable,
                status_code: Some(status_code),
                ttfb_ms: Some(ttfb_ms),
                exit_ip,
                colo: None,
                loc: None,
                geo: None,
                error: if reachable { None } else { Some(format!("HTTP {status_code}")) },
            }
        }
    }
}
