pub mod methods;
pub mod target;
pub mod types;

use std::collections::HashMap;
use std::sync::Arc;

use reqwest::Client;
use serde::Deserialize;
use tokio::sync::{Mutex, Semaphore};

use crate::network::client::build_aux_client;
use crate::probe::methods::execute_probe;
use crate::probe::types::{ProbeGeoInfo, ProbeReport, ProbeResult, ProbeTarget};

// ─── GeoIP cache ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct IpWhoIsGeoResp {
    success: bool,
    country: Option<String>,
    country_code: Option<String>,
    region: Option<String>,
    city: Option<String>,
    connection: Option<IpWhoIsConn>,
}

#[derive(Debug, Deserialize)]
struct IpWhoIsConn {
    isp: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IpSbGeoResp {
    country: Option<String>,
    country_code: Option<String>,
    region: Option<String>,
    city: Option<String>,
    isp: Option<String>,
}

struct GeoCache {
    cache: Mutex<HashMap<String, Option<ProbeGeoInfo>>>,
    client: Client,
    timeout_secs: u64,
}

impl GeoCache {
    fn new(client: Client, timeout_secs: u64) -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            client,
            timeout_secs,
        }
    }

    async fn lookup(&self, ip_str: &str) -> Option<ProbeGeoInfo> {
        // Check cache first
        {
            let cache = self.cache.lock().await;
            if let Some(entry) = cache.get(ip_str) {
                return entry.clone();
            }
        }

        let result = self.query_geo(ip_str).await;

        // Write into cache
        {
            let mut cache = self.cache.lock().await;
            cache.insert(ip_str.to_string(), result.clone());
        }
        result
    }

    async fn query_geo(&self, ip_str: &str) -> Option<ProbeGeoInfo> {
        let timeout = std::time::Duration::from_secs(self.timeout_secs.max(3).min(8));

        // Primary source: ipwho.is
        let primary_url = format!("https://ipwho.is/{ip_str}");
        if let Some(resp) = tokio::time::timeout(timeout, self.client.get(&primary_url).send())
            .await
            .ok()
            .and_then(|r| r.ok())
        {
            if resp.status().is_success() {
                if let Ok(body) = resp.json::<IpWhoIsGeoResp>().await {
                    if body.success {
                        return Some(ProbeGeoInfo {
                            country_code: body.country_code,
                            country: body.country,
                            region: body.region,
                            city: body.city,
                            isp: body.connection.and_then(|c| c.isp),
                        });
                    }
                }
            }
        }

        // Fallback source: api.ip.sb
        let fallback_url = format!("https://api.ip.sb/geoip/{ip_str}");
        let resp = tokio::time::timeout(timeout, self.client.get(&fallback_url).send())
            .await
            .ok()
            .and_then(|r| r.ok())?;

        if !resp.status().is_success() {
            return None;
        }
        let body: IpSbGeoResp = resp.json().await.ok()?;
        Some(ProbeGeoInfo {
            country_code: body.country_code,
            country: body.country,
            region: body.region,
            city: body.city,
            isp: body.isp,
        })
    }
}

// ─── Concurrent dispatch ────────────────────────────────────────────────────

pub async fn run_probe(
    targets: Vec<ProbeTarget>,
    concurrency: usize,
    timeout_secs: u64,
    proxy: Option<&str>,
    skip_geo: bool,
) -> Vec<ProbeResult> {
    let client = match build_aux_client(timeout_secs.max(3), proxy) {
        Ok(c) => c,
        Err(_) => {
            return targets
                .iter()
                .map(|t| ProbeResult {
                    name: t.name.to_string(),
                    category: t.category.to_string(),
                    url: t.url.to_string(),
                    reachable: false,
                    status_code: None,
                    ttfb_ms: None,
                    exit_ip: None,
                    colo: None,
                    loc: None,
                    geo: None,
                    error: Some("client init failed".to_string()),
                })
                .collect();
        }
    };

    let geo_cache = Arc::new(GeoCache::new(client.clone(), timeout_secs));
    let sem = Arc::new(Semaphore::new(concurrency.max(1)));
    let total = targets.len();
    let done_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    // Send target list first so TUI knows the full list, then initial progress (0/N)
    crate::tui::send(crate::tui::state::Event::ProbeInit {
        targets: targets.iter().map(|t| (t.name.to_string(), t.category.to_string())).collect(),
    });
    crate::tui::send(crate::tui::state::Event::ProbeProgress { done: 0, total });

    let mut handles = Vec::with_capacity(total);

    for (idx, target) in targets.into_iter().enumerate() {
        let sem = Arc::clone(&sem);
        let geo_cache = Arc::clone(&geo_cache);
        let client = client.clone();
        let skip_geo = skip_geo;
        let done_counter = Arc::clone(&done_counter);

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.expect("probe semaphore closed unexpectedly");
            let mut result = execute_probe(&client, &target, timeout_secs).await;

            if !skip_geo {
                if let Some(ref ip_str) = result.exit_ip {
                    result.geo = geo_cache.lookup(ip_str).await;
                }
            }

            // Report progress after each probe completes
            let done = done_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            crate::tui::send(crate::tui::state::Event::ProbePartial { result: result.clone() });
            crate::tui::send(crate::tui::state::Event::ProbeProgress { done, total });

            (idx, result)
        });
        handles.push(handle);
    }

    let mut indexed: Vec<(usize, ProbeResult)> = Vec::with_capacity(handles.len());
    for handle in handles {
        match handle.await {
            Ok(pair) => indexed.push(pair),
            Err(e) => {
                eprintln!("probe task panicked: {e}");
            }
        }
    }
    // Sort in original order
    indexed.sort_by_key(|(i, _)| *i);
    indexed.into_iter().map(|(_, r)| r).collect()
}

