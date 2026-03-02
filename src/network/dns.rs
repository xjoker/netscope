use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use anyhow::{Context, Result, anyhow};
use reqwest::{Client, Url};
use serde::Deserialize;

use crate::network::IpFamily;
use crate::network::client::{build_aux_client, build_client};
use crate::speed::backends::SpeedBackend;
use crate::speed::download::download_once;
use crate::speed::ping::measure_ping;
use crate::util::{compare_optional_f64, short_error};

#[derive(Debug, Clone, Copy)]
pub struct DohProvider {
    pub name: &'static str,
    pub endpoint: &'static str,
    pub use_accept_header: bool,
    /// Whether ECS (edns_client_subnet) is supported (AliDNS/DNSPod return 400 if sent)
    pub use_ecs: bool,
    /// Whether to send type as integer ("1"/"28") instead of string ("A"/"AAAA")
    /// Required by 360 DNS /resolution endpoint
    pub use_numeric_type: bool,
}

#[derive(Debug, Clone)]
pub struct SelectedTarget {
    pub ip: IpAddr,
    pub family: IpFamily,
    pub source: String,
}

/// Probe details for a single candidate IP, for the caller to include in the report
#[derive(Debug, Clone)]
pub struct CandidateResult {
    pub ip: IpAddr,
    pub sources: Vec<String>,
    pub rtt_ms: Option<f64>,
    pub download_ok: bool,
    pub selected: bool,
    /// Geolocation (country / region / city ISP)
    pub location: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct CandidateProbe {
    rtt_ms: Option<f64>,
    download_ok: bool,
}

#[derive(Debug, Deserialize)]
struct DnsJsonResponse {
    #[serde(rename = "Status")]
    status: Option<u32>,
    #[serde(rename = "Answer")]
    answer: Option<Vec<DnsAnswer>>,
}

#[derive(Debug, Deserialize)]
struct DnsAnswer {
    #[serde(rename = "type")]
    record_type: u16,
    data: String,
}

// CN-first: AliDNS/DNSPod/360DNS have no ECS but see direct CN source IP in mainland
pub const DOH_PROVIDERS_CN_FIRST: [DohProvider; 5] = [
    DohProvider { name: "AliDNS",    endpoint: "https://dns.alidns.com/dns-query",  use_accept_header: true,  use_ecs: false, use_numeric_type: false },
    DohProvider { name: "DNSPod",    endpoint: "https://doh.pub/dns-query",          use_accept_header: true,  use_ecs: false, use_numeric_type: false },
    DohProvider { name: "Cloudflare",endpoint: "https://cloudflare-dns.com/dns-query",use_accept_header: true, use_ecs: true,  use_numeric_type: false },
    DohProvider { name: "Google",    endpoint: "https://dns.google/resolve",         use_accept_header: false, use_ecs: true,  use_numeric_type: false },
    DohProvider { name: "Quad9",     endpoint: "https://dns.quad9.net/dns-query",    use_accept_header: true,  use_ecs: true,  use_numeric_type: false },
];

// Global-first: ECS-capable providers with real egress IP
pub const DOH_PROVIDERS_GLOBAL: [DohProvider; 3] = [
    DohProvider { name: "Google",    endpoint: "https://dns.google/resolve",          use_accept_header: false, use_ecs: true,  use_numeric_type: false },
    DohProvider { name: "Cloudflare",endpoint: "https://cloudflare-dns.com/dns-query",use_accept_header: true,  use_ecs: true,  use_numeric_type: false },
    DohProvider { name: "Quad9",     endpoint: "https://dns.quad9.net/dns-query",     use_accept_header: true,  use_ecs: true,  use_numeric_type: false },
];

pub fn normalize_country_code(cc: &str) -> Option<String> {
    let norm = cc.trim().to_uppercase();
    if norm.len() == 2 && norm.chars().all(|c| c.is_ascii_alphabetic()) {
        Some(norm)
    } else {
        None
    }
}

fn ecs_subnet_for_country(country: &str) -> Option<&'static str> {
    match country {
        "CN" => Some("202.96.128.0/24"),
        "MO" => Some("223.5.5.0/24"),
        "TW" => Some("168.95.1.0/24"),
        "HK" => Some("202.181.7.0/24"),
        "JP" => Some("210.130.1.0/24"),
        "KR" => Some("168.126.63.0/24"),
        "SG" => Some("203.116.1.0/24"),
        "US" => Some("8.8.8.0/24"),
        "DE" => Some("9.9.9.0/24"),
        _ => None,
    }
}

