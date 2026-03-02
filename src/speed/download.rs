use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use reqwest::Url;
use reqwest::Version;
use reqwest::header::{ACCEPT_ENCODING, RANGE};
use tokio::sync::Mutex;
use tokio::time::sleep;

use crate::report::{CdnMeta, DownloadStageMetric, SpeedStats};
use crate::speed::backends::SpeedBackend;
use crate::util::{short_error, speed_stats_from_samples};

#[derive(Debug, Clone, Copy)]
pub struct DownloadStagePlan {
    pub name: &'static str,
    pub duration_secs: u64,
    pub concurrency: usize,
    pub chunk_mib: u64,
}

#[derive(Debug)]
pub struct DownloadRunResult {
    pub best_mbps: Option<f64>,
    pub download_stats: Option<SpeedStats>,
    pub stages: Vec<DownloadStageMetric>,
    pub cdn_meta: Option<CdnMeta>,
}

/// Basic download: returns transferred byte count only.
pub async fn download_once(
    client: &Client,
    url: Url,
    range_value: String,
    chunk_limit: u64,
) -> Result<u64> {
    let mut resp = client
        .get(url)
        .header(ACCEPT_ENCODING, "identity")
        .header(RANGE, range_value)
        .send()
        .await
        .context("download request failed")?;
    if !resp.status().is_success() {
        return Err(anyhow!("download status {}", resp.status()));
    }

    let mut total = 0_u64;
    while total < chunk_limit {
        let next = resp.chunk().await.context("download read chunk failed")?;
        match next {
            Some(chunk) => {
                total += chunk.len() as u64;
                if total >= chunk_limit {
                    break;
                }
            }
            None => break,
        }
    }
    Ok(total.min(chunk_limit))
}

/// Extended download: returns byte count, CDN metadata, and whether a Range
/// fallback occurred (server responded 200 instead of 206).
pub async fn download_once_with_meta(
    client: &Client,
    url: Url,
    range_value: String,
    chunk_limit: u64,
) -> Result<(u64, CdnMeta, bool)> {
    let mut resp = client
        .get(url)
        .header(ACCEPT_ENCODING, "identity")
        .header(RANGE, range_value)
        .send()
        .await
        .context("download request failed")?;
    if !resp.status().is_success() {
        return Err(anyhow!("download status {}", resp.status()));
    }

    // Detect Range fallback: 200 means the server ignored the Range header.
    let range_fallback = resp.status() == reqwest::StatusCode::OK;

    let http_version = match resp.version() {
        Version::HTTP_11 => "HTTP/1.1",
        Version::HTTP_2 => "HTTP/2",
        Version::HTTP_3 => "HTTP/3",
        _ => "HTTP/?",
    }
    .to_string();

    let via = resp
        .headers()
        .get("via")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let x_cache = resp
        .headers()
        .get("x-cache")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let age = resp
        .headers()
        .get("age")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let server = resp
        .headers()
        .get("server")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let meta = CdnMeta {
        via,
        x_cache,
        age,
        server,
        http_version,
    };

    let mut total = 0_u64;
    while total < chunk_limit {
        let next = resp.chunk().await.context("download read chunk failed")?;
        match next {
            Some(chunk) => {
                total += chunk.len() as u64;
                if total >= chunk_limit {
                    break;
                }
            }
            None => break,
        }
    }
    Ok((total.min(chunk_limit), meta, range_fallback))
}

