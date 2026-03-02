use std::net::IpAddr;

use serde::Deserialize;

use crate::network::IpFamily;
use crate::network::client::{build_aux_client_v4, build_aux_client_v6};

#[derive(Debug)]
pub struct EgressProfile {
    /// CN-preferred chosen IP (CN sources win on mismatch)
    pub ipv4: Option<IpAddr>,
    /// CN-side detection (itdog / ip.3322.net / ipip.net)
    pub ipv4_cn: Option<IpAddr>,
    /// Global-side detection (icanhazip / ipify)
    pub ipv4_global: Option<IpAddr>,
    pub ipv6: Option<IpAddr>,
    pub ipv6_cn: Option<IpAddr>,
    pub ipv6_global: Option<IpAddr>,
    pub consistent: bool,
    pub note: String,
}

#[derive(Debug)]
struct EgressSample {
    is_cn:  bool,
    family: IpFamily,
    ip:     IpAddr,
}

#[derive(Debug, Deserialize)]
struct IpifyJson {
    ip: String,
}

#[derive(Debug, Deserialize)]
struct IpipJson {
    #[serde(rename = "data")]
    data: IpipData,
}

#[derive(Debug, Deserialize)]
struct IpipData {
    ip: String,
}

fn parse_ip_from_body(body: &str) -> Option<IpAddr> {
    body.trim().parse::<IpAddr>().ok()
}

async fn query_plain(
    client: &reqwest::Client,
    is_cn: bool,
    family: IpFamily,
    url: &'static str,
) -> Option<EgressSample> {
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() { return None; }
    let body = resp.text().await.ok()?;
    let ip = parse_ip_from_body(&body)?;
    family_check(is_cn, family, ip)
}

async fn query_ipify(
    client: &reqwest::Client,
    is_cn: bool,
    family: IpFamily,
    url: &'static str,
) -> Option<EgressSample> {
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() { return None; }
    let body: IpifyJson = resp.json().await.ok()?;
    let ip = parse_ip_from_body(&body.ip)?;
    family_check(is_cn, family, ip)
}

async fn query_ipip(
    client: &reqwest::Client,
    is_cn: bool,
    family: IpFamily,
    url: &'static str,
) -> Option<EgressSample> {
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() { return None; }
    let body: IpipJson = resp.json().await.ok()?;
    let ip = parse_ip_from_body(&body.data.ip)?;
    family_check(is_cn, family, ip)
}

fn family_check(
    is_cn: bool,
    family: IpFamily,
    ip: IpAddr,
) -> Option<EgressSample> {
    match (family, ip) {
        (IpFamily::V4, IpAddr::V4(_)) | (IpFamily::V6, IpAddr::V6(_)) => {
            Some(EgressSample { is_cn, family, ip })
        }
        _ => None,
    }
}

/// Pick majority IP from a slice of samples; itiebreak: first in list.
fn majority_ip(samples: &[&EgressSample]) -> Option<IpAddr> {
    if samples.is_empty() { return None; }
    let mut counts: std::collections::BTreeMap<String, (IpAddr, usize)> = Default::default();
    for s in samples {
        let e = counts.entry(s.ip.to_string()).or_insert((s.ip, 0));
        e.1 += 1;
    }
    counts.into_values().max_by_key(|v| v.1).map(|v| v.0)
}

/// Returns (cn_ip, global_ip, chosen_ip, consistent, note).
fn choose_egress_split(
    samples: &[EgressSample],
    family: IpFamily,
) -> (Option<IpAddr>, Option<IpAddr>, Option<IpAddr>, bool, String) {
    let fam: Vec<_> = samples.iter().filter(|s| s.family == family).collect();
    if fam.is_empty() {
        return (None, None, None, true, format!("{} unavailable", family.as_str()));
    }

    let cn_samples:  Vec<_> = fam.iter().filter(|s| s.is_cn).copied().collect();
    let gl_samples:  Vec<_> = fam.iter().filter(|s| !s.is_cn).copied().collect();

    let cn_ip     = majority_ip(&cn_samples);
    let global_ip = majority_ip(&gl_samples);

    match (cn_ip, global_ip) {
        (Some(cn), Some(gl)) if cn == gl => (
            Some(cn), Some(gl), Some(cn), true,
            format!("{} consistent ({})", family.as_str(), cn),
        ),
        (Some(cn), Some(gl)) => (
            Some(cn), Some(gl), Some(cn), false,
            format!("{} CN={} Global={} → using CN", family.as_str(), cn, gl),
        ),
        (Some(cn), None) => (
            Some(cn), None, Some(cn), true,
            format!("{} CN={} (no global)", family.as_str(), cn),
        ),
        (None, Some(gl)) => (
            None, Some(gl), Some(gl), true,
            format!("{} Global={} (no CN)", family.as_str(), gl),
        ),
        (None, None) => (None, None, None, true, format!("{} unavailable", family.as_str())),
    }
}

