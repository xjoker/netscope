use std::cmp::Ordering;

use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use reqwest::Url;

use crate::cli::{Command, command_name, sanitize_proxy_display, validate_proxy_url};
use crate::network::IpFamily;
use crate::network::client::build_client;
use crate::network::dns::{
    SelectedTarget, normalize_country_code, resolve_host_ip, select_best_ip,
};
use crate::network::egress::detect_egress_profile;
use crate::network::geo::{detect_country_by_ip, lookup_ip_location};
use crate::output::{StatusKind, emit_start_banner, emit_status};
use crate::report::{PathResult, Report, StackPreflightMetric, StageError};
use crate::speed::backends::SpeedBackend;
use crate::speed::download::{measure_download, run_download_tests};
use crate::speed::ping::{measure_ping, tcp_ping};
use crate::speed::upload::measure_upload;
use crate::tui::state::{Event as TuiEvent, PathRow, StageStatus as TuiStageStatus};
use crate::util::{now_unix, short_error};

fn preflight_metric_for<'a>(
    metrics: &'a [StackPreflightMetric],
    family: IpFamily,
) -> Option<&'a StackPreflightMetric> {
    metrics.iter().find(|m| m.family == family.as_str())
}

fn choose_primary_target(
    targets: &[SelectedTarget],
    preflight_metrics: &[StackPreflightMetric],
) -> Option<SelectedTarget> {
    let mut ranked = targets.to_vec();
    ranked.sort_by(|a, b| {
        let ma = preflight_metric_for(preflight_metrics, a.family);
        let mb = preflight_metric_for(preflight_metrics, b.family);
        let dl_cmp = match (
            ma.and_then(|m| m.download_mbps),
            mb.and_then(|m| m.download_mbps),
        ) {
            (Some(da), Some(db)) => db.partial_cmp(&da).unwrap_or(Ordering::Equal),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        };
        if dl_cmp != Ordering::Equal {
            return dl_cmp;
        }

        let rtt_cmp = match (ma.and_then(|m| m.rtt_ms), mb.and_then(|m| m.rtt_ms)) {
            (Some(ra), Some(rb)) => ra.partial_cmp(&rb).unwrap_or(Ordering::Equal),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        };
        if rtt_cmp != Ordering::Equal {
            return rtt_cmp;
        }

        match (a.family, b.family) {
            (IpFamily::V4, IpFamily::V6) => Ordering::Less,
            (IpFamily::V6, IpFamily::V4) => Ordering::Greater,
            _ => a.ip.to_string().cmp(&b.ip.to_string()),
        }
    });
    ranked.into_iter().next()
}

async fn preflight_stack(
    base_url: &Url,
    target: &SelectedTarget,
    proxy: Option<&str>,
    backend: SpeedBackend,
) -> StackPreflightMetric {
    let client = match build_client(base_url, target.ip, 4, proxy) {
        Ok(c) => c,
        Err(err) => {
            return StackPreflightMetric {
                family: target.family.as_str().to_string(),
                ip: target.ip.to_string(),
                rtt_ms: None,
                download_mbps: None,
                note: Some(short_error(&err)),
            };
        }
    };
    // preflight only takes median_ms, ignoring detailed PingStats
    let rtt_ms = measure_ping(&client, base_url, 1, backend).await.ok().map(|(ms, _)| ms);
    // For preflight, use 1 MiB chunks.
    let chunk_bytes = 1_u64 * 1024 * 1024;
    let dl_url = backend.download_url(base_url, chunk_bytes).ok();
    let download_mbps = if let Some(url) = dl_url {
        // preflight only takes the representative Mbps, ignoring SpeedStats
        measure_download(&client, url, 2, 2, 1).await.ok().map(|(mbps, _)| mbps)
    } else {
        None
    };
    StackPreflightMetric {
        family: target.family.as_str().to_string(),
        ip: target.ip.to_string(),
        rtt_ms,
        download_mbps,
        note: None,
    }
}

/// Build an HTTP client for the Cloudflare backend (no IP pinning needed).
fn build_cloudflare_client(timeout: u64, proxy: Option<&str>) -> Result<Client> {
    // Add 15s buffer on top of the user timeout to account for Cloudflare's
    // server-side processing overhead on __up (typically ~5-6s after body received).
    let effective_timeout = timeout.saturating_add(15);
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(effective_timeout))
        .tcp_keepalive(std::time::Duration::from_secs(30))
        // Force HTTP/1.1: Cloudflare __up over HTTP/2 resets streams on large body uploads.
        .http1_only();
    if let Some(p) = proxy {
        builder = builder.proxy(reqwest::Proxy::all(p).context("invalid proxy")?);
    }
    builder.build().context("build cloudflare client failed")
}

/// Fetch https://speed.cloudflare.com/cdn-cgi/trace and extract node IP, colo, country.
/// Returns (node_ip, location_string, ip_family, country_code).
/// e.g. location_string = "SIN (SG)" where SIN is IATA colo code, SG is country.
async fn fetch_cloudflare_trace(
    client: &Client,
    timeout: u64,
    proxy: Option<&str>,
) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
    // Use a short-timeout client for this probe — don't block test startup.
    let probe_client = {
        let t = timeout.min(6);
        let mut b = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(t));
        if let Some(p) = proxy {
            if let Ok(px) = reqwest::Proxy::all(p) { b = b.proxy(px); }
        }
        match b.build() { Ok(c) => c, Err(_) => client.clone() }
    };

    let resp = match probe_client
        .get("https://speed.cloudflare.com/cdn-cgi/trace")
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => return (None, None, None, None),
    };
    let body = match resp.text().await {
        Ok(t) => t,
        Err(_) => return (None, None, None, None),
    };

    // Parse key=value lines
    let mut ip: Option<String>   = None;
    let mut colo: Option<String> = None;
    let mut loc: Option<String>  = None;

    for line in body.lines() {
        if let Some(v) = line.strip_prefix("ip=")   { ip   = Some(v.trim().to_string()); }
        if let Some(v) = line.strip_prefix("colo=") { colo = Some(v.trim().to_string()); }
        if let Some(v) = line.strip_prefix("loc=")  { loc  = Some(v.trim().to_string()); }
    }

    let family = ip.as_deref().and_then(|s| {
        if s.contains(':') { Some("v6".to_string()) } else { Some("v4".to_string()) }
    });

    // Build a human-readable location string: "SIN (SG)"
    let location = match (&colo, &loc) {
        (Some(c), Some(l)) => Some(format!("{c} ({l})")),
        (Some(c), None)    => Some(c.clone()),
        (None, Some(l))    => Some(l.clone()),
        _                  => None,
    };

    (ip, location, family, loc)
}

