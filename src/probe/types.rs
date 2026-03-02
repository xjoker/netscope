use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeMethod {
    Trace,
    Http,
    ApiDirect,
    Header,
}

#[derive(Debug, Clone)]
pub struct ProbeTarget {
    pub name: &'static str,
    pub category: &'static str,
    pub method: ProbeMethod,
    pub url: &'static str,
    pub header_key: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProbeGeoInfo {
    pub country_code: Option<String>,
    pub country: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub isp: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProbeResult {
    pub name: String,
    pub category: String,
    pub url: String,
    pub reachable: bool,
    pub status_code: Option<u16>,
    pub ttfb_ms: Option<f64>,
    pub exit_ip: Option<String>,
    pub colo: Option<String>,
    pub loc: Option<String>,
    pub geo: Option<ProbeGeoInfo>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProbeReport {
    pub proxy: Option<String>,
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
    pub total: usize,
    pub reachable: usize,
    pub unreachable: usize,
    pub results: Vec<ProbeResult>,
}