/// Download speed test with per-request sampling: records each Mbps sample after every
/// download_once call, then applies IQR trimming at the end of the window to compute SpeedStats.
/// Returns (avg_mbps_after_trim, SpeedStats).
pub async fn measure_download(
    client: &Client,
    url: Url,
    duration_secs: u64,
    concurrency: usize,
    chunk_mib: u64,
) -> Result<(f64, SpeedStats)> {
    if duration_secs == 0 || concurrency == 0 || chunk_mib == 0 {
        return Err(anyhow!("download args must be > 0"));
    }

    let chunk_bytes = chunk_mib * 1024 * 1024;
    let range_value = format!("bytes=0-{}", chunk_bytes - 1);
    let started = Instant::now();
    let deadline = started + Duration::from_secs(duration_secs);
    let total_bytes = Arc::new(AtomicU64::new(0));
    let total_errors = Arc::new(AtomicU64::new(0));
    // Each concurrent worker collects its own samples (one Mbps value per request)
    let samples_shared: Arc<Mutex<Vec<f64>>> = Arc::new(Mutex::new(Vec::new()));

    let mut tasks = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        let client = client.clone();
        let url = url.clone();
        let range_value = range_value.clone();
        let total_bytes = Arc::clone(&total_bytes);
        let total_errors = Arc::clone(&total_errors);
        let samples_shared = Arc::clone(&samples_shared);

        tasks.push(tokio::spawn(async move {
            while Instant::now() < deadline {
                let req_start = Instant::now();
                match download_once(&client, url.clone(), range_value.clone(), chunk_bytes).await {
                    Ok(n) if n > 0 => {
                        total_bytes.fetch_add(n, AtomicOrdering::Relaxed);
                        let elapsed = req_start.elapsed().as_secs_f64();
                        if elapsed > 0.0 {
                            let mbps = (n as f64 * 8.0) / elapsed / 1_000_000.0;
                            samples_shared.lock().await.push(mbps);
                        }
                    }
                    Ok(_) => {
                        total_errors.fetch_add(1, AtomicOrdering::Relaxed);
                        sleep(Duration::from_millis(30)).await;
                    }
                    Err(_) => {
                        total_errors.fetch_add(1, AtomicOrdering::Relaxed);
                        sleep(Duration::from_millis(60)).await;
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        }));
    }

    for task in tasks {
        task.await.context("download task join failed")??;
    }

    let elapsed = started.elapsed().as_secs_f64();
    if elapsed <= 0.0 {
        return Err(anyhow!("download elapsed time invalid"));
    }
    let bytes = total_bytes.load(AtomicOrdering::Relaxed) as f64;
    if bytes <= 0.0 {
        let errors = total_errors.load(AtomicOrdering::Relaxed);
        return Err(anyhow!(
            "download no bytes transferred in window (errors={errors})"
        ));
    }

    // Compute SpeedStats from per-request samples (after IQR trimming)
    let raw_samples = {
        let lock = samples_shared.lock().await;
        lock.clone()
    };
    let stats = speed_stats_from_samples(raw_samples);
    // Use p90_mbps (after trimming) as the representative value for this stage — more indicative of sustained throughput than max
    let representative = stats.p90_mbps;

    Ok((representative, stats))
}

pub fn build_download_plan(duration_secs: u64) -> Vec<DownloadStagePlan> {
    let total = duration_secs.max(6);
    let first = (total / 3).max(2);
    let second = (total / 3).max(2);
    let third = total.saturating_sub(first + second).max(2);

    vec![
        DownloadStagePlan {
            name: "single",
            duration_secs: first,
            concurrency: 1,
            chunk_mib: 1,
        },
        DownloadStagePlan {
            name: "small",
            duration_secs: second,
            concurrency: 2,
            chunk_mib: 1,
        },
        DownloadStagePlan {
            name: "multi",
            duration_secs: third,
            concurrency: 6,
            chunk_mib: 4,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── build_download_plan ───────────────────────────────────────────────────

    #[test]
    fn plan_has_three_stages() {
        let plan = build_download_plan(20);
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0].name, "single");
        assert_eq!(plan[1].name, "small");
        assert_eq!(plan[2].name, "multi");
    }

    #[test]
    fn plan_concurrency_ordering() {
        let plan = build_download_plan(20);
        assert_eq!(plan[0].concurrency, 1);
        assert_eq!(plan[1].concurrency, 2);
        assert_eq!(plan[2].concurrency, 6);
    }

    #[test]
    fn plan_total_duration_reasonable() {
        // For 20s: first=6, second=6, third=max(20-12,2)=8 → total ≥ 20
        let plan = build_download_plan(20);
        let total: u64 = plan.iter().map(|p| p.duration_secs).sum();
        assert!(total >= 20, "expected total duration ≥ 20, got {total}");
    }

    #[test]
    fn plan_minimum_duration_enforced() {
        // Even with duration_secs=0, each stage must be ≥ 2s
        let plan = build_download_plan(0);
        for stage in &plan {
            assert!(
                stage.duration_secs >= 2,
                "stage {} has duration {}s < 2",
                stage.name,
                stage.duration_secs
            );
        }
    }

    #[test]
    fn plan_large_duration_scales() {
        let plan = build_download_plan(60);
        let total: u64 = plan.iter().map(|p| p.duration_secs).sum();
        // Sum should be at least 60 seconds
        assert!(total >= 60, "expected ≥ 60s total, got {total}");
    }

    #[test]
    fn plan_first_stage_chunk_is_1mib() {
        let plan = build_download_plan(20);
        assert_eq!(plan[0].chunk_mib, 1, "warm-up stage must use 1 MiB chunks");
    }
}

pub async fn run_download_tests(
    client: &Client,
    base_url: &Url,
    duration_secs: u64,
    backend: SpeedBackend,
    stage_cb: Option<Box<dyn Fn(f64) + Send>>,
) -> Result<DownloadRunResult> {
    let plans = build_download_plan(duration_secs);
    let mut stages = Vec::with_capacity(plans.len());
    let mut best_mbps: Option<f64> = None;
    // Collect SpeedStats from the last stage (multi) to write into the report
    let mut last_stats: Option<SpeedStats> = None;

    // Capture CDN metadata from the very first request using OnceLock.
    let meta_lock: Arc<OnceLock<CdnMeta>> = Arc::new(OnceLock::new());
    let range_fallback_total = Arc::new(AtomicU64::new(0));

    let mut is_first_stage = true;
    // warmup_mbps: set after the first stage (single) completes; used for adaptive chunk sizing in subsequent stages
    let mut warmup_mbps: Option<f64> = None;

    for plan in &plans {
        // Adaptive chunk size: target ~2s per request per worker
        // chunk_mib = clamp(ceil(warmup_mbps × 2 / 8 / concurrency), 2, 64)
        // Divide by concurrency to avoid oversized single-request bodies that may cause CDN rejection/timeout
        // First stage is fixed at 1 MiB as a warm-up probe
        let chunk_mib = match warmup_mbps {
            Some(w) if w > 0.0 => {
                let conc = plan.concurrency.max(1) as f64;
                let target = (w * 2.0 / 8.0 / conc).ceil() as u64;
                target.max(2).min(64)
            }
            _ => plan.chunk_mib,
        };

        let chunk_bytes = chunk_mib * 1024 * 1024;
        let url = backend.download_url(base_url, chunk_bytes)?;

        // On the very first stage, attempt to collect metadata via
        // download_once_with_meta before running the timed measurement.
        if is_first_stage {
            is_first_stage = false;
            let range_val = format!("bytes=0-{}", chunk_bytes - 1);
            if let Ok((_, meta, fallback)) =
                download_once_with_meta(client, url.clone(), range_val, chunk_bytes).await
            {
                let _ = meta_lock.set(meta);
                if fallback {
                    range_fallback_total.fetch_add(1, AtomicOrdering::Relaxed);
                }
            }
        }

        match measure_download(
            client,
            url,
            plan.duration_secs,
            plan.concurrency,
            chunk_mib,
        )
        .await
        {
            Ok((v, stage_stats)) => {
                // Single-stage result used as warm-up rate to guide adaptive chunk sizing in later stages
                if warmup_mbps.is_none() {
                    warmup_mbps = Some(v);
                }
                best_mbps = Some(best_mbps.map_or(v, |current| current.max(v)));
                last_stats = Some(stage_stats);
                if let Some(ref cb) = stage_cb {
                    cb(v);
                }
                stages.push(DownloadStageMetric {
                    name: plan.name.to_string(),
                    duration_secs: plan.duration_secs,
                    concurrency: plan.concurrency,
                    chunk_mib,
                    mbps: Some(v),
                    error: None,
                });
            }
            Err(err) => {
                // Even if the first stage fails, mark it as attempted so subsequent stages fall back to the default chunk size
                if warmup_mbps.is_none() {
                    warmup_mbps = Some(0.0);
                }
                stages.push(DownloadStageMetric {
                    name: plan.name.to_string(),
                    duration_secs: plan.duration_secs,
                    concurrency: plan.concurrency,
                    chunk_mib,
                    mbps: None,
                    error: Some(short_error(&err)),
                });
            }
        }
    }

    // Extract captured metadata (if any).
    let cdn_meta = Arc::try_unwrap(meta_lock)
        .ok()
        .and_then(|lock| lock.into_inner());

    Ok(DownloadRunResult {
        best_mbps,
        download_stats: last_stats,
        stages,
        cdn_meta,
    })
}