// ─── Output ─────────────────────────────────────────────────────────────────

const CATEGORY_ORDER: &[&str] = &[
    "ai", "social", "streaming", "search", "news",
    "game", "dev", "cloud", "crypto", "nsfw", "cn",
];

fn category_display(cat: &str) -> &str {
    match cat {
        "ai"        => "AI",
        "social"    => "Social",
        "streaming" => "Streaming",
        "search"    => "Search",
        "news"      => "News",
        "game"      => "Game",
        "dev"       => "Dev",
        "cloud"     => "Cloud",
        "crypto"    => "Crypto",
        "nsfw"      => "NSFW",
        "cn"        => "CN",
        other       => other,
    }
}

pub fn output_probe_report(report: &ProbeReport, is_json: bool) {
    if is_json {
        match serde_json::to_string_pretty(report) {
            Ok(s) => println!("{s}"),
            Err(e) => eprintln!("JSON serialize failed: {e}"),
        }
        return;
    }

    let proxy_str = report.proxy.as_deref().unwrap_or("-");

    println!("========================= Connectivity Probe Results =========================");
    println!("Proxy    : {proxy_str}");

    // Show CN-side / global-side egress IPs and locations
    fn fmt_ip_geo(ip: Option<&str>, geo: Option<&str>) -> String {
        match (ip, geo) {
            (Some(i), Some(g)) => format!("{i} ({g})"),
            (Some(i), None)    => i.to_string(),
            _                  => "-".to_string(),
        }
    }
    let v4_cn  = fmt_ip_geo(report.egress_ipv4_cn.as_deref(),     report.egress_ipv4_cn_geo.as_deref());
    let v4_gl  = fmt_ip_geo(report.egress_ipv4_global.as_deref(), report.egress_ipv4_global_geo.as_deref());
    let v6_cn  = fmt_ip_geo(report.egress_ipv6_cn.as_deref(),     report.egress_ipv6_cn_geo.as_deref());
    let v6_gl  = fmt_ip_geo(report.egress_ipv6_global.as_deref(), report.egress_ipv6_global_geo.as_deref());

    println!("Egress(CN)    : v4={v4_cn}");
    println!("Egress(Global): v4={v4_gl}");
    if report.egress_ipv6.is_some() {
        println!("Egress(CN)    : v6={v6_cn}");
        println!("Egress(Global): v6={v6_gl}");
    }
    println!(
        "Sites: {}  reachable: {}  blocked: {}",
        report.total, report.reachable, report.unreachable
    );
    println!();

    // Output grouped by category
    for &cat in CATEGORY_ORDER {
        let group: Vec<&ProbeResult> = report
            .results
            .iter()
            .filter(|r| r.category == cat)
            .collect();
        if group.is_empty() {
            continue;
        }

        let label = category_display(cat);
        println!(
            "── {label} {}",
            "─".repeat(60usize.saturating_sub(label.len() + 4))
        );
        println!(
            "{:<16} {:<6} {:<18} {:<6} {:<16} {}",
            "Site", "Status", "Egress IP", "Colo", "Region", "TTFB(ms)"
        );

        for r in &group {
            let status = if r.reachable {
                format!("OK({})", r.status_code.unwrap_or(0))
            } else {
                "FAIL".to_string()
            };

            let ip = r.exit_ip.as_deref().unwrap_or("-");
            let colo = r.colo.as_deref().unwrap_or("-");

            // Region: prefer geo information; fall back to loc field
            let region = if let Some(ref geo) = r.geo {
                let cc = geo.country_code.as_deref().unwrap_or("");
                let reg = geo.region.as_deref().unwrap_or("");
                if reg.is_empty() {
                    cc.to_string()
                } else {
                    format!("{cc}/{reg}")
                }
            } else {
                r.loc.as_deref().unwrap_or("-").to_string()
            };

            let ttfb = r
                .ttfb_ms
                .map(|v| format!("{v:.1}"))
                .unwrap_or_else(|| "-".to_string());

            println!(
                "{:<16} {:<6} {:<18} {:<6} {:<16} {}",
                truncate(r.name.as_str(), 15),
                truncate(&status, 6),
                truncate(ip, 18),
                truncate(colo, 6),
                truncate(&region, 16),
                ttfb,
            );

            if let Some(ref err) = r.error {
                println!("  error: {err}");
            }
        }
        println!();
    }

    println!("============================================================");
}

fn truncate(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else {
        let mut truncated: String = chars[..max_chars.saturating_sub(1)].iter().collect();
        truncated.push('…');
        truncated
    }
}