/// Truncate the real egress IP to /24 (IPv4) or /48 (IPv6) and pass it as ECS in the DoH query,
/// so the resolver returns the CDN node nearest to that egress IP rather than the node nearest
/// to the resolver itself.
fn ecs_subnet_from_ip(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            format!("{}.{}.{}.0/24", o[0], o[1], o[2])
        }
        IpAddr::V6(v6) => {
            let o = v6.octets();
            format!(
                "{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}::/48",
                o[0], o[1], o[2], o[3], o[4], o[5]
            )
        }
    }
}

pub async fn resolve_host_ip(host: &str) -> Option<IpAddr> {
    let mut ips = Vec::new();
    for addr in tokio::net::lookup_host((host, 443)).await.ok()? {
        ips.push(addr.ip());
    }
    ips.into_iter().next()
}

fn provider_list(country: Option<&str>) -> &'static [DohProvider] {
    match country {
        Some("CN") | Some("MO") | Some("TW") | Some("HK") => &DOH_PROVIDERS_CN_FIRST,
        _ => &DOH_PROVIDERS_GLOBAL,
    }
}

async fn query_doh_provider(
    client: &Client,
    provider: DohProvider,
    host: &str,
    country: Option<&str>,
    family: IpFamily,
    egress_ip: Option<IpAddr>,
    ecs_override: Option<&str>,
    verbose: bool,
) -> Result<Vec<IpAddr>> {
    let mut uniq_ips = BTreeSet::new();
    // ecs_override takes priority (used during CN multi-subnet concurrent queries); then real egress IP /24; finally a static country subnet.
    let ecs_owned: Option<String> = egress_ip.map(ecs_subnet_from_ip);
    let ecs: Option<&str> = ecs_override
        .or_else(|| ecs_owned.as_deref())
        .or_else(|| country.and_then(ecs_subnet_for_country));

    if verbose {
        eprintln!(
            "[dns] provider={} host={} family={:?} ecs={:?}",
            provider.name, host, family, ecs
        );
    }

    let record_type = match (family, provider.use_numeric_type) {
        (IpFamily::V4, false) => "A",
        (IpFamily::V6, false) => "AAAA",
        (IpFamily::V4, true)  => "1",
        (IpFamily::V6, true)  => "28",
    };
    {
        let mut req = client
            .get(provider.endpoint)
            .query(&[("name", host), ("type", record_type)]);
        if provider.use_accept_header {
            req = req.header("accept", "application/dns-json");
        }
        if provider.use_ecs {
            if let Some(subnet) = ecs {
                req = req.query(&[("edns_client_subnet", subnet)]);
            }
        }

        let resp = req.send().await.with_context(|| {
            format!("request DoH failed: {} ({record_type})", provider.endpoint)
        })?;
        if !resp.status().is_success() {
            return Err(anyhow!("doh status {}", resp.status()));
        }

        let body: DnsJsonResponse = resp.json().await.context("decode dns-json failed")?;
        if body.status.unwrap_or(0) != 0 {
            return Err(anyhow!("dns status {}", body.status.unwrap_or(0)));
        }

        if let Some(answers) = body.answer {
            for answer in answers {
                if (answer.record_type == 1 || answer.record_type == 28)
                    && let Ok(ip) = answer.data.parse::<IpAddr>()
                {
                    uniq_ips.insert(ip);
                }
            }
        }
    }

    if uniq_ips.is_empty() {
        return Err(anyhow!("dns status unavailable"));
    }
    let ips: Vec<IpAddr> = uniq_ips.into_iter().collect();
    if verbose {
        eprintln!("[dns] provider={} → {} IP(s): {:?}", provider.name, ips.len(), ips);
    }
    Ok(ips)
}

async fn probe_candidate_ip(
    base_url: &Url,
    ip: IpAddr,
    timeout_secs: u64,
    proxy: Option<&str>,
) -> Result<CandidateProbe> {
    let probe_timeout = timeout_secs.max(1).min(3);
    let client = build_client(base_url, ip, probe_timeout, proxy)?;
    // Always use the domain-name URL:
    // - Without proxy: .resolve() in build_client has already pinned the IP; TLS SNI works correctly.
    // - With proxy: the proxy handles DNS resolution and routing; we fully delegate and do not force a specific IP.
    let req_url = base_url.clone();
    let rtt_ms = measure_ping(&client, &req_url, 1, SpeedBackend::Apple)
        .await
        .ok()
        .map(|(ms, _)| ms);
    let download_ok = probe_download_once(&client, &req_url).await.is_ok();
    Ok(CandidateProbe {
        rtt_ms,
        download_ok,
    })
}