pub async fn run(cli: crate::cli::Cli) -> Result<(Report, u8)> {
    let is_json = cli.json;

    // Resolve backend from CLI arg; derive the base URL from backend choice.
    let backend = SpeedBackend::from_str(&cli.backend);
    let base_url_str = match backend {
        SpeedBackend::Cloudflare => "https://speed.cloudflare.com",
        _                        => "https://mensura.cdn-apple.com",
    };
    let parsed = Url::parse(base_url_str).context("invalid base url")?;
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("base url missing host"))?
        .to_string();

    // Destructure CLI fields to avoid borrow/move conflicts.
    let cli_timeout = cli.timeout;
    let cli_country = cli.country;
    let cli_verbose = cli.verbose;
    let proxy_owned = cli.proxy.clone();
    let proxy = proxy_owned.as_deref();

    let command = cli.command.unwrap_or(Command::Full {
        count: 8,
        duration: 20,
        ul_mib: 16,
        ul_repeat: 3,
    });
    let mode = command_name(&command).to_string();
    emit_start_banner(is_json, &mode, &parsed, cli_timeout);
    if let Some(proxy_url) = proxy {
        validate_proxy_url(proxy_url)?;
        emit_status(
            is_json,
            StatusKind::Info,
            "proxy",
            &format!("proxy enabled: {}", sanitize_proxy_display(proxy_url)),
        );
    }

    emit_status(
        is_json,
        StatusKind::Info,
        "egress",
        "detecting egress IPs (ipify/icanhazip/itdog)",
    );
    let egress = detect_egress_profile(cli_timeout, proxy).await;
    emit_status(
        is_json,
        StatusKind::Ok,
        "egress",
        &format!(
            "v4={} v6={} consistent={}",
            egress
                .ipv4
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string()),
            egress
                .ipv6
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string()),
            egress.consistent
        ),
    );

    // Concurrently look up geolocation for egress IPs (up to 4, deduplicated and queried in parallel)
    let geo_timeout = cli_timeout.max(3).min(6);
    let (geo_v4_cn, geo_v4_global, geo_v6_cn, geo_v6_global) = {
        use std::collections::BTreeSet;
        use std::net::IpAddr;
        let unique_ips: BTreeSet<IpAddr> = [
            egress.ipv4_cn,
            egress.ipv4_global,
            egress.ipv6_cn,
            egress.ipv6_global,
        ]
        .into_iter()
        .flatten()
        .collect();
        let mut geo_tasks: tokio::task::JoinSet<(IpAddr, Option<String>)> =
            tokio::task::JoinSet::new();
        for ip in unique_ips {
            let proxy_owned2 = proxy.map(str::to_string);
            let t = geo_timeout;
            geo_tasks.spawn(async move {
                let loc = crate::network::geo::lookup_ip_location(ip, t, proxy_owned2.as_deref())
                    .await
                    .ok()
                    .flatten();
                (ip, loc)
            });
        }
        let mut egress_geo: std::collections::HashMap<std::net::IpAddr, Option<String>> =
            std::collections::HashMap::new();
        while let Some(Ok((ip, loc))) = geo_tasks.join_next().await {
            egress_geo.insert(ip, loc);
        }
        let gv4cn  = egress.ipv4_cn.and_then(|ip| egress_geo.get(&ip).cloned().flatten());
        let gv4gl  = egress.ipv4_global.and_then(|ip| egress_geo.get(&ip).cloned().flatten());
        let gv6cn  = egress.ipv6_cn.and_then(|ip| egress_geo.get(&ip).cloned().flatten());
        let gv6gl  = egress.ipv6_global.and_then(|ip| egress_geo.get(&ip).cloned().flatten());
        (gv4cn, gv4gl, gv6cn, gv6gl)
    };

    crate::tui::send(TuiEvent::EgressDone {
        v4_cn:         egress.ipv4_cn.map(|v| v.to_string()),
        v4_global:     egress.ipv4_global.map(|v| v.to_string()),
        v6_cn:         egress.ipv6_cn.map(|v| v.to_string()),
        v6_global:     egress.ipv6_global.map(|v| v.to_string()),
        v4_cn_geo:     geo_v4_cn.clone(),
        v4_global_geo: geo_v4_global.clone(),
        v6_cn_geo:     geo_v6_cn.clone(),
        v6_global_geo: geo_v6_global.clone(),
    });

    let resolver_country = match cli_country {
        Some(cc) => normalize_country_code(&cc),
        None => {
            // Prefer CN-side IP (itdog) for country detection; fall back to global.
            let ip = egress.ipv4_cn.or(egress.ipv4_global).or(egress.ipv6_cn).or(egress.ipv6_global);
            if let Some(ip) = ip {
                detect_country_by_ip(ip, cli_timeout, proxy).await
            } else {
                None
            }
        }
    };

    // For Cloudflare backend, skip DoH DNS resolution and IP selection.
    // Build a plain client and set target_host to speed.cloudflare.com.
    let (client, selected_ip_str, selected_family_str, selected_source, target_host) =
        if backend == SpeedBackend::Cloudflare {
            let cf_client = build_cloudflare_client(cli_timeout, proxy)?;
            (
                cf_client,
                None::<String>,
                None::<String>,
                "cloudflare-direct".to_string(),
                "speed.cloudflare.com".to_string(),
            )
        } else {
            // Apple backend: DoH resolution + IP selection.
            emit_status(
                is_json,
                StatusKind::Info,
                "resolve",
                &format!(
                    "DoH resolving {} (country={})",
                    host,
                    resolver_country
                        .clone()
                        .unwrap_or_else(|| "auto".to_string())
                ),
            );

            let mut targets = Vec::new();
            let mut resolve_errors = Vec::new();
            let egress_ip_v4 = egress.ipv4_cn.or(egress.ipv4_global);
            let egress_ip_v6 = egress.ipv6_cn.or(egress.ipv6_global);
            let has_v6 = egress.ipv6.is_some();

            // Resolve v4 and v6 concurrently to save DoH + candidate probe time
            let parsed_v4 = parsed.clone();
            let parsed_v6 = parsed.clone();
            let host_v4 = host.clone();
            let host_v6 = host.clone();
            let proxy_v4 = proxy.map(str::to_string);
            let proxy_v6 = proxy.map(str::to_string);
            let country_v4 = resolver_country.clone();
            let country_v6 = resolver_country.clone();
            let (res_v4, res_v6) = tokio::join!(
                async move {
                    select_best_ip(
                        &parsed_v4, &host_v4, cli_timeout,
                        country_v4.as_deref(), IpFamily::V4,
                        proxy_v4.as_deref(), egress_ip_v4, cli_verbose,
                    ).await
                },
                async move {
                    if !has_v6 {
                        return Err(anyhow!("no v6 egress"));
                    }
                    select_best_ip(
                        &parsed_v6, &host_v6, cli_timeout,
                        country_v6.as_deref(), IpFamily::V6,
                        proxy_v6.as_deref(), egress_ip_v6, cli_verbose,
                    ).await
                },
            );
            match res_v4 {
                Ok((t, _)) => targets.push(t),
                Err(err) => resolve_errors.push(format!("v4 {}", short_error(&err))),
            }
            match res_v6 {
                Ok((t, _)) => targets.push(t),
                Err(err) if has_v6 => resolve_errors.push(format!("v6 {}", short_error(&err))),
                Err(_) => {}
            }
            if targets.is_empty() {
                emit_status(
                    is_json,
                    StatusKind::Error,
                    "resolve",
                    &format!("DoH failed, fallback to system DNS: {}", resolve_errors.join(" | ")),
                );
                if let Some(ip) = resolve_host_ip(&host).await {
                    targets.push(SelectedTarget {
                        family: if ip.is_ipv4() {
                            IpFamily::V4
                        } else {
                            IpFamily::V6
                        },
                        ip,
                        source: "system-dns".to_string(),
                    });
                }
            }

            // Dual-stack preflight (only for Apple backend).
            let mut dual_stack_preflight_inner = Vec::new();
            if targets.len() > 1 {
                emit_status(
                    is_json,
                    StatusKind::Info,
                    "dual",
                    "detected IPv6, running dual-stack preflight",
                );
                let mut joinset = tokio::task::JoinSet::new();
                for target in targets.clone() {
                    let base = parsed.clone();
                    let proxy_owned = proxy.map(str::to_string);
                    joinset.spawn(async move {
                        preflight_stack(&base, &target, proxy_owned.as_deref(), backend).await
                    });
                }
                while let Some(joined) = joinset.join_next().await {
                    dual_stack_preflight_inner.push(joined.context("dual preflight task failed")?);
                }
                emit_status(is_json, StatusKind::Ok, "dual", "dual-stack preflight done");
            }

            let selected = choose_primary_target(&targets, &dual_stack_preflight_inner)
                .ok_or_else(|| anyhow!("no resolved target ip"))?;
            emit_status(
                is_json,
                StatusKind::Ok,
                "resolve",
                &format!(
                    "{} {} -> {}",
                    selected.family.as_str(),
                    selected.source,
                    selected.ip
                ),
            );
            crate::tui::send(TuiEvent::ResolveDone {
                ip: selected.ip.to_string(),
                family: selected.family.as_str().to_string(),
                source: selected.source.clone(),
            });
            if cli_verbose {
                eprintln!(
                    "using host={}, family={}, ip={}, resolver={}, country={}",
                    host,
                    selected.family.as_str(),
                    selected.ip,
                    selected.source,
                    resolver_country.clone().unwrap_or_else(|| "-".to_string())
                );
            }

            let apple_client = build_client(&parsed, selected.ip, cli_timeout, proxy)?;
            let ip_str = selected.ip.to_string();
            let fam_str = selected.family.as_str().to_string();
            let src_str = selected.source.clone();

            // We store preflight in a temporary variable and return it alongside.
            // To keep the tuple uniform, we store it in the report later.
            // Return a sentinel tuple; dual_stack_preflight is handled below.
            return run_with_apple(
                cli_timeout,
                is_json,
                parsed,
                host,
                backend,
                proxy,
                egress,
                resolver_country,
                selected,
                apple_client,
                ip_str,
                fam_str,
                src_str,
                dual_stack_preflight_inner,
                command,
                mode,
                geo_v4_cn,
                geo_v4_global,
                geo_v6_cn,
                geo_v6_global,
            )
            .await;
        };

    // Cloudflare path: build report and run tests without DNS/preflight.
    let cf_base = Url::parse("https://speed.cloudflare.com").unwrap();

    // Fetch cdn-cgi/trace to extract the CDN node IP, colo code, and country.
    // This mirrors what Apple backend does via DNS + GeoIP, giving equivalent info.
    let (cf_node_ip, cf_node_location, cf_family, cf_resolver_country) =
        fetch_cloudflare_trace(&client, cli_timeout, proxy).await;

    // Send TUI events so Node Info and result page fill in like Apple backend.
    if let Some(ref ip) = cf_node_ip {
        crate::tui::send(TuiEvent::ResolveDone {
            ip: ip.clone(),
            family: cf_family.clone().unwrap_or_else(|| "v4".to_string()),
            source: "cdn-cgi/trace".to_string(),
        });
    }
    if let Some(ref loc) = cf_node_location {
        crate::tui::send(TuiEvent::GeoDone { location: loc.clone() });
    }

    let effective_resolver_country = resolver_country.or(cf_resolver_country);

    let mut report = Report {
        schema_version: 1,
        mode: mode.clone(),
        target_host: target_host.clone(),
        proxy: proxy.map(sanitize_proxy_display),
        resolver_country: effective_resolver_country,
        resolver_source: Some(selected_source),
        selected_family: cf_family,
        selected_ip: cf_node_ip,
        selected_location: cf_node_location,
        egress_ipv4: egress.ipv4.map(|v| v.to_string()),
        egress_ipv4_cn: egress.ipv4_cn.map(|v| v.to_string()),
        egress_ipv4_cn_geo: geo_v4_cn.clone(),
        egress_ipv4_global: egress.ipv4_global.map(|v| v.to_string()),
        egress_ipv4_global_geo: geo_v4_global.clone(),
        egress_ipv6: egress.ipv6.map(|v| v.to_string()),
        egress_ipv6_cn: egress.ipv6_cn.map(|v| v.to_string()),
        egress_ipv6_cn_geo: geo_v6_cn.clone(),
        egress_ipv6_global: egress.ipv6_global.map(|v| v.to_string()),
        egress_ipv6_global_geo: geo_v6_global.clone(),
        egress_consistent: Some(egress.consistent),
        egress_note: Some(egress.note),
        ..Default::default()
    };

    let code = execute_command(command, &client, &cf_base, backend, is_json, &mut report).await;

    Ok((report, code))
}

