use std::time::Instant;

use reqwest::Client;

use crate::probe::types::{ProbeMethod, ProbeResult, ProbeTarget};

// Number of timing samples per probe.  The first sample warms up the TCP/TLS
// connection; subsequent samples measure steady-state latency.  Displayed
// ttfb_ms is the average of samples [1..].
const PROBE_SAMPLES: usize = 3;

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
async fn sample_ttfb(client: &Client, url: &str, timeout_secs: u64) -> Option<f64> {
    let t = Instant::now();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        client.get(url).send(),
    )
    .await;
    match result {
        Ok(Ok(resp)) if resp.status().as_u16() < 600 => {
            Some(t.elapsed().as_secs_f64() * 1000.0)
        }
        _ => None,
    }
}

/// Collect PROBE_SAMPLES timing samples for a reachable site and return the
/// average of samples[1..] (skipping the cold-start first sample).
async fn avg_ttfb(client: &Client, url: &str, timeout_secs: u64, first_ms: f64) -> f64 {
    if PROBE_SAMPLES <= 1 {
        return first_ms;
    }
    let mut samples = vec![first_ms];
    for _ in 1..PROBE_SAMPLES {
        if let Some(ms) = sample_ttfb(client, url, timeout_secs).await {
            samples.push(ms);
        }
    }
    // Average of samples[1..] if available, otherwise fall back to all samples
    let warm: Vec<f64> = if samples.len() > 1 {
        samples[1..].to_vec()
    } else {
        samples
    };
    warm.iter().sum::<f64>() / warm.len() as f64
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
            // timeout → fallback to http
            return probe_http(client, target, timeout_secs).await;
        }
        Ok(Err(_e)) => {
            // request error → fallback to http
            return probe_http(client, target, timeout_secs).await;
        }
        Ok(Ok(resp)) => {
            let first_ms = start.elapsed().as_secs_f64() * 1000.0;
            let status_code = resp.status().as_u16();
            if !resp.status().is_success() {
                // non-2xx → fallback
                return probe_http(client, target, timeout_secs).await;
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

            // No ip field → fallback to http probe
            if exit_ip.is_none() {
                let _ = fl;
                return probe_http(client, target, timeout_secs).await;
            }

            // Warm averaged latency from additional samples
            let ttfb_ms = avg_ttfb(client, target.url, timeout_secs, first_ms).await;

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
                avg_ttfb(client, target.url, timeout_secs, first_ms).await
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
            let ttfb_ms = avg_ttfb(client, target.url, timeout_secs, first_ms).await;

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
            let reachable   = status.is_success();

            let exit_ip = target.header_key.and_then(|key| {
                resp.headers()
                    .get(key)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string())
            });

            let ttfb_ms = if reachable {
                avg_ttfb(client, target.url, timeout_secs, first_ms).await
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
