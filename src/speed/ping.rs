use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use reqwest::Url;
use reqwest::header::ACCEPT_ENCODING;
use tokio::time::sleep;

use crate::report::PingStats;
use crate::speed::backends::SpeedBackend;
use crate::util::{median, percentile_sorted};

/// TCP connect ping: establish a TCP connection directly to host:port and measure handshake latency.
/// Does not go through a proxy (transparent TCP connect is not possible with a proxy); callers should skip this when a proxy is in use.
pub async fn tcp_ping(host: &str, port: u16, count: u32) -> Result<f64> {
    let addr = format!("{host}:{port}");
    let mut samples = Vec::with_capacity(count as usize);

    for idx in 0..count {
        let start = Instant::now();
        let stream = tokio::net::TcpStream::connect(&addr)
            .await
            .context("TCP connect failed")?;
        drop(stream); // close immediately — we only measure connection time
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
        if idx + 1 < count {
            sleep(Duration::from_millis(100)).await;
        }
    }

    if samples.is_empty() {
        return Err(anyhow!("tcp ping: no samples"));
    }
    let mut sorted = samples.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(median(&mut sorted).unwrap_or(0.0))
}

/// HTTP ping: perform multiple rounds of requests, returning (median_ms, PingStats).
pub async fn measure_ping(
    client: &Client,
    base_url: &Url,
    count: u32,
    backend: SpeedBackend,
) -> Result<(f64, PingStats)> {
    let mut samples = Vec::with_capacity(count as usize);
    let url = backend.ping_url(base_url)?;

    for idx in 0..count {
        let started = Instant::now();
        let resp = client
            .get(url.clone())
            .header(ACCEPT_ENCODING, "identity")
            .send()
            .await
            .context("ping request failed")?;
        resp.error_for_status_ref()
            .context("ping non-success status")?;
        let _ = resp.bytes().await.context("read ping body failed")?;
        samples.push(started.elapsed().as_secs_f64() * 1000.0);

        if idx + 1 < count {
            sleep(Duration::from_millis(120)).await;
        }
    }

    if samples.is_empty() {
        return Err(anyhow!("ping has no samples"));
    }

    let jitter_ms = if samples.len() >= 2 {
        let diffs: Vec<f64> = samples
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .collect();
        diffs.iter().sum::<f64>() / diffs.len() as f64
    } else {
        0.0
    };

    let mut sorted = samples.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let min_ms    = sorted.first().copied().unwrap_or(0.0);
    let max_ms    = sorted.last().copied().unwrap_or(0.0);
    let avg_ms    = samples.iter().sum::<f64>() / samples.len() as f64;
    let median_ms = median(&mut sorted.clone()).unwrap_or(0.0);
    let p95_ms    = percentile_sorted(&sorted, 95.0);

    let ping_stats = PingStats {
        samples: samples.clone(),
        min_ms,
        avg_ms,
        median_ms,
        max_ms,
        jitter_ms,
        p95_ms,
        tcp_rtt_ms: None, // filled in by runner.rs after the call
    };

    Ok((median_ms, ping_stats))
}