/// Execute the speed command stages, mutating the report.
/// Returns the exit code.
async fn execute_command(
    command: Command,
    client: &Client,
    base_url: &Url,
    backend: SpeedBackend,
    is_json: bool,
    report: &mut Report,
) -> u8 {
    match command {
        Command::Ping { count } => {
            crate::tui::send(TuiEvent::StageUpdate { stage: "ping", status: TuiStageStatus::Running });
            emit_status(is_json, StatusKind::Info, "ping", "starting ping");
            match measure_ping(client, base_url, count, backend).await {
                Ok((ping, ping_stats)) => {
                    emit_status(
                        is_json,
                        StatusKind::Ok,
                        "ping",
                        &format!("done {:.2} ms", ping),
                    );
                    report.rtt_ms = Some(ping);
                    report.ping_stats = Some(ping_stats);
                    crate::tui::send(TuiEvent::StageUpdate { stage: "ping", status: TuiStageStatus::Ok(format!("{ping:.2} ms")) });
                    0
                }
                Err(e) => {
                    let msg = short_error(&e);
                    emit_status(is_json, StatusKind::Error, "ping", &format!("failed: {msg}"));
                    report.errors.push(StageError { stage: "ping", message: format!("{e:#}") });
                    crate::tui::send(TuiEvent::StageUpdate { stage: "ping", status: TuiStageStatus::Fail(msg) });
                    2
                }
            }
        }
        Command::Download { duration } => {
            crate::tui::send(TuiEvent::StageUpdate { stage: "download", status: TuiStageStatus::Running });
            emit_status(is_json, StatusKind::Info, "download", "starting download");
            match run_download_tests(client, base_url, duration, backend, None).await {
                Ok(dl_run) => {
                    for s in &dl_run.stages {
                        if let Some(mbps) = s.mbps {
                            crate::tui::send(TuiEvent::DownloadStage {
                                name: s.name.clone(),
                                concurrency: s.concurrency,
                                chunk_mib: s.chunk_mib,
                                secs: s.duration_secs,
                                mbps,
                            });
                        }
                    }
                    report.cdn_meta = dl_run.cdn_meta;
                    report.download_stages = dl_run.stages;
                    report.download_stats = dl_run.download_stats;
                    match dl_run.best_mbps {
                        Some(dl) => {
                            report.download_mbps = Some(dl);
                            emit_status(is_json, StatusKind::Ok, "download", &format!("done {:.2} Mbps", dl));
                            crate::tui::send(TuiEvent::StageUpdate { stage: "download", status: TuiStageStatus::Ok(format!("{dl:.2} Mbps")) });
                            0
                        }
                        None => {
                            emit_status(is_json, StatusKind::Error, "download", "failed: all download stages failed");
                            report.errors.push(StageError { stage: "download", message: "all download stages failed".to_string() });
                            crate::tui::send(TuiEvent::StageUpdate { stage: "download", status: TuiStageStatus::Fail("all stages failed".to_string()) });
                            2
                        }
                    }
                }
                Err(e) => {
                    let msg = short_error(&e);
                    emit_status(is_json, StatusKind::Error, "download", &format!("failed: {msg}"));
                    report.errors.push(StageError { stage: "download", message: format!("{e:#}") });
                    crate::tui::send(TuiEvent::StageUpdate { stage: "download", status: TuiStageStatus::Fail(msg) });
                    2
                }
            }
        }
        Command::Upload { ul_mib, ul_repeat } => {
            // Cloudflare __up incurs ~5-6s server-side processing overhead per request,
            // so cap payload to 4 MiB to stay within practical timeout budgets.
            let ul_mib = if backend == SpeedBackend::Cloudflare { ul_mib.min(4) } else { ul_mib };
            crate::tui::send(TuiEvent::StageUpdate { stage: "upload", status: TuiStageStatus::Running });
            emit_status(is_json, StatusKind::Info, "upload", "starting upload");
            match measure_upload(client, base_url, ul_mib, ul_repeat, backend).await {
                Ok((ul, ul_stats)) => {
                    report.upload_mbps = Some(ul);
                    report.upload_stats = Some(ul_stats);
                    emit_status(is_json, StatusKind::Ok, "upload", &format!("done {:.2} Mbps", ul));
                    crate::tui::send(TuiEvent::StageUpdate { stage: "upload", status: TuiStageStatus::Ok(format!("{ul:.2} Mbps")) });
                    0
                }
                Err(e) => {
                    let msg = short_error(&e);
                    emit_status(is_json, StatusKind::Error, "upload", &format!("failed: {msg}"));
                    report.errors.push(StageError { stage: "upload", message: format!("{e:#}") });
                    crate::tui::send(TuiEvent::StageUpdate { stage: "upload", status: TuiStageStatus::Fail(msg) });
                    2
                }
            }
        }
        Command::Full {
            count,
            duration,
            ul_mib,
            ul_repeat,
        } => {
            let mut success = 0_u8;

            crate::tui::send(TuiEvent::StageUpdate { stage: "ping", status: TuiStageStatus::Running });
            emit_status(is_json, StatusKind::Info, "ping", "starting ping");
            match measure_ping(client, base_url, count, backend).await {
                Ok((v, ping_stats)) => {
                    report.rtt_ms = Some(v);
                    report.ping_stats = Some(ping_stats);
                    emit_status(is_json, StatusKind::Ok, "ping", &format!("done {:.2} ms", v));
                    crate::tui::send(TuiEvent::StageUpdate { stage: "ping", status: TuiStageStatus::Ok(format!("{v:.2} ms")) });
                    success += 1;
                }
                Err(e) => {
                    let msg = short_error(&e);
                    emit_status(is_json, StatusKind::Error, "ping", &format!("failed: {msg}"));
                    report.errors.push(StageError { stage: "ping", message: format!("{e:#}") });
                    crate::tui::send(TuiEvent::StageUpdate { stage: "ping", status: TuiStageStatus::Fail(msg) });
                }
            }

            crate::tui::send(TuiEvent::StageUpdate { stage: "download", status: TuiStageStatus::Running });
            emit_status(is_json, StatusKind::Info, "download", "starting download");
            match run_download_tests(client, base_url, duration, backend, None).await {
                Ok(dl_run) if dl_run.best_mbps.is_some() => {
                    let v = dl_run.best_mbps.unwrap_or(0.0);
                    for s in &dl_run.stages {
                        if let Some(mbps) = s.mbps {
                            crate::tui::send(TuiEvent::DownloadStage {
                                name: s.name.clone(),
                                concurrency: s.concurrency,
                                chunk_mib: s.chunk_mib,
                                secs: s.duration_secs,
                                mbps,
                            });
                        }
                    }
                    report.cdn_meta = dl_run.cdn_meta;
                    report.download_stages = dl_run.stages;
                    report.download_stats = dl_run.download_stats;
                    report.download_mbps = Some(v);
                    emit_status(is_json, StatusKind::Ok, "download", &format!("done {:.2} Mbps", v));
                    crate::tui::send(TuiEvent::StageUpdate { stage: "download", status: TuiStageStatus::Ok(format!("{v:.2} Mbps")) });
                    success += 1;
                }
                Ok(dl_run) => {
                    report.cdn_meta = dl_run.cdn_meta;
                    report.download_stages = dl_run.stages;
                    report.download_stats = dl_run.download_stats;
                    let message = "all download stages failed".to_string();
                    emit_status(is_json, StatusKind::Error, "download", &format!("failed: {message}"));
                    report.errors.push(StageError { stage: "download", message: message.clone() });
                    crate::tui::send(TuiEvent::StageUpdate { stage: "download", status: TuiStageStatus::Fail(message) });
                }
                Err(e) => {
                    let msg = short_error(&e);
                    emit_status(is_json, StatusKind::Error, "download", &format!("failed: {msg}"));
                    report.errors.push(StageError { stage: "download", message: format!("{e:#}") });
                    crate::tui::send(TuiEvent::StageUpdate { stage: "download", status: TuiStageStatus::Fail(msg) });
                }
            }

            crate::tui::send(TuiEvent::StageUpdate { stage: "upload", status: TuiStageStatus::Running });
            emit_status(is_json, StatusKind::Info, "upload", "starting upload");
            // Cloudflare __up incurs ~5-6s server-side processing per request; cap to 4 MiB.
            let ul_mib_eff = if backend == SpeedBackend::Cloudflare { ul_mib.min(4) } else { ul_mib };
            match measure_upload(client, base_url, ul_mib_eff, ul_repeat, backend).await {
                Ok((v, ul_stats)) => {
                    report.upload_mbps = Some(v);
                    report.upload_stats = Some(ul_stats);
                    emit_status(is_json, StatusKind::Ok, "upload", &format!("done {:.2} Mbps", v));
                    crate::tui::send(TuiEvent::StageUpdate { stage: "upload", status: TuiStageStatus::Ok(format!("{v:.2} Mbps")) });
                    success += 1;
                }
                Err(e) => {
                    let msg = short_error(&e);
                    emit_status(is_json, StatusKind::Error, "upload", &format!("failed: {msg}"));
                    report.errors.push(StageError { stage: "upload", message: format!("{e:#}") });
                    crate::tui::send(TuiEvent::StageUpdate { stage: "upload", status: TuiStageStatus::Fail(msg) });
                }
            }

            if success == 3 {
                0
            } else if success == 0 {
                2
            } else {
                3
            }
        }
        Command::Probe { .. } => {
            // Probe command is handled by main.rs before entering this function; this branch is unreachable
            2
        }
    }
}

