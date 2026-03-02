use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use reqwest::Url;

#[derive(Debug, Parser)]
#[command(name = "netscope", about = "CDN speed test & connectivity probe (Apple / Cloudflare)")]
pub struct Cli {
    #[arg(long)]
    pub country: Option<String>,
    #[arg(long)]
    pub proxy: Option<String>,
    #[arg(long, default_value_t = 8)]
    pub timeout: u64,
    #[arg(long)]
    pub json: bool,
    #[arg(long)]
    pub verbose: bool,
    #[arg(
        long,
        default_value = "apple",
        value_parser = ["apple", "cloudflare"],
        help = "Speed test backend: apple (default, better for China) or cloudflare"
    )]
    pub backend: String,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    Ping {
        #[arg(long, default_value_t = 8)]
        count: u32,
    },
    Download {
        #[arg(long, default_value_t = 20)]
        duration: u64,
    },
    Upload {
        #[arg(long, default_value_t = 16)]
        ul_mib: u64,
        #[arg(long, default_value_t = 3)]
        ul_repeat: u32,
    },
    Full {
        #[arg(long, default_value_t = 8)]
        count: u32,
        #[arg(long, default_value_t = 20)]
        duration: u64,
        #[arg(long, default_value_t = 16)]
        ul_mib: u64,
        #[arg(long, default_value_t = 3)]
        ul_repeat: u32,
    },
    Probe {
        #[arg(long, default_value_t = 6)]
        concurrency: usize,
        #[arg(long, default_value_t = 10)]
        probe_timeout: u64,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        site: Option<String>,
        #[arg(long, default_value_t = false)]
        skip_geo: bool,
    },
}

pub fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Ping { .. } => "ping",
        Command::Download { .. } => "download",
        Command::Upload { .. } => "upload",
        Command::Full { .. } => "full",
        Command::Probe { .. } => "probe",
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
