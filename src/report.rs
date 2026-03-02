use serde::Serialize;

use crate::output::{Colors, box_top, box_bot, box_row, box_sep_row, short_message};

/// Speed distribution statistics (in Mbps)
#[derive(Debug, Serialize, Clone, Default)]
pub struct SpeedStats {
    pub min_mbps: f64,
    pub avg_mbps: f64,
    pub max_mbps: f64,
    pub stddev_mbps: f64,
    pub p25_mbps: f64,
    pub p75_mbps: f64,
    pub p90_mbps: f64,
    /// MB/s (MegaBytes per second) = mbps / 8
    pub min_mbs: f64,
    pub avg_mbs: f64,
    pub max_mbs: f64,
}

/// Ping latency distribution statistics (in ms)
#[derive(Debug, Serialize, Clone, Default)]
pub struct PingStats {
    pub samples: Vec<f64>,
    pub min_ms: f64,
    pub avg_ms: f64,
    pub median_ms: f64,
    pub max_ms: f64,
    /// Average of absolute differences between adjacent samples (jitter)
    pub jitter_ms: f64,
    pub p95_ms: f64,
    /// TCP connect latency (present if no proxy; None if proxied)
    pub tcp_rtt_ms: Option<f64>,
}

/// Probe results for candidate CDN IPs (probed one by one after DNS resolution)
#[derive(Debug, Serialize, Clone)]
pub struct CandidateProbeResult {
    pub ip: String,
    /// List of DNS providers that returned this IP
    pub sources: Vec<String>,
    pub rtt_ms: Option<f64>,
    pub download_ok: bool,
    /// Whether this IP is selected as the final speed test node
    pub selected: bool,
    /// Geographic location of this IP (country/region/city ISP)
    pub location: Option<String>,
}

/// Results of a single speed test path (IP version × CN/Global side)
#[derive(Debug, Serialize, Clone)]
pub struct PathResult {
    /// e.g. "v4-cn", "v4-global", "v6-cn", "v6-global"
    pub path_id: String,
    pub family: String,
    /// "cn" or "global"
    pub side: String,
    /// Egress IP used for this path (for ECS construction)
    pub egress_ip: Option<String>,
    /// DNS resolver source(s) that provided the CDN IP for this path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolver_source: Option<String>,
    /// CDN node IP resolved by DNS
    pub cdn_ip: Option<String>,
    /// CDN node geographic location
    pub cdn_location: Option<String>,
    pub rtt_ms: Option<f64>,
    /// Detailed ping statistics for this path (including sparkline samples)
    pub ping_stats: Option<PingStats>,
    pub download_mbps: Option<f64>,
    /// Detailed download statistics for this path
    pub download_stats: Option<SpeedStats>,
    pub upload_mbps: Option<f64>,
    pub download_stages: Vec<DownloadStageMetric>,
    /// Probe results for all candidate IPs of this path
    pub candidates: Vec<CandidateProbeResult>,
    pub error: Option<String>,
}

