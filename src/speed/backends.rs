use anyhow::Result;
use reqwest::Url;

/// Speed test backend selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpeedBackend {
    #[default]
    Apple,
    Cloudflare,
}

impl SpeedBackend {
    /// Parse from CLI string value.
    pub fn from_str(s: &str) -> Self {
        match s {
            "cloudflare" => Self::Cloudflare,
            _ => Self::Apple,
        }
    }

    /// Ping/latency probe URL.
    pub fn ping_url(&self, base: &Url) -> Result<Url> {
        match self {
            Self::Apple => Ok(base.join("/api/v1/gm/small")?),
            Self::Cloudflare => Ok(Url::parse("https://speed.cloudflare.com/cdn-cgi/trace")?),
        }
    }

    /// Download URL for the given chunk size in bytes.
    pub fn download_url(&self, base: &Url, chunk_bytes: u64) -> Result<Url> {
        match self {
            Self::Apple => Ok(base.join("/api/v1/gm/large")?),
            Self::Cloudflare => Ok(Url::parse(&format!(
                "https://speed.cloudflare.com/__down?bytes={chunk_bytes}"
            ))?),
        }
    }

    /// Upload endpoint URL.
    pub fn upload_url(&self, base: &Url) -> Result<Url> {
        match self {
            Self::Apple => Ok(base.join("/api/v1/gm/slurp")?),
            Self::Cloudflare => Ok(Url::parse("https://speed.cloudflare.com/__up")?),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apple_base() -> Url {
        Url::parse("https://mensura.cdn-apple.com").unwrap()
    }

    fn cf_base() -> Url {
        Url::parse("https://speed.cloudflare.com").unwrap()
    }

    // ── from_str ──────────────────────────────────────────────────────────────

    #[test]
    fn from_str_cloudflare() {
        assert_eq!(SpeedBackend::from_str("cloudflare"), SpeedBackend::Cloudflare);
    }

    #[test]
    fn from_str_apple_explicit() {
        assert_eq!(SpeedBackend::from_str("apple"), SpeedBackend::Apple);
    }

    #[test]
    fn from_str_unknown_defaults_to_apple() {
        assert_eq!(SpeedBackend::from_str("unknown"), SpeedBackend::Apple);
    }

    #[test]
    fn from_str_empty_defaults_to_apple() {
        assert_eq!(SpeedBackend::from_str(""), SpeedBackend::Apple);
    }

    // ── ping_url ──────────────────────────────────────────────────────────────

    #[test]
    fn apple_ping_url_contains_small() {
        let url = SpeedBackend::Apple.ping_url(&apple_base()).unwrap();
        assert!(url.path().contains("small"), "Apple ping URL: {url}");
    }

    #[test]
    fn cloudflare_ping_url_contains_trace() {
        let url = SpeedBackend::Cloudflare.ping_url(&cf_base()).unwrap();
        assert!(url.as_str().contains("cdn-cgi/trace"), "CF ping URL: {url}");
    }

    // ── download_url ──────────────────────────────────────────────────────────

    #[test]
    fn apple_download_url_contains_large() {
        let url = SpeedBackend::Apple.download_url(&apple_base(), 1024).unwrap();
        assert!(url.path().contains("large"), "Apple download URL: {url}");
    }

    #[test]
    fn cloudflare_download_url_contains_bytes_param() {
        let chunk = 4 * 1024 * 1024_u64; // 4 MiB
        let url = SpeedBackend::Cloudflare.download_url(&cf_base(), chunk).unwrap();
        assert!(
            url.as_str().contains(&chunk.to_string()),
            "CF download URL must contain chunk size: {url}"
        );
        assert!(url.path().contains("__down"), "CF download URL: {url}");
    }

    // ── upload_url ────────────────────────────────────────────────────────────

    #[test]
    fn apple_upload_url_contains_slurp() {
        let url = SpeedBackend::Apple.upload_url(&apple_base()).unwrap();
        assert!(url.path().contains("slurp"), "Apple upload URL: {url}");
    }

    #[test]
    fn cloudflare_upload_url_contains_up() {
        let url = SpeedBackend::Cloudflare.upload_url(&cf_base()).unwrap();
        assert!(url.path().contains("__up"), "CF upload URL: {url}");
    }
}