/// Apple-backend full run (with DNS resolution, IP selection, preflight).
#[allow(clippy::too_many_arguments)]
async fn run_with_apple(
    cli_timeout: u64,
    is_json: bool,
    parsed: Url,
    host: String,
    backend: SpeedBackend,
    proxy: Option<&str>,
    egress: crate::network::egress::EgressProfile,
    resolver_country: Option<String>,
    selected: SelectedTarget,
    client: Client,
    ip_str: String,
    fam_str: String,
    src_str: String,
    dual_stack_preflight: Vec<StackPreflightMetric>,
    command: Command,
    mode: String,
    egress_ipv4_cn_geo: Option<String>,
    egress_ipv4_global_geo: Option<String>,
    egress_ipv6_cn_geo: Option<String>,
    egress_ipv6_global_geo: Option<String>,
) -> Result<(Report, u8)> {
    let mut report = Report {
        schema_version: 1,
        mode: mode.clone(),
        target_host: host.clone(),
        proxy: proxy.map(sanitize_proxy_display),
        resolver_country: resolver_country.clone(),
        resolver_source: Some(src_str),
        selected_family: Some(fam_str),
        selected_ip: Some(ip_str),
        selected_location: None,
        egress_ipv4: egress.ipv4.map(|v| v.to_string()),
        egress_ipv4_cn: egress.ipv4_cn.map(|v| v.to_string()),
        egress_ipv4_cn_geo,
        egress_ipv4_global: egress.ipv4_global.map(|v| v.to_string()),
        egress_ipv4_global_geo,
        egress_ipv6: egress.ipv6.map(|v| v.to_string()),
        egress_ipv6_cn: egress.ipv6_cn.map(|v| v.to_string()),
        egress_ipv6_cn_geo,
        egress_ipv6_global: egress.ipv6_global.map(|v| v.to_string()),
        egress_ipv6_global_geo,
        egress_consistent: Some(egress.consistent),
        egress_note: Some(egress.note),
        timestamp_unix: now_unix(),
        dual_stack_preflight,
        ..Default::default()
    };

    emit_status(
        is_json,
        StatusKind::Info,
        "geo",
        &format!("querying node location {}", selected.ip),
    );
    match lookup_ip_location(selected.ip, cli_timeout, proxy).await {
        Ok(Some(loc)) => {
            report.selected_location = Some(loc.clone());
            emit_status(is_json, StatusKind::Ok, "geo", &loc);
            crate::tui::send(TuiEvent::GeoDone { location: loc });
        }
        Ok(None) => {
            emit_status(is_json, StatusKind::Info, "geo", "no location data");
            crate::tui::send(TuiEvent::GeoDone { location: "-".to_string() });
        }
        Err(err) => {
            emit_status(
                is_json,
                StatusKind::Error,
                "geo",
                &format!("geo lookup failed: {}", short_error(&err)),
            );
        }
    }

    // ── Multi-path speed test: (v4,v6) × (cn,global), deduplicated ──────────
    // Build path descriptor list: (path_id, family, side, egress_ip)
    // Only mainland China (CN) gets separate cn/global path testing; other regions test only the global path
    let is_cn_mode = resolver_country.as_deref() == Some("CN");
    crate::tui::send(TuiEvent::CnMode(is_cn_mode));

    struct PathSpec {
        path_id: String,
        family: IpFamily,
        side: String,
        egress_ip: Option<std::net::IpAddr>,
    }

    let mut path_specs: Vec<PathSpec> = Vec::new();

    // v4 paths
    if let Some(v4_cn) = egress.ipv4_cn {
        if is_cn_mode {
            let v4_gl = egress.ipv4_global;
            path_specs.push(PathSpec {
                path_id: "v4-cn".to_string(),
                family: IpFamily::V4,
                side: "cn".to_string(),
                egress_ip: Some(v4_cn),
            });
            // If the global IP differs from the CN IP, or if global is None (proxy present but no global egress detected),
            // add a global path so that ECS=None on the proxy side lets the CDN return the globally nearest node
            let add_global_v4 = v4_gl.map(|ip| ip != v4_cn).unwrap_or(true);
            if add_global_v4 {
                path_specs.push(PathSpec {
                    path_id: "v4-global".to_string(),
                    family: IpFamily::V4,
                    side: "global".to_string(),
                    egress_ip: v4_gl,
                });
            }
        } else {
            // Non-CN: test global path only
            path_specs.push(PathSpec {
                path_id: "v4-global".to_string(),
                family: IpFamily::V4,
                side: "global".to_string(),
                egress_ip: egress.ipv4_global.or(Some(v4_cn)),
            });
        }
    } else if egress.ipv4_global.is_some() {
        path_specs.push(PathSpec {
            path_id: "v4-global".to_string(),
            family: IpFamily::V4,
            side: "global".to_string(),
            egress_ip: egress.ipv4_global,
        });
    }

    // v6 paths
    if let Some(v6_cn) = egress.ipv6_cn {
        if is_cn_mode {
            let v6_gl = egress.ipv6_global;
            path_specs.push(PathSpec {
                path_id: "v6-cn".to_string(),
                family: IpFamily::V6,
                side: "cn".to_string(),
                egress_ip: Some(v6_cn),
            });
            let add_global_v6 = v6_gl.map(|ip| ip != v6_cn).unwrap_or(true);
            if add_global_v6 {
                path_specs.push(PathSpec {
                    path_id: "v6-global".to_string(),
                    family: IpFamily::V6,
                    side: "global".to_string(),
                    egress_ip: v6_gl,
                });
            }
        } else {
            path_specs.push(PathSpec {
                path_id: "v6-global".to_string(),
                family: IpFamily::V6,
                side: "global".to_string(),
                egress_ip: egress.ipv6_global.or(Some(v6_cn)),
            });
        }
    } else if egress.ipv6_global.is_some() {
        path_specs.push(PathSpec {
            path_id: "v6-global".to_string(),
            family: IpFamily::V6,
            side: "global".to_string(),
            egress_ip: egress.ipv6_global,
        });
    }

    if path_specs.is_empty() {
        // Fallback: run a single test using the already-selected target
        let code = execute_command(command, &client, &parsed, backend, is_json, &mut report).await;
        return Ok((report, code));
    }

    // Initialise the TUI path list
    let init_rows: Vec<PathRow> = path_specs.iter().map(|p| PathRow {
        path_id: p.path_id.clone(),
        family: p.family.as_str().to_string(),
        side: p.side.clone(),
        current_stage: "waiting".to_string(),
        cdn_ip: None,
        cdn_location: None,
        rtt_ms: None,
        tcp_rtt_ms: None,
        dl_mbps: None,
        ul_mbps: None,
        error: None,
        done: false,
    }).collect();
    crate::tui::send(TuiEvent::PathsInit { paths: init_rows });

    // Extract command parameters (cloned and passed to each path)
    let (ping_count, dl_duration, ul_mib, ul_repeat) = match &command {
        Command::Full { count, duration, ul_mib, ul_repeat } => (*count, *duration, *ul_mib, *ul_repeat),
        Command::Ping { count } => (*count, 0, 0, 0),
        Command::Download { duration } => (0, *duration, 0, 0),
        Command::Upload { ul_mib, ul_repeat } => (0, 0, *ul_mib, *ul_repeat),
        Command::Probe { .. } => (0, 0, 0, 0),
    };

    // ── Phase 1: parallel DNS resolution + candidate IP probing + GeoIP lookup ──

    /// Resolution result for a single path
    struct ResolvedPath {
        path_id: String,
        family: crate::network::IpFamily,
        side: String,
        egress_ip: Option<std::net::IpAddr>,
        /// None means resolution failed
        cdn_target: Option<crate::network::dns::SelectedTarget>,
        cdn_location: Option<String>,
        candidates_report: Vec<crate::report::CandidateProbeResult>,
        /// Only set when resolution fails
        error: Option<String>,
        /// Whether the node is reachable (selected candidate download_ok)
        reachable: bool,
    }

    // Send all path_specs to a JoinSet for parallel resolution
    let mut resolve_set: tokio::task::JoinSet<ResolvedPath> = tokio::task::JoinSet::new();

    for spec in path_specs {
        let parsed2    = parsed.clone();
        let host2      = host.clone();
        let proxy_str  = proxy.map(str::to_string);
        let path_id    = spec.path_id.clone();
        let family     = spec.family;
        let side       = spec.side.clone();
        let egress_ip  = spec.egress_ip;

        // Update TUI: resolution started
        crate::tui::send(TuiEvent::PathUpdate {
            path_id: path_id.clone(),
            current_stage: "resolving".to_string(),
            cdn_ip: None, cdn_location: None,
            rtt_ms: None, tcp_rtt_ms: None, dl_mbps: None, ul_mbps: None, error: None, done: false,
        });
        emit_status(is_json, StatusKind::Info, &path_id,
            &format!("[{path_id}] resolving via DoH (egress={egress_ip:?})"));

        resolve_set.spawn(async move {
            let country_for_doh: Option<&str> = if side == "cn" { Some("CN") } else { None };
            // CN path: both the DoH query and candidate probing bypass the proxy.
            // A detected CN egress IP means a direct mainland connection exists; the proxy only affects global paths.
            // If CN DoH also went through the proxy, AliDNS/DNSPod would see the proxy IP and return HK nodes.
            let doh_proxy: Option<&str> = if side == "cn" { None } else { proxy_str.as_deref() };
            let resolve_result = select_best_ip(
                &parsed2, &host2, cli_timeout,
                country_for_doh, family, doh_proxy, egress_ip, false,
            ).await;

            match resolve_result {
                Err(e) => ResolvedPath {
                    path_id, family, side, egress_ip,
                    cdn_target: None, cdn_location: None,
                    candidates_report: vec![],
                    error: Some(short_error(&e)),
                    reachable: false,
                },
                Ok((cdn_target, candidate_results)) => {
                    let reachable = candidate_results.iter()
                        .find(|c| c.selected)
                        .map(|c| c.download_ok)
                        .unwrap_or(false);

                    // Concurrent GeoIP lookup (reachable nodes only)
                    let cdn_location = if reachable {
                        lookup_ip_location(cdn_target.ip, cli_timeout, proxy_str.as_deref())
                            .await.ok().flatten()
                    } else {
                        None
                    };

                    let candidates_report = candidate_results.iter()
                        .map(|c| crate::report::CandidateProbeResult {
                            ip: c.ip.to_string(),
                            sources: c.sources.clone(),
                            rtt_ms: c.rtt_ms,
                            download_ok: c.download_ok,
                            selected: c.selected,
                            location: c.location.clone(),
                        })
                        .collect();

                    ResolvedPath {
                        path_id, family, side, egress_ip,
                        cdn_target: Some(cdn_target),
                        cdn_location,
                        candidates_report,
                        error: None,
                        reachable,
                    }
                }
            }
        });
    }

    // Collect results in the original path order (JoinSet completes out of order; sort by path_id)
    let mut resolved_paths: Vec<ResolvedPath> = Vec::new();
    while let Some(Ok(rp)) = resolve_set.join_next().await {
        resolved_paths.push(rp);
    }
    // Maintain v4-cn / v4-global / v6-cn / v6-global order
    let order = ["v4-cn", "v4-global", "v6-cn", "v6-global"];
    resolved_paths.sort_by_key(|rp| {
        order.iter().position(|&s| s == rp.path_id).unwrap_or(99)
    });

    // Update TUI: all paths resolved; refresh node / error state
    let mut primary_node_updated = false;
    for rp in &resolved_paths {
        if let Some(ref target) = rp.cdn_target {
            let cdn_ip_str = target.ip.to_string();

            // Log: candidate IP details
            for c in &rp.candidates_report {
                let sel = if c.selected { " ★" } else { "" };
                emit_status(is_json, StatusKind::Info, &rp.path_id, &format!(
                    "[{}] candidate {} rtt={} dl_ok={}{sel}",
                    rp.path_id, c.ip,
                    c.rtt_ms.map(|v| format!("{v:.1}ms")).unwrap_or_else(|| "-".to_string()),
                    c.download_ok,
                ));
            }

            if !rp.reachable {
                emit_status(is_json, StatusKind::Error, &rp.path_id,
                    &format!("[{}] CDN node unreachable, skipped", rp.path_id));
                crate::tui::send(TuiEvent::PathUpdate {
                    path_id: rp.path_id.clone(),
                    current_stage: "skipped".to_string(),
                    cdn_ip: Some(cdn_ip_str),
                    cdn_location: rp.cdn_location.clone(),
                    rtt_ms: None, tcp_rtt_ms: None, dl_mbps: None, ul_mbps: None,
                    error: Some("CDN node unreachable".to_string()), done: true,
                });
            } else {
                emit_status(is_json, StatusKind::Ok, &rp.path_id, &format!(
                    "[{}] CDN IP = {} (best of {} candidates)",
                    rp.path_id, cdn_ip_str, rp.candidates_report.len()
                ));
                crate::tui::send(TuiEvent::PathUpdate {
                    path_id: rp.path_id.clone(),
                    current_stage: "pending".to_string(),
                    cdn_ip: Some(cdn_ip_str.clone()),
                    cdn_location: rp.cdn_location.clone(),
                    rtt_ms: None, tcp_rtt_ms: None, dl_mbps: None, ul_mbps: None, error: None, done: false,
                });

                // First reachable path updates the primary node display
                if !primary_node_updated {
                    primary_node_updated = true;
                    crate::tui::send(TuiEvent::ResolveDone {
                        ip: cdn_ip_str.clone(),
                        family: rp.family.as_str().to_string(),
                        source: format!("{} ({})", target.source, rp.path_id),
                    });
                    report.selected_ip = Some(cdn_ip_str);
                    report.selected_family = Some(rp.family.as_str().to_string());
                    if let Some(ref loc) = rp.cdn_location {
                        report.selected_location = Some(loc.clone());
                        crate::tui::send(TuiEvent::GeoDone { location: loc.clone() });
                    }
                }
            }
        } else {
            // Resolution failed
            let msg = rp.error.as_deref().unwrap_or("DoH failed");
            emit_status(is_json, StatusKind::Error, &rp.path_id, &format!("DoH failed: {msg}"));
            crate::tui::send(TuiEvent::PathUpdate {
                path_id: rp.path_id.clone(),
                current_stage: "failed".to_string(),
                cdn_ip: None, cdn_location: None,
                rtt_ms: None, tcp_rtt_ms: None, dl_mbps: None, ul_mbps: None,
                error: Some(msg.to_string()), done: true,
            });
        }
    }

    // ── Phase 2: sequential speed testing (avoid saturating bandwidth simultaneously which would skew results) ──

    let mut path_results: Vec<PathResult> = Vec::new();
    let mut any_success = false;

    for rp in resolved_paths {
        let path_id = &rp.path_id;

        // Paths that failed resolution are written to results directly
        let cdn_target = match rp.cdn_target {
            None => {
                path_results.push(PathResult {
                    path_id: path_id.clone(),
                    family: rp.family.as_str().to_string(),
                    side: rp.side.clone(),
                    egress_ip: rp.egress_ip.map(|ip| ip.to_string()),
                    resolver_source: None,
                    cdn_ip: None, cdn_location: None,
                    rtt_ms: None, ping_stats: None,
                    download_mbps: None, download_stats: None,
                    upload_mbps: None, download_stages: vec![],
                    candidates: rp.candidates_report,
                    error: rp.error,
                });
                continue;
            }
            Some(t) => t,
        };

        // Paths whose node is unreachable
        if !rp.reachable {
            path_results.push(PathResult {
                path_id: path_id.clone(),
                family: rp.family.as_str().to_string(),
                side: rp.side.clone(),
                egress_ip: rp.egress_ip.map(|ip| ip.to_string()),
                resolver_source: Some(cdn_target.source.clone()),
                cdn_ip: Some(cdn_target.ip.to_string()),
                cdn_location: rp.cdn_location,
                rtt_ms: None, ping_stats: None,
                download_mbps: None, download_stats: None,
                upload_mbps: None, download_stages: vec![],
                candidates: rp.candidates_report,
                error: Some("CDN node unreachable, skipped".to_string()),
            });
            continue;
        }

        let cdn_ip_str = cdn_target.ip.to_string();
        let path_cdn_location = rp.cdn_location;

        // CN paths have a direct mainland egress; bypass the proxy so build_client uses .resolve() to pin the CDN IP.
        // Global paths still go through the proxy (if any).
        let path_proxy = if rp.side == "cn" { None } else { proxy };

        // Build a client with the CDN IP pinned
        let path_client = match build_client(&parsed, cdn_target.ip, cli_timeout, path_proxy) {
            Ok(c) => c,
            Err(e) => {
                let msg = short_error(&e);
                crate::tui::send(TuiEvent::PathUpdate {
                    path_id: path_id.clone(),
                    current_stage: "failed".to_string(),
                    cdn_ip: Some(cdn_ip_str.clone()),
                    cdn_location: path_cdn_location.clone(),
                    rtt_ms: None, tcp_rtt_ms: None, dl_mbps: None, ul_mbps: None,
                    error: Some(msg.clone()), done: true,
                });
                path_results.push(PathResult {
                    path_id: path_id.clone(),
                    family: rp.family.as_str().to_string(),
                    side: rp.side.clone(),
                    egress_ip: rp.egress_ip.map(|ip| ip.to_string()),
                    resolver_source: Some(cdn_target.source.clone()),
                    cdn_ip: Some(cdn_ip_str),
                    cdn_location: None,
                    rtt_ms: None, ping_stats: None,
                    download_mbps: None, download_stats: None,
                    upload_mbps: None, download_stages: vec![],
                    candidates: rp.candidates_report,
                    error: Some(msg),
                });
                continue;
            }
        };

        // Always use the domain-name URL regardless of proxy:
        // - Without proxy: .resolve() in build_client has already pinned the IP; TLS SNI works correctly.
        // - With proxy: the proxy handles DNS resolution and routing; we do not force an IP, which matches user expectations.
        let pinned_url = parsed.clone();

        let mut path_rtt: Option<f64> = None;
        let mut path_tcp_rtt: Option<f64> = None;
        let mut path_ping_stats: Option<crate::report::PingStats> = None;
        let mut path_dl: Option<f64> = None;
        let mut path_dl_stats: Option<crate::report::SpeedStats> = None;
        let mut path_ul: Option<f64> = None;
        let mut path_dl_stages: Vec<crate::report::DownloadStageMetric> = vec![];
        let mut path_error: Option<String> = None;

        // Ping (HTTP RTT + TCP RTT concurrently)
        if matches!(&command, Command::Ping { .. } | Command::Full { .. }) {
            crate::tui::send(TuiEvent::PathUpdate {
                path_id: path_id.clone(),
                current_stage: "ping".to_string(),
                cdn_ip: Some(cdn_ip_str.clone()),
                cdn_location: path_cdn_location.clone(),
                rtt_ms: None, tcp_rtt_ms: None, dl_mbps: None, ul_mbps: None, error: None, done: false,
            });

            // TCP ping: connect directly when no proxy; skip when a proxy is present (TCP SYN cannot be forwarded transparently through a proxy)
            if proxy.is_none() {
                let host_str = parsed.host_str().unwrap_or("").to_string();
                let port = parsed.port_or_known_default().unwrap_or(443);
                if let Ok(tcp_ms) = tcp_ping(&host_str, port, ping_count.min(4)).await {
                    path_tcp_rtt = Some(tcp_ms);
                    emit_status(is_json, StatusKind::Ok, path_id,
                        &format!("[{path_id}] tcp-ping {tcp_ms:.2}ms"));
                }
            }

            match measure_ping(&path_client, &pinned_url, ping_count, backend).await {
                Ok((ms, mut ps)) => {
                    ps.tcp_rtt_ms = path_tcp_rtt;
                    path_rtt = Some(ms);
                    path_ping_stats = Some(ps);
                    emit_status(is_json, StatusKind::Ok, path_id, &format!("[{path_id}] http-ping {ms:.2}ms"));
                    crate::tui::send(TuiEvent::PathUpdate {
                        path_id: path_id.clone(),
                        current_stage: "ping".to_string(),
                        cdn_ip: Some(cdn_ip_str.clone()),
                        cdn_location: path_cdn_location.clone(),
                        rtt_ms: Some(ms), tcp_rtt_ms: path_tcp_rtt, dl_mbps: None, ul_mbps: None, error: None, done: false,
                    });
                }
                Err(e) => {
                    let msg = short_error(&e);
                    emit_status(is_json, StatusKind::Error, path_id, &format!("[{path_id}] ping failed: {msg}"));
                    if path_error.is_none() { path_error = Some(format!("ping: {msg}")); }
                }
            }
        }

        // Download
        if matches!(&command, Command::Download { .. } | Command::Full { .. }) {
            crate::tui::send(TuiEvent::PathUpdate {
                path_id: path_id.clone(),
                current_stage: "download".to_string(),
                cdn_ip: Some(cdn_ip_str.clone()),
                cdn_location: path_cdn_location.clone(),
                rtt_ms: path_rtt, tcp_rtt_ms: path_tcp_rtt, dl_mbps: None, ul_mbps: None, error: None, done: false,
            });
            match run_download_tests(&path_client, &pinned_url, dl_duration, backend, Some(Box::new({
                let path_id2 = path_id.clone();
                let cdn_ip2 = cdn_ip_str.clone();
                let cdn_loc2 = path_cdn_location.clone();
                let rtt = path_rtt;
                move |v| {
                    crate::tui::send(TuiEvent::PathUpdate {
                        path_id: path_id2.clone(),
                        current_stage: "download".to_string(),
                        cdn_ip: Some(cdn_ip2.clone()),
                        cdn_location: cdn_loc2.clone(),
                        rtt_ms: rtt, tcp_rtt_ms: None, dl_mbps: Some(v), ul_mbps: None, error: None, done: false,
                    });
                }
            }))).await {
                Ok(dl_run) => {
                    path_dl_stages = dl_run.stages;
                    path_dl_stats = dl_run.download_stats;
                    if let Some(v) = dl_run.best_mbps {
                        path_dl = Some(v);
                        emit_status(is_json, StatusKind::Ok, path_id, &format!("[{path_id}] dl {v:.2}Mbps"));
                        any_success = true;
                    } else if path_error.is_none() {
                        path_error = Some("download: all stages failed".to_string());
                    }
                    crate::tui::send(TuiEvent::PathUpdate {
                        path_id: path_id.clone(),
                        current_stage: "download".to_string(),
                        cdn_ip: Some(cdn_ip_str.clone()),
                        cdn_location: path_cdn_location.clone(),
                        rtt_ms: path_rtt, tcp_rtt_ms: path_tcp_rtt, dl_mbps: path_dl, ul_mbps: None, error: None, done: false,
                    });
                }
                Err(e) => {
                    let msg = short_error(&e);
                    emit_status(is_json, StatusKind::Error, path_id, &format!("[{path_id}] download failed: {msg}"));
                    if path_error.is_none() { path_error = Some(format!("download: {msg}")); }
                }
            }
        }

        // Upload
        if matches!(&command, Command::Upload { .. } | Command::Full { .. }) {
            crate::tui::send(TuiEvent::PathUpdate {
                path_id: path_id.clone(),
                current_stage: "upload".to_string(),
                cdn_ip: Some(cdn_ip_str.clone()),
                cdn_location: path_cdn_location.clone(),
                rtt_ms: path_rtt, tcp_rtt_ms: path_tcp_rtt, dl_mbps: path_dl, ul_mbps: None, error: None, done: false,
            });
            let ul_timeout = cli_timeout.max(60);
            let ul_client = build_client(&parsed, cdn_target.ip, ul_timeout, path_proxy)
                .unwrap_or(path_client.clone());
            match measure_upload(&ul_client, &pinned_url, ul_mib, ul_repeat, backend).await {
                Ok((v, _)) => {
                    path_ul = Some(v);
                    emit_status(is_json, StatusKind::Ok, path_id, &format!("[{path_id}] ul {v:.2}Mbps"));
                    any_success = true;
                }
                Err(e) => {
                    let msg = short_error(&e);
                    emit_status(is_json, StatusKind::Error, path_id, &format!("[{path_id}] upload failed: {msg}"));
                }
            }
        }

        // Path completed
        crate::tui::send(TuiEvent::PathUpdate {
            path_id: path_id.clone(),
            current_stage: "done".to_string(),
            cdn_ip: Some(cdn_ip_str.clone()),
            cdn_location: path_cdn_location.clone(),
            rtt_ms: path_rtt, tcp_rtt_ms: path_tcp_rtt, dl_mbps: path_dl, ul_mbps: path_ul,
            error: path_error.clone(), done: true,
        });

        if path_rtt.is_some() || path_dl.is_some() || path_ul.is_some() {
            any_success = true;
        }

        path_results.push(PathResult {
            path_id: path_id.clone(),
            family: rp.family.as_str().to_string(),
            side: rp.side.clone(),
            egress_ip: rp.egress_ip.map(|ip| ip.to_string()),
            resolver_source: Some(cdn_target.source.clone()),
            cdn_ip: Some(cdn_ip_str),
            cdn_location: path_cdn_location,
            rtt_ms: path_rtt,
            ping_stats: path_ping_stats,
            download_mbps: path_dl,
            download_stats: path_dl_stats,
            upload_mbps: path_ul,
            download_stages: path_dl_stages,
            candidates: rp.candidates_report,
            error: path_error,
        });
    }

    // Write multi-path results into the report
    report.paths = path_results;

    // Extract the best values from multi-path results and fill top-level fields (for backward compatibility)
    let best_path = report.paths.iter()
        .filter(|p| p.error.is_none())
        .max_by(|a, b| {
            let da = a.download_mbps.unwrap_or(0.0);
            let db = b.download_mbps.unwrap_or(0.0);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
    if let Some(bp) = best_path {
        if report.rtt_ms.is_none() { report.rtt_ms = bp.rtt_ms; }
        if report.download_mbps.is_none() { report.download_mbps = bp.download_mbps; }
        if report.upload_mbps.is_none() { report.upload_mbps = bp.upload_mbps; }
    }

    let code = if any_success { 0 } else { 2 };

    // Full mode: automatically run routing probe after speed test completes
    if matches!(command, Command::Full { .. }) {
        emit_status(is_json, StatusKind::Info, "probe", "running connectivity probe...");
        let probe_targets = crate::probe::target::all_targets();
        let probe_results = crate::probe::run_probe(probe_targets, 8, 10, proxy, false).await;
        let reachable = probe_results.iter().filter(|r| r.reachable).count();
        emit_status(is_json, StatusKind::Ok, "probe",
            &format!("done {} sites, {reachable} reachable", probe_results.len()));
        report.probe_results = probe_results.clone();
        crate::tui::send(crate::tui::state::Event::ProbeDone { results: probe_results });
    }

    Ok((report, code))
}
