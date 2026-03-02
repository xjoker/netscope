use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use reqwest::Url;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "netscope",
    about = "CDN speed test & connectivity probe (Apple / Cloudflare)",
    long_about = "CDN speed test & connectivity probe.\n\
                  Measures latency, download and upload speed via Apple CDN or Cloudflare,\n\
                  with multi-path testing (v4-CN / v4-Global / v6-CN / v6-Global)\n\
                  and connectivity probing for 70+ sites across 11 categories.\n\
                  \n\
                  Without a subcommand, runs full speed test (latency + download + upload).",
)]
pub struct Cli {
    #[arg(
        long,
        value_name = "CC",
        help = "Force routing country for CDN node selection (e.g. CN, HK, SG, US).\n\
                When set to CN, uses mainland China DoH resolvers (AliDNS / DNSPod / 360DNS)\n\
                and tests both CN and Global paths separately.\n\
                Auto-detected from egress IP if not specified."
    )]
    pub country: Option<String>,

    #[arg(
        long,
        value_name = "URL",
        help = "Proxy URL for all requests.\n\
                Supported schemes: http, https, socks5, socks5h.\n\
                Example: --proxy socks5://127.0.0.1:1080\n\
                CN-side paths always bypass the proxy to reach mainland resolvers directly."
    )]
    pub proxy: Option<String>,

    #[arg(
        long,
        default_value_t = 8,
        value_name = "SECS",
        help = "Per-request timeout in seconds (default: 8).\n\
                Applies to DNS, ping, download and upload requests individually.\n\
                Increase on slow or high-latency connections."
    )]
    pub timeout: u64,

    #[arg(
        long,
        help = "Output results as JSON to stdout instead of the interactive TUI.\n\
                Progress messages are written to stderr.\n\
                Useful for scripting, CI, or piping into jq."
    )]
    pub json: bool,

    #[arg(
        long,
        requires = "json",
        help = "Print extra diagnostic fields in JSON output (requires --json).\n\
                Adds per-path candidate IP details and DNS resolver sources.\n\
                Cannot be combined with TUI mode (i.e. requires --json)."
    )]
    pub verbose: bool,

    #[arg(
        long,
        default_value = "apple",
        value_parser = ["apple", "cloudflare"],
        help = "Speed test backend (default: apple).\n\
                  apple      — Apple CDN (mensura.cdn-apple.com). Uses DoH-based IP selection\n\
                               and IP pinning. Optimised for detecting mainland China routing.\n\
                  cloudflare — Cloudflare (speed.cloudflare.com). Direct connection,\n\
                               no IP pinning. Suitable for general global speed tests."
    )]
    pub backend: String,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Measure latency only (HTTP RTT + TCP connect time).
    Ping {
        #[arg(
            long,
            default_value_t = 8,
            value_name = "N",
            help = "Number of ping requests per path (default: 8).\n\
                    More samples give a more accurate median and jitter reading."
        )]
        count: u32,
    },

    /// Measure download speed only.
    Download {
        #[arg(
            long,
            default_value_t = 20,
            value_name = "SECS",
            help = "Total download test duration in seconds (default: 20).\n\
                    The test runs multiple stages (single-stream → multi-stream)\n\
                    and reports the best result."
        )]
        duration: u64,
    },

    /// Measure upload speed only.
    Upload {
        #[arg(
            long,
            default_value_t = 16,
            value_name = "MiB",
            help = "Payload size per upload request in MiB (default: 16).\n\
                    Larger values yield more accurate throughput on fast connections.\n\
                    Cloudflare backend is capped at 4 MiB due to server-side limits."
        )]
        ul_mib: u64,
        #[arg(
            long,
            default_value_t = 3,
            value_name = "N",
            help = "Number of upload repetitions (default: 3).\n\
                    The median result is reported."
        )]
        ul_repeat: u32,
    },

    /// Full speed test: latency + download + upload (default when no subcommand given).
    ///
    /// Also runs a connectivity probe after the speed test completes.
    Full {
        #[arg(
            long,
            default_value_t = 8,
            value_name = "N",
            help = "Number of ping requests per path (default: 8)."
        )]
        count: u32,
        #[arg(
            long,
            default_value_t = 20,
            value_name = "SECS",
            help = "Download test duration in seconds (default: 20)."
        )]
        duration: u64,
        #[arg(
            long,
            default_value_t = 16,
            value_name = "MiB",
            help = "Upload payload size per request in MiB (default: 16)."
        )]
        ul_mib: u64,
        #[arg(
            long,
            default_value_t = 3,
            value_name = "N",
            help = "Number of upload repetitions (default: 3)."
        )]
        ul_repeat: u32,
    },

    /// Probe connectivity to 70+ sites without running a speed test.
    ///
    /// Tests reachability and TTFB latency across 11 categories:
    /// ai, social, streaming, search, news, game, dev, cloud, crypto, nsfw, cn.
    Probe {
        #[arg(
            long,
            default_value_t = 6,
            value_name = "N",
            help = "Number of concurrent probe requests (default: 6).\n\
                    Higher values finish faster but may trigger rate limiting."
        )]
        concurrency: usize,
        #[arg(
            long,
            default_value_t = 10,
            value_name = "SECS",
            help = "Per-site request timeout in seconds (default: 10)."
        )]
        probe_timeout: u64,
        #[arg(
            long,
            value_name = "LIST",
            help = "Comma-separated list of categories to probe.\n\
                    Available: ai, social, streaming, search, news, game, dev, cloud, crypto, nsfw, cn\n\
                    Example: --category ai,social,streaming"
        )]
        category: Option<String>,
        #[arg(
            long,
            value_name = "KEYWORD",
            help = "Filter sites by name keyword (case-insensitive substring match).\n\
                    Example: --site github"
        )]
        site: Option<String>,
        #[arg(
            long,
            default_value_t = false,
            help = "Skip GeoIP location lookup for each site.\n\
                    Speeds up probing but omits country information from results."
        )]
        skip_geo: bool,
    },
}