pub async fn detect_egress_profile(timeout_secs: u64, proxy: Option<&str>) -> EgressProfile {
    let t = timeout_secs.max(1).min(8);

    // v4 and v6 clients are built independently and do not interfere with each other
    let client_v4 = build_aux_client_v4(t, proxy).ok();
    let client_v6 = build_aux_client_v6(t, proxy).ok();

    let mut tasks = tokio::task::JoinSet::<Option<EgressSample>>::new();

    macro_rules! spawn_plain {
        ($c:expr, $cn:expr, $fam:expr, $url:expr) => {{
            let cl = $c.clone();
            tasks.spawn(async move { query_plain(&cl, $cn, $fam, $url).await });
        }};
    }
    macro_rules! spawn_ipify {
        ($c:expr, $cn:expr, $fam:expr, $url:expr) => {{
            let cl = $c.clone();
            tasks.spawn(async move { query_ipify(&cl, $cn, $fam, $url).await });
        }};
    }
    macro_rules! spawn_ipip {
        ($c:expr, $cn:expr, $fam:expr, $url:expr) => {{
            let cl = $c.clone();
            tasks.spawn(async move { query_ipip(&cl, $cn, $fam, $url).await });
        }};
    }

    // CN v4 (using the force-IPv4 client)
    if let Some(ref c4) = client_v4 {
        spawn_plain!(c4,  true,  IpFamily::V4, "https://ipv4_cu.itdog.cn/");
        spawn_plain!(c4,  true,  IpFamily::V4, "https://ip.3322.net/");
        spawn_ipip!( c4,  true,  IpFamily::V4, "https://myip.ipip.net/json");
        // Global v4
        spawn_plain!(c4, false, IpFamily::V4, "https://ipv4.icanhazip.com/");
        spawn_ipify!(c4, false, IpFamily::V4, "https://api4.ipify.org/?format=json");
    }

    // CN v6 (using the force-IPv6 client; when a proxy is present the proxy determines the egress)
    if let Some(ref c6) = client_v6 {
        spawn_plain!(c6,  true,  IpFamily::V6, "https://ipv6_ct.itdog.cn/");
        spawn_plain!(c6,  true,  IpFamily::V6, "https://6.ipw.cn/");
        // Global v6 — pure IPv6-only endpoint (domain resolves only to AAAA; proxy must support IPv6 egress)
        spawn_plain!(c6, false, IpFamily::V6, "https://ipv6.icanhazip.com/");
        spawn_ipify!(c6, false, IpFamily::V6, "https://api6.ipify.org/?format=json");
        spawn_plain!(c6, false, IpFamily::V6, "https://v6.ident.me/");
        // 6.ipw.cn/api/ip returns {"ip":"..."} JSON format, requires the ipify parser
        spawn_ipify!(c6, false, IpFamily::V6, "https://6.ipw.cn/api/ip");
        spawn_ipify!(c6, false, IpFamily::V6, "https://api64.ipify.org/?format=json");
    }

    let mut samples = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        if let Ok(Some(s)) = joined { samples.push(s); }
    }

    let (ipv4_cn, ipv4_global, ipv4, consistent_v4, note_v4) = choose_egress_split(&samples, IpFamily::V4);
    let (ipv6_cn, ipv6_global, ipv6, consistent_v6, note_v6) = choose_egress_split(&samples, IpFamily::V6);
    EgressProfile {
        ipv4, ipv4_cn, ipv4_global,
        ipv6, ipv6_cn, ipv6_global,
        consistent: consistent_v4 && consistent_v6,
        note: format!("{note_v4}; {note_v6}"),
    }
}
