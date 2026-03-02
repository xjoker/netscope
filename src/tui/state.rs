use crate::report::Report;

/// Status of each stage
#[derive(Debug, Clone, PartialEq)]
pub enum StageStatus {
    Waiting,
    Running,
    Ok(String),
    Fail(String),
}

impl StageStatus {
    pub fn is_done(&self) -> bool {
        matches!(self, Self::Ok(_) | Self::Fail(_))
    }
}

/// Real-time status of a single path in multi-path speed testing
#[derive(Debug, Clone)]
pub struct PathRow {
    /// e.g. "v4-cn", "v4-global", "v6-cn", "v6-global"
    pub path_id: String,
    /// Name of the currently executing sub-stage
    pub current_stage: String,
    /// Selected CDN node IP for this path
    pub cdn_ip: Option<String>,
    /// Selected CDN node location for this path
    pub cdn_location: Option<String>,
    /// Ping result (HTTP RTT)
    pub rtt_ms: Option<f64>,
    /// TCP connect latency (available when no proxy is used)
    pub tcp_rtt_ms: Option<f64>,
    /// Download result
    pub dl_mbps: Option<f64>,
    /// Upload result
    pub ul_mbps: Option<f64>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Whether this path has fully completed
    pub done: bool,
}

/// Events sent from async tasks to the TUI rendering thread
#[derive(Debug)]
pub enum Event {
    /// Egress IP detection completed
    EgressDone {
        v4_cn: Option<String>,
        v4_global: Option<String>,
        v6_cn: Option<String>,
        v6_global: Option<String>,
        v4_cn_geo: Option<String>,
        v4_global_geo: Option<String>,
        v6_cn_geo: Option<String>,
        v6_global_geo: Option<String>,
    },
    /// CN mode determined (sent after is_cn_mode is resolved in run_with_apple)
    CnMode(bool),
    /// DNS resolution completed (kept for single-path mode compatibility)
    ResolveDone { ip: String, family: String, source: String },
    /// GeoIP lookup completed (kept for single-path mode compatibility)
    GeoDone { location: String },
    /// Stage status change (ping / download / upload, single-path mode)
    StageUpdate { stage: &'static str, status: StageStatus },
    /// Multi-path: path list initialised (before speed test starts)
    PathsInit { paths: Vec<PathRow> },
    /// Multi-path: single-path progress update
    PathUpdate {
        path_id: String,
        current_stage: String,
        cdn_ip: Option<String>,
        cdn_location: Option<String>,
        rtt_ms: Option<f64>,
        tcp_rtt_ms: Option<f64>,
        dl_mbps: Option<f64>,
        ul_mbps: Option<f64>,
        error: Option<String>,
        done: bool,
    },
    /// All done, carries the final report + exit code
    Done { report: Box<Report>, code: u8 },
    /// Routing probe: real-time progress (done / total)
    ProbeProgress { done: usize, total: usize },
    /// Routing probe completed
    ProbeDone { results: Vec<crate::probe::types::ProbeResult> },
    /// Unrecoverable error
    Fatal(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultFocus {
    Speed,
    Connectivity,
}

#[derive(Debug)]
pub struct AppState {
    pub mode: String,
    pub proxy: Option<String>,
    /// Speed-test backend: "apple" or "cloudflare"
    pub backend: String,
    /// CN mode flag (None = not yet determined)
    pub cn_mode: Option<bool>,

    // Egress IPs (4 slots)
    pub egress_v4_cn: Option<String>,
    pub egress_v4_global: Option<String>,
    pub egress_v6_cn: Option<String>,
    pub egress_v6_global: Option<String>,
    pub egress_done: bool,
    // Egress IP geolocation
    pub egress_v4_cn_geo: Option<String>,
    pub egress_v4_global_geo: Option<String>,
    pub egress_v6_cn_geo: Option<String>,
    pub egress_v6_global_geo: Option<String>,

    // DNS (single-path mode)
    pub resolved_ip: Option<String>,
    pub resolved_family: Option<String>,
    pub resolved_source: Option<String>,

    // GeoIP (single-path mode)
    pub location: Option<String>,

    // Speed stages (single-path mode, used by Cloudflare backend)
    pub ping_status: StageStatus,
    pub download_status: StageStatus,
    pub upload_status: StageStatus,

    // Multi-path real-time state (used by Apple backend)
    pub paths: Vec<PathRow>,

    // Completion state
    pub finished: bool,
    pub exit_code: u8,
    pub final_report: Option<Box<Report>>,
    /// Routing probe results (Full mode)
    pub probe_results: Vec<crate::probe::types::ProbeResult>,
    /// Routing probe real-time progress: (done, total), None = not started
    pub probe_progress: Option<(usize, usize)>,

    // Animation frame counter
    pub tick: u64,
    // Result page: focused panel and per-panel scroll offsets
    pub result_focus: ResultFocus,
    pub scroll_speed: u16,
    pub scroll_conn: u16,
}

impl AppState {
    pub fn new(mode: &str, proxy: Option<String>, backend: String) -> Self {
        let (ping_status, download_status, upload_status) = match mode {
            "ping"     => (StageStatus::Running, StageStatus::Waiting, StageStatus::Waiting),
            "download" => (StageStatus::Waiting, StageStatus::Running, StageStatus::Waiting),
            "upload"   => (StageStatus::Waiting, StageStatus::Waiting, StageStatus::Running),
            _          => (StageStatus::Running, StageStatus::Waiting, StageStatus::Waiting),
        };
        AppState {
            mode: mode.to_string(),
            proxy,
            backend,
            cn_mode: None,
            egress_v4_cn: None,
            egress_v4_global: None,
            egress_v6_cn: None,
            egress_v6_global: None,
            egress_done: false,
            egress_v4_cn_geo: None,
            egress_v4_global_geo: None,
            egress_v6_cn_geo: None,
            egress_v6_global_geo: None,
            resolved_ip: None,
            resolved_family: None,
            resolved_source: None,
            location: None,
            ping_status,
            download_status,
            upload_status,
            paths: vec![],
            finished: false,
            exit_code: 0,
            final_report: None,
            probe_results: vec![],
            probe_progress: None,
            tick: 0,
            result_focus: ResultFocus::Speed,
            scroll_speed: 0,
            scroll_conn: 0,
        }
    }
}

