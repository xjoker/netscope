use std::time::Instant;

use anyhow::{Result, anyhow};
use rand::RngCore;
use reqwest::Client;
use reqwest::Url;
use reqwest::header::{ACCEPT_ENCODING, CONTENT_TYPE};

use crate::report::SpeedStats;
use crate::speed::backends::SpeedBackend;
use crate::util::speed_stats_from_samples;

/// Perform ul_repeat upload rounds, collecting a Mbps sample per round.
/// After IQR trimming, compute SpeedStats and return (median/avg_mbps, SpeedStats).
/// ul_mib is the caller-suggested payload size cap; internally a 1 MiB warm-up probe runs first,
/// then the actual payload size is adapted based on the measured upload rate (targeting ~2 s per round).
pub async fn measure_upload(
    client: &Client,
    base_url: &Url,
    ul_mib: u64,
    ul_repeat: u32,
    backend: SpeedBackend,
) -> Result<(f64, SpeedStats)> {
    if ul_mib == 0 || ul_repeat == 0 {
        return Err(anyhow!("upload args must be > 0"));
    }

    let url = backend.upload_url(base_url)?;
    let mut mbps = Vec::with_capacity(ul_repeat as usize + 1);

    // ── Warm-up probe: send 1 MiB to estimate the actual upload rate ─────────────────────
    let warmup_bytes = 1_usize * 1024 * 1024;
    let warmup_mbps: f64 = {
        let mut warmup_payload = vec![0_u8; warmup_bytes];
        rand::rng().fill_bytes(&mut warmup_payload);
        let t = Instant::now();
        let r = client
            .post(url.clone())
            .header(ACCEPT_ENCODING, "identity")
            .header(CONTENT_TYPE, "application/octet-stream")
            .body(warmup_payload)
            .send()
            .await;
        if let Ok(resp) = r {
            let _ = resp.bytes().await;
            let elapsed = t.elapsed().as_secs_f64().max(0.01);
            let rate = (warmup_bytes as f64 * 8.0) / elapsed / 1_000_000.0;
            mbps.push(rate); // include warm-up sample in statistics
            rate
        } else {
            // Warm-up failed; fall back to a fixed 1 MiB payload
            10.0
        }
    };

    // Adaptive payload size: target ~2 s per upload, clamped to [1, ul_mib] MiB
    let adaptive_mib = {
        let target_mib = (warmup_mbps * 2.0 / 8.0).ceil() as u64;
        target_mib.max(1).min(ul_mib)
    };

    let payload_len = (adaptive_mib * 1024 * 1024) as usize;
    let mut payload = vec![0_u8; payload_len];
    rand::rng().fill_bytes(&mut payload);

    let mut consecutive_errors = 0_u32;
    for _ in 0..ul_repeat {
        let started = Instant::now();
        let result = client
            .post(url.clone())
            .header(ACCEPT_ENCODING, "identity")
            .header(CONTENT_TYPE, "application/octet-stream")
            .body(payload.clone())
            .send()
            .await;
        let resp = match result {
            Ok(r) => r,
            Err(e) => {
                consecutive_errors += 1;
                // A single failure is skipped, but two consecutive failures abort the test
                if consecutive_errors >= 2 {
                    return Err(anyhow!("upload request failed: {}", e));
                }
                continue;
            }
        };
        if !resp.status().is_success() {
            consecutive_errors += 1;
            if consecutive_errors >= 2 {
                return Err(anyhow!("upload status {}", resp.status()));
            }
            continue;
        }
        consecutive_errors = 0;
        // Drain response body to cleanly close the HTTP/2 stream.
        let _ = resp.bytes().await;
        let elapsed = started.elapsed().as_secs_f64();
        let rate = ((payload_len as f64) * 8.0) / elapsed / 1_000_000.0;
        mbps.push(rate);
    }

    if mbps.is_empty() {
        return Err(anyhow!("upload has no samples (all attempts failed)"));
    }

    let stats = speed_stats_from_samples(mbps);
    // Use avg_mbps (after IQR trimming) as the representative value
    let representative = stats.avg_mbps;

    Ok((representative, stats))
}