async fn probe_download_once(client: &Client, base_url: &Url) -> Result<()> {
    let url = base_url.join("/api/v1/gm/large")?;
    let _ = download_once(client, url, "bytes=0-65535".to_string(), 64 * 1024).await?;
    Ok(())
}

pub async fn select_best_ip(
    base_url: &Url,
    host: &str,
    timeout_secs: u64,
    country: Option<&str>,
    family: IpFamily,
    proxy: Option<&str>,
    egress_ip: Option<IpAddr>,
    verbose: bool,
) -> Result<(SelectedTarget, Vec<CandidateResult>)> {
    let client = build_aux_client(timeout_secs.max(1).min(6), proxy)
        .context("build dns resolver client failed")?;

    let is_cn = country.map(|c| c == "CN").unwrap_or(false);
    let mut source_map: BTreeMap<IpAddr, BTreeSet<String>> = BTreeMap::new();
    let mut errors = Vec::new();

    if is_cn {
        // CN mode: fire ALL DoH queries in a single concurrent phase.
        //
        // Three groups:
        //   A. Non-ECS providers (AliDNS / DNSPod) — no ECS field, result
        //      depends on source IP seen by the resolver, useful when the DoH
        //      request naturally exits via a CN ISP.
        //   B. ECS-capable providers (Cloudflare / Google / Quad9) with the
        //      real egress IP /24 — tells Apple CDN the user is in CN.
        //   C. ECS-capable providers × a wider set of CN backbone subnets —
        //      covers cases where Apple CDN returns HK for the user's egress
        //      /24 but serves mainland nodes for other city/ISP subnets.
        //
        // By running everything concurrently we get the widest candidate pool
        // in a single round-trip time, then probe + GeoIP + sort all at once.

        const ECS_CAPABLE: &[DohProvider] = &[
            DohProvider { name: "Cloudflare", endpoint: "https://cloudflare-dns.com/dns-query", use_accept_header: true,  use_ecs: true,  use_numeric_type: false },
            DohProvider { name: "Google",     endpoint: "https://dns.google/resolve",           use_accept_header: false, use_ecs: true,  use_numeric_type: false },
            DohProvider { name: "Quad9",      endpoint: "https://dns.quad9.net/dns-query",      use_accept_header: true,  use_ecs: true,  use_numeric_type: false },
        ];
        const NON_ECS: &[DohProvider] = &[
            DohProvider { name: "AliDNS", endpoint: "https://dns.alidns.com/dns-query",  use_accept_header: true, use_ecs: false, use_numeric_type: false },
            DohProvider { name: "DNSPod", endpoint: "https://doh.pub/dns-query",          use_accept_header: true, use_ecs: false, use_numeric_type: false },
            // 360 DNS: /resolution path accepts JSON with numeric type= field, returns many mainland IPs
            DohProvider { name: "360DNS", endpoint: "https://doh.360.cn/resolution",      use_accept_header: true, use_ecs: false, use_numeric_type: true },
        ];
        // Representative CN backbone subnets across ISPs and cities.
        // Apple CDN may return different PoPs for different ECS subnets.
        const CN_ECS_SUBNETS: &[&str] = &[
            "202.96.128.0/24",  // Shanghai Telecom
            "61.135.169.0/24",  // Beijing Unicom
            "221.130.33.0/24",  // Beijing Mobile
            "218.85.0.0/24",    // Fuzhou Telecom
            "125.33.0.0/24",    // Beijing Telecom
            "117.136.0.0/24",   // Mobile
        ];

        let mut tasks = tokio::task::JoinSet::new();

        // Group A: non-ECS providers
        for &p in NON_ECS {
            let client2 = client.clone();
            let host_owned = host.to_string();
            let fam = family;
            let v = verbose;
            tasks.spawn(async move {
                let result = query_doh_provider(
                    &client2, p, &host_owned, Some("CN"), fam, egress_ip, None, v,
                ).await;
                (p.name, result)
            });
        }

        // Group B: ECS-capable providers with the real egress IP /24
        for &p in ECS_CAPABLE {
            let client2 = client.clone();
            let host_owned = host.to_string();
            let fam = family;
            let v = verbose;
            tasks.spawn(async move {
                let result = query_doh_provider(
                    &client2, p, &host_owned, Some("CN"), fam, egress_ip, None, v,
                ).await;
                (p.name, result)
            });
        }

        // Group C: ECS-capable providers × backbone subnets
        // Use only Cloudflare + Google (fastest) to keep total tasks reasonable.
        const ECS_FOR_BACKBONE: &[DohProvider] = &[
            DohProvider { name: "Cloudflare", endpoint: "https://cloudflare-dns.com/dns-query", use_accept_header: true,  use_ecs: true, use_numeric_type: false },
            DohProvider { name: "Google",     endpoint: "https://dns.google/resolve",           use_accept_header: false, use_ecs: true, use_numeric_type: false },
        ];
        for &ecs_subnet in CN_ECS_SUBNETS {
            // Skip if this subnet matches the egress_ip /24 (already covered by group B)
            let skip = egress_ip.map(|ip| {
                let derived = ecs_subnet_from_ip(ip);
                derived.starts_with(&ecs_subnet[..ecs_subnet.len().min(derived.len().min(ecs_subnet.len()))])
                    || ecs_subnet.starts_with(&derived[..derived.len().min(ecs_subnet.len())])
            }).unwrap_or(false);
            if skip { continue; }

            for &p in ECS_FOR_BACKBONE {
                let client2 = client.clone();
                let host_owned = host.to_string();
                let ecs_s = ecs_subnet.to_string();
                let fam = family;
                tasks.spawn(async move {
                    let result = query_doh_provider(
                        &client2, p, &host_owned, Some("CN"), fam, None, Some(&ecs_s), false,
                    ).await;
                    (p.name, result)
                });
            }
        }

        while let Some(joined) = tasks.join_next().await {
            match joined {
                Ok((pname, Ok(ips))) if !ips.is_empty() => {
                    if verbose {
                        eprintln!("[dns] CN {pname} → {} IP(s): {:?}", ips.len(), ips);
                    }
                    for ip in ips {
                        source_map.entry(ip).or_default().insert(pname.to_string());
                    }
                }
                Ok((pname, Ok(_)))    => errors.push(format!("{pname}: no A/AAAA")),
                Ok((pname, Err(err))) => errors.push(format!("{pname}: {}", short_error(&err))),
                Err(_) => {}
            }
        }
    } else {
        // Non-CN mode: query all global providers concurrently, using the real egress IP or country subnet as ECS.
        let mut tasks = tokio::task::JoinSet::new();
        for provider in provider_list(country) {
            let client2 = client.clone();
            let host_owned = host.to_string();
            let country_owned = country.map(str::to_string);
            let pname = provider.name;
            let pendpoint = provider.endpoint;
            let paccept = provider.use_accept_header;
            let pecs = provider.use_ecs;
            let pnum = provider.use_numeric_type;
            let fam = family;
            let v = verbose;
            tasks.spawn(async move {
                let p = DohProvider { name: pname, endpoint: pendpoint, use_accept_header: paccept, use_ecs: pecs, use_numeric_type: pnum };
                let result = query_doh_provider(
                    &client2, p, &host_owned, country_owned.as_deref(), fam, egress_ip, None, v,
                ).await;
                (pname, result)
            });
        }
        while let Some(joined) = tasks.join_next().await {
            match joined {
                Ok((pname, Ok(ips))) if !ips.is_empty() => {
                    for ip in ips {
                        source_map.entry(ip).or_default().insert(pname.to_string());
                    }
                }
                Ok((pname, Ok(_))) => errors.push(format!("{pname}: no A/AAAA")),
                Ok((pname, Err(err))) => errors.push(format!("{pname}: {}", short_error(&err))),
                Err(_) => {}
            }
        }
    }

    if source_map.is_empty() {
        // All DoH providers failed — fall back to system DNS via tokio::net::lookup_host.
        // This keeps the tool functional on networks that block DoH (e.g. corporate firewalls,
        // certain ISPs) at the cost of no ECS-based CDN node selection.
        if verbose {
            eprintln!("[dns] all DoH providers failed ({}); falling back to system DNS", errors.join(" | "));
        }
        let sys_ips: Vec<IpAddr> = match tokio::net::lookup_host(format!("{host}:443")).await {
            Ok(addrs) => addrs
                .map(|s| s.ip())
                .filter(|ip| match family {
                    IpFamily::V4 => ip.is_ipv4(),
                    IpFamily::V6 => ip.is_ipv6(),
                })
                .collect(),
            Err(_) => vec![],
        };
        if sys_ips.is_empty() {
            return Err(anyhow!("all doh providers fail, system DNS also returned no results: {}", errors.join(" | ")));
        }
        for ip in sys_ips {
            source_map.entry(ip).or_default().insert("SysDNS".to_string());
        }
    }

    let mut candidates = source_map
        .into_iter()
        .map(|(ip, sources)| (ip, sources.into_iter().collect::<Vec<_>>()))
        .collect::<Vec<_>>();
    // Sort descending by occurrence count (number of ISPs covering this IP), take the top 12 for probing
    candidates.sort_by(|a, b| {
        b.1.len()
            .cmp(&a.1.len())
            .then_with(|| a.0.to_string().cmp(&b.0.to_string()))
    });
    candidates.truncate(12);

    // Unified for CN / non-CN: probe candidate IPs concurrently and select the node with the lowest latency that can also download successfully.
    let mut tasks = tokio::task::JoinSet::new();
    for (ip, sources) in candidates {
        let base = base_url.clone();
        let proxy_owned = proxy.map(str::to_string);
        tasks.spawn(async move {
            let probe = probe_candidate_ip(&base, ip, timeout_secs, proxy_owned.as_deref())
                .await
                .unwrap_or(CandidateProbe {
                    rtt_ms: None,
                    download_ok: false,
                });
            (ip, sources, probe)
        });
    }

    let mut evaluated = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        evaluated.push(joined.context("probe candidate task failed")?);
    }

    if verbose {
        for (ip, srcs, probe) in &evaluated {
            eprintln!(
                "[dns] candidate {} (from {:?}): rtt={:?} dl_ok={}",
                ip, srcs, probe.rtt_ms, probe.download_ok
            );
        }
    }

    // GeoIP for all candidates
    let geo_timeout: u64 = 3;
    let mut geo_tasks: tokio::task::JoinSet<(IpAddr, Option<String>)> = tokio::task::JoinSet::new();
    for (ip, _, _) in &evaluated {
        let ip = *ip;
        let proxy_owned = proxy.map(str::to_string);
        geo_tasks.spawn(async move {
            let loc = crate::network::geo::lookup_ip_location(ip, geo_timeout, proxy_owned.as_deref())
                .await
                .ok()
                .flatten();
            (ip, loc)
        });
    }
    let mut geo_map: std::collections::HashMap<IpAddr, Option<String>> = std::collections::HashMap::new();
    while let Some(Ok((ip, loc))) = geo_tasks.join_next().await {
        geo_map.insert(ip, loc);
    }

    // Sort: mainland nodes first (geo starts with "China/"), then rtt
    let is_cn_path = country == Some("CN");
    evaluated.sort_by(|a, b| {
        let dl_cmp = b.2.download_ok.cmp(&a.2.download_ok);
        if dl_cmp != std::cmp::Ordering::Equal { return dl_cmp; }

        // CN path: mainland nodes take priority over non-mainland nodes
        if is_cn_path {
            let a_cn = geo_map.get(&a.0)
                .and_then(|v| v.as_deref())
                .map(|s| s.starts_with("China/"))
                .unwrap_or(false);
            let b_cn = geo_map.get(&b.0)
                .and_then(|v| v.as_deref())
                .map(|s| s.starts_with("China/"))
                .unwrap_or(false);
            let geo_cmp = b_cn.cmp(&a_cn); // true > false, mainland first
            if geo_cmp != std::cmp::Ordering::Equal { return geo_cmp; }
        }

        // Within the same region, sort by RTT ascending
        compare_optional_f64(a.2.rtt_ms, b.2.rtt_ms)
            .then_with(|| b.1.len().cmp(&a.1.len()))
            .then_with(|| a.0.to_string().cmp(&b.0.to_string()))
    });

    let best = evaluated
        .first()
        .ok_or_else(|| anyhow!("no available candidate"))?;
    let best_ip = best.0;

    let candidate_results: Vec<CandidateResult> = evaluated
        .iter()
        .map(|(ip, sources, probe)| CandidateResult {
            ip: *ip,
            sources: sources.clone(),
            rtt_ms: probe.rtt_ms,
            download_ok: probe.download_ok,
            selected: *ip == best_ip,
            location: geo_map.get(ip).cloned().flatten(),
        })
        .collect();

    Ok((
        SelectedTarget {
            ip: best_ip,
            family,
            source: best.1.join("+"),
        },
        candidate_results,
    ))
}