pub fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Ping { .. }     => "ping",
        Command::Download { .. } => "download",
        Command::Upload { .. }   => "upload",
        Command::Full { .. }     => "full",
        Command::Probe { .. }    => "probe",
    }
}

pub fn validate_proxy_url(proxy_url: &str) -> Result<()> {
    let parsed = Url::parse(proxy_url).context("invalid --proxy url")?;
    let scheme = parsed.scheme();
    if matches!(scheme, "http" | "https" | "socks5" | "socks5h") {
        Ok(())
    } else {
        Err(anyhow!(
            "unsupported proxy scheme: {scheme}, only http/https/socks5/socks5h"
        ))
    }
}

pub fn sanitize_proxy_display(proxy_url: &str) -> String {
    match Url::parse(proxy_url) {
        Ok(url) => {
            let host = url.host_str().unwrap_or("-");
            let scheme = url.scheme();
            let port = url.port_or_known_default().unwrap_or(0);
            format!("{scheme}://{host}:{port}")
        }
        Err(_) => "invalid-proxy".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── validate_proxy_url ────────────────────────────────────────────────────

    #[test]
    fn proxy_http_valid() {
        assert!(validate_proxy_url("http://127.0.0.1:8080").is_ok());
    }

    #[test]
    fn proxy_https_valid() {
        assert!(validate_proxy_url("https://proxy.example.com:443").is_ok());
    }

    #[test]
    fn proxy_socks5_valid() {
        assert!(validate_proxy_url("socks5://127.0.0.1:1080").is_ok());
    }

    #[test]
    fn proxy_socks5h_valid() {
        assert!(validate_proxy_url("socks5h://proxy.example.com:1080").is_ok());
    }

    #[test]
    fn proxy_ftp_rejected() {
        assert!(validate_proxy_url("ftp://proxy.example.com:21").is_err());
    }

    #[test]
    fn proxy_invalid_url_rejected() {
        assert!(validate_proxy_url("not_a_url").is_err());
    }

    // ── sanitize_proxy_display ────────────────────────────────────────────────

    #[test]
    fn sanitize_strips_credentials() {
        // Password must NOT appear in output
        let result = sanitize_proxy_display("http://user:secret@proxy.example.com:8080");
        assert!(!result.contains("secret"), "password leaked: {result}");
        assert!(result.contains("proxy.example.com"));
    }

    #[test]
    fn sanitize_preserves_host_and_port() {
        let result = sanitize_proxy_display("socks5://127.0.0.1:1080");
        assert_eq!(result, "socks5://127.0.0.1:1080");
    }

    #[test]
    fn sanitize_invalid_url_returns_sentinel() {
        let result = sanitize_proxy_display(":::garbage:::");
        assert_eq!(result, "invalid-proxy");
    }

    // ── command_name ──────────────────────────────────────────────────────────

    #[test]
    fn command_name_all_variants() {
        assert_eq!(command_name(&Command::Ping { count: 3 }), "ping");
        assert_eq!(command_name(&Command::Download { duration: 10 }), "download");
        assert_eq!(command_name(&Command::Upload { ul_mib: 4, ul_repeat: 2 }), "upload");
        assert_eq!(command_name(&Command::Full { count: 8, duration: 20, ul_mib: 16, ul_repeat: 3 }), "full");
        assert_eq!(
            command_name(&Command::Probe {
                concurrency: 6,
                probe_timeout: 10,
                category: None,
                site: None,
                skip_geo: false,
            }),
            "probe"
        );
    }
}