/// CDN response header metadata collected from the first download request.
#[derive(Debug, Serialize, Clone, Default)]
pub struct CdnMeta {
    pub via: Option<String>,
    pub x_cache: Option<String>,
    pub age: Option<String>,
    pub server: Option<String>,
    pub http_version: String,
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct Report {
    /// JSON schema version — only increments, fields are never removed or type-changed.
    pub schema_version: u32,
    pub mode: String,
    pub target_host: String,
    pub proxy: Option<String>,
    pub resolver_country: Option<String>,
    pub resolver_source: Option<String>,
    pub selected_family: Option<String>,
    pub selected_ip: Option<String>,
    pub selected_location: Option<String>,
    pub egress_ipv4: Option<String>,
    pub egress_ipv4_cn: Option<String>,
    pub egress_ipv4_cn_geo: Option<String>,
    pub egress_ipv4_global: Option<String>,
    pub egress_ipv4_global_geo: Option<String>,
    pub egress_ipv6: Option<String>,
    pub egress_ipv6_cn: Option<String>,
    pub egress_ipv6_cn_geo: Option<String>,
    pub egress_ipv6_global: Option<String>,
    pub egress_ipv6_global_geo: Option<String>,
    pub egress_consistent: Option<bool>,
    pub egress_note: Option<String>,
    pub timestamp_unix: u64,
    pub rtt_ms: Option<f64>,
    pub ping_stats: Option<PingStats>,
    pub dual_stack_preflight: Vec<StackPreflightMetric>,
    pub download_stages: Vec<DownloadStageMetric>,
    pub download_mbps: Option<f64>,
    pub download_stats: Option<SpeedStats>,
    pub upload_mbps: Option<f64>,
    pub upload_stats: Option<SpeedStats>,
    pub errors: Vec<StageError>,
    pub cdn_meta: Option<CdnMeta>,
    pub range_fallback_count: u64,
    /// Multipath speed test results (v4-cn / v4-global / v6-cn / v6-global)
    pub paths: Vec<PathResult>,
    /// Traffic split detection results (auto-run in Full mode)
    pub probe_results: Vec<crate::probe::types::ProbeResult>,
}

#[derive(Debug, Serialize, Clone)]
pub struct StageError {
    pub stage: &'static str,
    pub message: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct DownloadStageMetric {
    pub name: String,
    pub duration_secs: u64,
    pub concurrency: usize,
    pub chunk_mib: u64,
    pub mbps: Option<f64>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct StackPreflightMetric {
    pub family: String,
    pub ip: String,
    pub rtt_ms: Option<f64>,
    pub download_mbps: Option<f64>,
    pub note: Option<String>,
}

fn stage_error<'a>(report: &'a Report, stage: &str) -> Option<&'a StageError> {
    report.errors.iter().find(|e| e.stage == stage)
}

/// Print a key=value dashboard row inside a box.
/// `label` is plain text for width calc; `value_raw` for padding; `value_col` has ANSI.
fn kv_row(c: &Colors, label: &str, value_raw: &str, value_col: &str) {
    let raw = format!("{label:<13} {value_raw}");
    let col = format!("{}{label:<13}{} {value_col}", c.dim, c.reset, value_col = value_col);
    box_row(c, &raw, &col);
}

pub fn output_report(report: &Report, code: u8, is_json: bool, verbose: bool) {
    if is_json {
        match serde_json::to_string_pretty(report) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("json serialize failed: {e}");
                println!("{{\"exit_code\":{code}}}");
            }
        }
        return;
    }

    let c = Colors::new();
    println!();
    box_top(&c);

    // ── Section header ────────────────────────────────────
    let hdr_raw = "  Speed Test Results";
    let hdr_col = format!("  {}{}Speed Test Results{}", c.bold, c.white, c.reset);
    box_row(&c, hdr_raw, &hdr_col);
    box_sep_row(&c);

    // ── Basic Information ─────────────────────────────────
    kv_row(&c, "Mode",
        &report.mode,
        &format!("{}{}{}", c.white, report.mode, c.reset));
    kv_row(&c, "Target",
        &report.target_host,
        &format!("{}{}{}", c.cyan, report.target_host, c.reset));
    kv_row(&c, "Proxy",
        report.proxy.as_deref().unwrap_or("-"),
        report.proxy.as_deref().unwrap_or("-"));

    // ── Egress IP ─────────────────────────────────────────
    box_sep_row(&c);
    let sec_raw = "  Egress IP  (CN / Global)";
    let sec_col = format!("  {}Egress IP{} {}(CN / Global){}", c.bold, c.reset, c.dim, c.reset);
    box_row(&c, sec_raw, &sec_col);

    let fmt_egress = |label: &str, cn: Option<&str>, global: Option<&str>| {
        let cn_s = cn.unwrap_or("-");
        let gl_s = global.unwrap_or("-");
        let mismatch = cn.is_some() && global.is_some() && cn != global;
        let (cn_col_str, gl_col_str, warn_raw, warn_col) = if mismatch {
            (
                format!("{}{cn_s}{}", c.yellow, c.reset),
                format!("{}{gl_s}{}", c.dim, c.reset),
                "  ⚠ CN≠Global".to_string(),
                format!("  {}⚠ CN≠Global{}", c.yellow, c.reset),
            )
        } else {
            let cn_c = if cn.is_some() { format!("{}{cn_s}{}", c.green, c.reset) }
                       else { format!("{}{cn_s}{}", c.dim, c.reset) };
            (cn_c, format!("{}{gl_s}{}", c.dim, c.reset), String::new(), String::new())
        };
        let raw = format!("{label:<13} {cn_s}  /  {gl_s}{warn_raw}");
        let col = format!("{}{label:<13}{}  {cn_col_str}  {}/ {} {gl_col_str}{warn_col}",
            c.dim, c.reset, c.dim, c.reset);
        box_row(&c, &raw, &col);
    };

    fmt_egress("Egress v4", report.egress_ipv4_cn.as_deref(), report.egress_ipv4_global.as_deref());
    fmt_egress("Egress v6", report.egress_ipv6_cn.as_deref(), report.egress_ipv6_global.as_deref());

    let consist = report.egress_consistent.unwrap_or(true);
    let (cs_raw, cs_col) = if consist {
        ("consistent".to_string(), format!("{}consistent{}", c.green, c.reset))
    } else {
        ("mismatch".to_string(), format!("{}mismatch{}", c.yellow, c.reset))
    };
    kv_row(&c, "Egress OK", &cs_raw, &cs_col);

    // ── Multipath Results Table ───────────────────────────
    if !report.paths.is_empty() {
        box_sep_row(&c);
        let t_raw = "  Results";
        let t_col = format!("  {}Results{}", c.bold, c.reset);
        box_row(&c, t_raw, &t_col);

        // Column widths: Path(9) + " │ "(3) + CDN(24) + " │ "(3) + Ping(8) + " │ "(3) + DL(10) + " │ "(3) + UL(9) = 72
        const CP: usize = 9;
        const CN_W: usize = 24;
        const CG: usize = 8;
        const CD: usize = 10;
        const CU: usize = 9;

        // Header row
        let th_raw = format!(
            "{:<CP$} │ {:<CN_W$} │ {:>CG$} │ {:>CD$} │ {:>CU$}",
            "Path", "CDN Node", "Ping", "Download", "Upload",
        );
        let th_col = {
            let b = c.bold; let r = c.reset; let d = c.dim;
            format!(
                "{}{:<CP$}{} {}│{} {}{:<CN_W$}{} {}│{} {:<CG$} {}│{} {:<CD$} {}│{} {:<CU$}",
                b, "Path", r, d, r,
                d, "CDN Node", r, d, r,
                "Ping", d, r,
                "Download", d, r,
                "Upload",
            )
        };
        box_row(&c, &th_raw, &th_col);

        // Separator row: ─×CP ─┼─ ─×CN_W ─┼─ ...
        let sep_raw = format!(
            "{}─┼─{}─┼─{}─┼─{}─┼─{}",
            "─".repeat(CP),
            "─".repeat(CN_W),
            "─".repeat(CG),
            "─".repeat(CD),
            "─".repeat(CU),
        );
        let sep_col = format!("{}{sep_raw}{}", c.dim, c.reset);
        box_row(&c, &sep_raw, &sep_col);

        // Data row per path
        for path in &report.paths {
            // Path column
            let path_cell = format!("{:<CP$}", path.path_id);

            // CDN Node column: IP + Location, truncated to CN_W
            let cdn_combined = {
                let ip  = path.cdn_ip.as_deref().unwrap_or("-");
                let geo = path.cdn_location.as_deref().unwrap_or("");
                if geo.is_empty() { ip.to_string() } else { format!("{ip} {geo}") }
            };
            let cdn_cell = {
                let chars: Vec<char> = cdn_combined.chars().collect();
                if chars.len() <= CN_W {
                    format!("{:<CN_W$}", cdn_combined)
                } else {
                    let mut s: String = chars[..CN_W.saturating_sub(1)].iter().collect();
                    s.push('…');
                    s
                }
            };

            // Ping column: right-aligned 8 chars (e.g. "  7.43ms")
            let ping_cell = match path.rtt_ms {
                Some(v) => format!("{:>6.2}ms", v),
                None    => format!("{:>CG$}", "-"),
            };

            // Download column: right-aligned 10 chars (e.g. "120.36 Mb✓")
            let dl_cell = match path.download_mbps {
                Some(v) => {
                    let s = if v >= 1000.0 {
                        format!("{:.1} Mb✓", v)   // "1000.0 Mb✓" = 10
                    } else {
                        format!("{:.2} Mb✓", v)   // "120.36 Mb✓" = 10
                    };
                    format!("{:>CD$}", s)
                }
                None => format!("{:>CD$}", "✗"),
            };

            // Upload column: right-aligned 9 chars (e.g. " 76.07 Mb✓")
            let ul_cell = match path.upload_mbps {
                Some(v) => {
                    let s = if v >= 1000.0 {
                        format!("{:.1} Mb✓", v)   // "1000.0 Mb✓" = 10 → truncated but rare
                    } else {
                        format!("{:.2} Mb✓", v)   // " 76.07 Mb✓" = 10 → right-padded to 9
                    };
                    format!("{:>CU$}", s)
                }
                None => format!("{:>CU$}", "✗"),
            };

            let row_raw = format!(
                "{path_cell} │ {cdn_cell} │ {ping_cell} │ {dl_cell} │ {ul_cell}",
            );

            // Colored version
            let path_col = format!("{}{path_cell}{}", c.cyan, c.reset);
            let cdn_col  = format!("{}{cdn_cell}{}", c.white, c.reset);
            let ping_col = match path.rtt_ms {
                Some(_) => format!("{}{ping_cell}{}", c.green, c.reset),
                None    => format!("{}{ping_cell}{}", c.dim, c.reset),
            };
            let dl_col = match path.download_mbps {
                Some(_) => format!("{}{dl_cell}{}", c.green, c.reset),
                None    => format!("{}{dl_cell}{}", c.red, c.reset),
            };
            let ul_col = match path.upload_mbps {
                Some(_) => format!("{}{ul_cell}{}", c.green, c.reset),
                None    => format!("{}{ul_cell}{}", c.red, c.reset),
            };
            // If the path itself failed, overwrite CDN column with the error
            let cdn_display = if path.cdn_ip.is_none() {
                if let Some(ref e) = path.error {
                    let truncated: String = e.chars().take(CN_W).collect();
                    format!("{}{:<CN_W$}{}", c.red, truncated, c.reset)
                } else {
                    cdn_col
                }
            } else {
                cdn_col
            };

            let row_col = format!(
                "{path_col} {}│{} {cdn_display} {}│{} {ping_col} {}│{} {dl_col} {}│{} {ul_col}",
                c.dim, c.reset, c.dim, c.reset, c.dim, c.reset, c.dim, c.reset,
            );
            box_row(&c, &row_raw, &row_col);
        }
    } else {
        // ── Legacy Single Path Output (backward compat) ──────
        box_sep_row(&c);
        let sp_raw = "  Speed Stages";
        let sp_col = format!("  {}Speed Stages{}", c.bold, c.reset);
        box_row(&c, sp_raw, &sp_col);

        let print_stage = |label: &str, status_ok: bool, metric_raw: &str, metric_col: &str| {
            let (st_raw, st_col) = if status_ok {
                ("✓ OK  ".to_string(), format!("{}✓ OK  {}", c.green, c.reset))
            } else {
                ("✗ FAIL".to_string(), format!("{}✗ FAIL{}", c.red, c.reset))
            };
            let raw = format!("{label:<14}  {st_raw}  {metric_raw}");
            let col = format!("{}{label:<14}{}  {st_col}  {metric_col}", c.dim, c.reset);
            box_row(&c, &raw, &col);
        };

        if report.mode == "ping" || report.mode == "full" {
            let ok = stage_error(report, "ping").is_none();
            let (m_raw, m_col) = if let Some(v) = report.rtt_ms {
                (format!("RTT  {v:.2} ms"), format!("RTT  {}{v:.2} ms{}", c.white, c.reset))
            } else {
                let msg = stage_error(report, "ping")
                    .map(|e| short_message(&e.message)).unwrap_or_else(|| "-".to_string());
                (msg.clone(), format!("{}{msg}{}", c.red, c.reset))
            };
            print_stage("Latency / ping", ok, &m_raw, &m_col);
        }

        if report.mode == "download" || report.mode == "full" {
            let ok = stage_error(report, "download").is_none();
            let (m_raw, m_col) = if let Some(v) = report.download_mbps {
                (format!("↓  {v:.2} Mbps"), format!("↓  {}{v:.2} Mbps{}", c.white, c.reset))
            } else {
                let msg = stage_error(report, "download")
                    .map(|e| short_message(&e.message)).unwrap_or_else(|| "-".to_string());
                (msg.clone(), format!("{}{msg}{}", c.red, c.reset))
            };
            print_stage("Download", ok, &m_raw, &m_col);

            if !report.download_stages.is_empty() {
                let hdr = format!("{}{:>12}  {:>4}  {:>4}  {:>6}s  result{}", c.dim, "Stage", "conc", "chunk", "dur", c.reset);
                let hdr_raw = format!("{:>12}  {:>4}  {:>4}  {:>6}s  result", "Stage", "conc", "chunk", "dur");
                box_row(&c, &hdr_raw, &hdr);
                for s in &report.download_stages {
                    let result = s.mbps
                        .map(|v| format!("{v:.2} Mbps"))
                        .unwrap_or_else(|| s.error.as_deref().map(short_message).unwrap_or_else(|| "-".to_string()));
                    let raw = format!("{:>12}  {:>4}  {:>4}  {:>7}  {result}", s.name, s.concurrency, s.chunk_mib, s.duration_secs);
                    let col = format!("{}{:>12}  {:>4}  {:>4}  {:>7}  {result}{}", c.dim, s.name, s.concurrency, s.chunk_mib, s.duration_secs, c.reset);
                    box_row(&c, &raw, &col);
                }
            }
        }

        if report.mode == "upload" || report.mode == "full" {
            let ok = stage_error(report, "upload").is_none();
            let (m_raw, m_col) = if let Some(v) = report.upload_mbps {
                (format!("↑  {v:.2} Mbps"), format!("↑  {}{v:.2} Mbps{}", c.white, c.reset))
            } else {
                let msg = stage_error(report, "upload")
                    .map(|e| short_message(&e.message)).unwrap_or_else(|| "-".to_string());
                (msg.clone(), format!("{}{msg}{}", c.red, c.reset))
            };
            print_stage("Upload", ok, &m_raw, &m_col);
        }
    }

    // ── CDN Metadata ──────────────────────────────────────
    if let Some(meta) = &report.cdn_meta {
        box_sep_row(&c);
        let cdn_raw = "  CDN Meta";
        let cdn_col = format!("  {}CDN Meta{}", c.bold, c.reset);
        box_row(&c, cdn_raw, &cdn_col);
        kv_row(&c, "Via",      meta.via.as_deref().unwrap_or("-"), meta.via.as_deref().unwrap_or("-"));
        kv_row(&c, "Cache",    meta.x_cache.as_deref().unwrap_or("-"), meta.x_cache.as_deref().unwrap_or("-"));
        kv_row(&c, "Protocol", &meta.http_version, &meta.http_version);
        if report.range_fallback_count > 0 {
            let rf = report.range_fallback_count.to_string();
            kv_row(&c, "RangeFall", &rf, &format!("{}{rf}{}", c.yellow, c.reset));
        }
    }

    // ── Verbose: candidate IPs + ping stats per path ──────
    if verbose && !report.paths.is_empty() {
        box_sep_row(&c);
        let v_raw = "  Verbose Details";
        let v_col = format!("  {}Verbose Details{}", c.bold, c.reset);
        box_row(&c, v_raw, &v_col);

        for path in &report.paths {
            // Path header
            let ph_raw = format!("  [{}]", path.path_id);
            let ph_col = format!("  {}[{}]{}", c.cyan, path.path_id, c.reset);
            box_row(&c, &ph_raw, &ph_col);

            // Resolver source
            if let Some(ref src) = path.resolver_source {
                kv_row(&c, "  DNS source", src, &format!("{}{src}{}", c.dim, c.reset));
            }

            // Candidate IPs
            if !path.candidates.is_empty() {
                let hdr_raw = format!("  {:<18}  {:<8}  {:<28}  {}", "IP", "RTT", "Location", "Sources");
                let hdr_col = format!("  {}{:<18}  {:<8}  {:<28}  {}{}", c.dim, "IP", "RTT", "Location", "Sources", c.reset);
                box_row(&c, &hdr_raw, &hdr_col);
                for cand in &path.candidates {
                    let ip   = cand.ip.as_str();
                    let rtt  = cand.rtt_ms.map(|v| format!("{v:.1}ms")).unwrap_or_else(|| "-".to_string());
                    let loc  = cand.location.as_deref().unwrap_or("-");
                    let srcs = cand.sources.join("+");
                    let sel  = if cand.selected { format!(" {}●{}", c.green, c.reset) } else { String::new() };
                    let raw  = format!("  {ip:<18}  {rtt:<8}  {loc:<28}  {srcs}");
                    let col  = format!("  {}{ip:<18}{}  {}{rtt:<8}{}  {loc:<28}  {}{srcs}{}{sel}",
                        c.white, c.reset, c.green, c.reset, c.dim, c.reset);
                    box_row(&c, &raw, &col);
                }
            }

            // Ping stats
            if let Some(ps) = &path.ping_stats {
                if ps.samples.len() >= 2 {
                    let ps_raw = format!("  ping  avg {:.1}  min {:.1}  max {:.1}  p95 {:.1}  jitter {:.1} ms",
                        ps.avg_ms, ps.min_ms, ps.max_ms, ps.p95_ms, ps.jitter_ms);
                    let ps_col = format!("  {}ping{} avg {:.1}  min {:.1}  max {:.1}  p95 {:.1}  jitter {:.1} ms",
                        c.dim, c.reset, ps.avg_ms, ps.min_ms, ps.max_ms, ps.p95_ms, ps.jitter_ms);
                    box_row(&c, &ps_raw, &ps_col);
                }
            }

            // Download stages
            if !path.download_stages.is_empty() {
                for s in &path.download_stages {
                    let result = s.mbps
                        .map(|v| format!("{v:.2} Mbps"))
                        .unwrap_or_else(|| s.error.as_deref().map(short_message).unwrap_or_else(|| "-".to_string()));
                    let raw = format!("  dl/{:<8}  conc:{:<3}  chunk:{:<4}MiB  {result}", s.name, s.concurrency, s.chunk_mib);
                    let col = format!("  {}dl/{:<8}  conc:{:<3}  chunk:{:<4}MiB{}  {result}", c.dim, s.name, s.concurrency, s.chunk_mib, c.reset);
                    box_row(&c, &raw, &col);
                }
            }
        }
    }

    // ── Footer ────────────────────────────────────────────
    box_sep_row(&c);
    let (ec_raw, ec_col) = if code == 0 {
        ("Exit Code  0".to_string(), format!("{}Exit Code{}  {}0{}", c.bold, c.reset, c.green, c.reset))
    } else {
        (format!("Exit Code  {code}"), format!("{}Exit Code{}  {}{code}{}", c.bold, c.reset, c.red, c.reset))
    };
    box_row(&c, &ec_raw, &ec_col);
    box_bot(&c);
    println!();
}
