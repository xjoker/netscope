use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Padding, Paragraph},
};

use crate::tui::state::AppState;

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn spin(tick: u64) -> &'static str {
    SPINNER[(tick as usize) % SPINNER.len()]
}


fn cyan_bold(s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
}
fn white_bold(s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
}
fn dim(s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), Style::default().fg(Color::DarkGray))
}
fn green(s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), Style::default().fg(Color::Green))
}
fn green_bold(s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
}
fn yellow(s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), Style::default().fg(Color::Yellow))
}
fn red(s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), Style::default().fg(Color::Red))
}
fn color(s: impl Into<String>, c: Color) -> Span<'static> {
    Span::styled(s.into(), Style::default().fg(c))
}

/// Return a color based on RTT latency (ms):
/// ≤ 80ms → Green, ≤ 180ms → Yellow, ≤ 350ms → LightRed, > 350ms → Red
fn rtt_color(ms: f64) -> Color {
    if ms <= 80.0       { Color::Green }
    else if ms <= 180.0 { Color::Yellow }
    else if ms <= 350.0 { Color::LightRed }
    else                { Color::Red }
}

fn rtt_span(label: String, ms: f64) -> Span<'static> {
    Span::styled(label, Style::default().fg(rtt_color(ms)).add_modifier(Modifier::BOLD))
}

fn ping_sparkline(samples: &[f64]) -> String {
    if samples.len() < 2 {
        return String::new();
    }
    let min = samples.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let blocks = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if (max - min).abs() < 0.001 {
        return blocks[3].to_string().repeat(samples.len());
    }
    samples
        .iter()
        .map(|&v| {
            let idx = ((v - min) / (max - min) * 7.0).round() as usize;
            blocks[idx.min(7)]
        })
        .collect()
}

fn speed_bar(mbps: f64, max: f64, width: usize) -> String {
    let ratio = (mbps / max).min(1.0);
    let filled = (ratio * width as f64).round() as usize;
    format!(
        "{}{}",
        "█".repeat(filled),
        "░".repeat(width - filled),
    )
}

// ═══════════════════════════════════════════════════════════════
// Header: extended version, holding badges + Egress IP + Geo (2~5 lines)
// ═══════════════════════════════════════════════════════════════

fn draw_header(f: &mut Frame, area: Rect, state: &AppState) {
    let ver = env!("APP_VERSION");

    let backend_upper = state.backend.to_uppercase();
    let (badge_text, badge_col) = if state.backend == "cloudflare" {
        (format!(" {backend_upper} "), Color::LightYellow)
    } else {
        (format!(" {backend_upper} "), Color::Cyan)
    };

    let cn_badge: Option<(&str, Color)> = match state.cn_mode {
        Some(true)  => Some((" CN Mode ", Color::LightRed)),
        Some(false) => Some((" Global  ", Color::Blue)),
        None        => None,
    };

    let proxy_str = match &state.proxy {
        Some(p) => format!("  proxy {p}"),
        None    => String::new(),
    };

    // Line 1: title + badges + mode
    let title_line = Line::from(vec![
        Span::raw(" "),
        white_bold("netscope"),
        dim(format!("  v{ver}")),
        Span::raw("   "),
        Span::styled(&badge_text, Style::default().fg(Color::Black).bg(badge_col).add_modifier(Modifier::BOLD)),
        if let Some((txt, col)) = cn_badge {
            Span::styled(format!(" {txt}"), Style::default().fg(Color::Black).bg(col).add_modifier(Modifier::BOLD))
        } else {
            Span::raw("")
        },
        dim(format!("   mode {}", state.mode)),
        dim(&proxy_str),
    ]);

    let block = Block::default()
        .title(title_line)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .padding(Padding::horizontal(1));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Lines 2~N: Egress IP summary (shown after detection, with geo)
    let mut lines: Vec<Line> = vec![];

    if !state.egress_done {
        lines.push(Line::from(vec![
            Span::styled(spin(state.tick).to_string(), Style::default().fg(Color::Cyan)),
            dim("  detecting egress IPs..."),
        ]));
    } else {
        // v4
        let v4_line = egress_header_line(
            "v4",
            state.egress_v4_cn.as_deref(),
            state.egress_v4_global.as_deref(),
            state.egress_v4_cn_geo.as_deref(),
            state.egress_v4_global_geo.as_deref(),
        );
        for l in v4_line { lines.push(l); }

        // v6 (only shown when v6 is available)
        if state.egress_v6_cn.is_some() || state.egress_v6_global.is_some() {
            let v6_line = egress_header_line(
                "v6",
                state.egress_v6_cn.as_deref(),
                state.egress_v6_global.as_deref(),
                state.egress_v6_cn_geo.as_deref(),
                state.egress_v6_global_geo.as_deref(),
            );
            for l in v6_line { lines.push(l); }
        }
    }

    f.render_widget(Paragraph::new(lines), inner);
}

/// Generate egress display lines for a single IP family (1~2 lines: CN/GL + geo)
fn egress_header_line(
    label: &'static str,
    cn: Option<&str>,
    gl: Option<&str>,
    cn_geo: Option<&str>,
    gl_geo: Option<&str>,
) -> Vec<Line<'static>> {
    let mut out = vec![];
    let mismatch = cn.is_some() && gl.is_some() && cn != gl;

    let cn_s  = cn.unwrap_or("-");
    let gl_s  = gl.unwrap_or("-");

    let cn_span = if mismatch { yellow(cn_s.to_string()) }
                  else if cn.is_some() { green(cn_s.to_string()) }
                  else { dim(cn_s.to_string()) };

    let gl_span = if mismatch { yellow(gl_s.to_string()) } else { dim(gl_s.to_string()) };

    if mismatch {
        // CN and GL differ: display on separate lines
        let cn_geo_s = cn_geo.map(shorten_geo).unwrap_or_default();
        let gl_geo_s = gl_geo.map(shorten_geo).unwrap_or_default();
        out.push(Line::from(vec![
            dim(format!("{label} CN ")), cn_span,
            if !cn_geo_s.is_empty() { dim(format!("  {cn_geo_s}")) } else { Span::raw("") },
        ]));
        out.push(Line::from(vec![
            dim(format!("{}  GL ", " ".repeat(label.len()))), gl_span,
            if !gl_geo_s.is_empty() { dim(format!("  {gl_geo_s}")) } else { Span::raw("") },
            yellow("  ⚠ split routing".to_string()),
        ]));
    } else {
        // CN == GL or only one exists: single line
        let ip_s = cn.or(gl).unwrap_or("-");
        let ip_span = if cn.is_some() { green(ip_s.to_string()) } else { dim(ip_s.to_string()) };
        let geo_s = cn_geo.or(gl_geo).map(shorten_geo).unwrap_or_default();
        out.push(Line::from(vec![
            dim(format!("{label}    ")), ip_span,
            if !geo_s.is_empty() { dim(format!("  {geo_s}")) } else { Span::raw("") },
        ]));
    }
    out
}

/// Simplify a geo string: keep only country/city + the last parenthesised segment (ISP)
/// e.g. "China/Fujian/Fuzhou (AS4134 | Chinanet)" → "Fuzhou (Chinanet)"
fn shorten_geo(geo: &str) -> String {
    // Extract parenthesised content (ISP)
    let isp = if let (Some(l), Some(r)) = (geo.find('('), geo.rfind(')')) {
        let inner = &geo[l+1..r];
        // If " | " is present, take the second half (provider name)
        if let Some(pos) = inner.find(" | ") {
            inner[pos+3..].trim().to_string()
        } else {
            inner.trim().to_string()
        }
    } else {
        String::new()
    };

    // Extract the city before the last "/"
    let slash_part = if let Some(p) = geo.find('(') { &geo[..p] } else { geo };
    let city = slash_part.trim_end_matches('/').trim()
        .rsplit('/')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();

    if !city.is_empty() && !isp.is_empty() {
        format!("{city} ({isp})")
    } else if !city.is_empty() {
        city
    } else {
        geo.chars().take(40).collect()
    }
}

// ═══════════════════════════════════════════════════════════════
// In-progress view: draw()
// Header(dynamic height) / Body(Min) / Footer(3)
// Body = left column (Egress + DNS) | right column (multi-path table or single-path stages)
// ═══════════════════════════════════════════════════════════════

pub fn draw(f: &mut Frame, state: &AppState) {
    let area = f.area();

    // Header height: detecting=3, no v6=4, has v6 and split=6, otherwise 5
    let header_h = header_height(state);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_h),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .margin(1)
        .split(area);

    draw_header(f, chunks[0], state);
    draw_body(f, chunks[1], state);
    draw_footer(f, chunks[2], state);
}

fn header_height(state: &AppState) -> u16 {
    if !state.egress_done { return 3; }
    let has_v6 = state.egress_v6_cn.is_some() || state.egress_v6_global.is_some();
    let v4_split = state.egress_v4_cn.is_some() && state.egress_v4_global.is_some()
        && state.egress_v4_cn != state.egress_v4_global;
    let v6_split = has_v6 && state.egress_v6_cn.is_some() && state.egress_v6_global.is_some()
        && state.egress_v6_cn != state.egress_v6_global;
    let rows = (if v4_split { 2 } else { 1 }) + (if has_v6 { if v6_split { 2 } else { 1 } } else { 0 });
    2 + rows as u16  // border(2) + content rows
}

// ── Body ────────────────────────────────────────────────────────

fn draw_body(f: &mut Frame, area: Rect, state: &AppState) {
    if !state.paths.is_empty() {
        // Multi-path mode: Node Info (full-width top) + Speed Progress table (bottom)
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(6), Constraint::Length(state.paths.len() as u16 + 5)])
            .split(area);
        draw_left_minimal(f, rows[0], state);
        draw_multipath_progress(f, rows[1], state);
    } else {
        // Pre-test / single-path fallback: Node Info full-width
        draw_left_minimal(f, area, state);
    }
}

// ── Left column (in-progress): shows only DNS resolution result + CDN node ──

fn draw_left_minimal(f: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .title(Line::from(vec![Span::raw(" "), dim("Node Info"), Span::raw(" ")]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .padding(Padding::horizontal(1));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = vec![];

    // DNS / CDN node info
    if state.backend == "cloudflare" {
        // Cloudflare skips DoH — show direct connection note
        lines.push(Line::from(vec![cyan_bold("  CDN")]));
        lines.push(Line::from(vec![dim("  "), dim("speed.cloudflare.com")]));
        lines.push(Line::from(vec![dim("  "), dim("direct (no DoH)")]));
    } else {
        lines.push(Line::from(vec![cyan_bold("  DNS")]));
        if let Some(ip) = &state.resolved_ip {
            let family = state.resolved_family.as_deref().unwrap_or("-");
            let source = state.resolved_source.as_deref().unwrap_or("-");
            lines.push(Line::from(vec![
                dim("  "), Span::styled(format!("{family:<4}"), Style::default().fg(Color::White)),
                dim(source.to_string()),
            ]));
            lines.push(Line::from(vec![dim("  "), white_bold(ip.clone())]));
        } else {
            lines.push(Line::from(vec![
                dim("  "),
                Span::styled(spin(state.tick).to_string(), Style::default().fg(Color::Cyan)),
                dim(" resolving..."),
            ]));
        }
    }

    // Multi-path mode: show CDN node per path
    if !state.paths.is_empty() {
        let located: Vec<_> = state.paths.iter()
            .filter(|r| r.cdn_ip.is_some())
            .collect();
        if !located.is_empty() {
            lines.push(Line::from(vec![]));
            lines.push(Line::from(vec![cyan_bold("  CDN Node")]));
            for row in &located {
                let ip_s = row.cdn_ip.as_deref().unwrap_or("-");
                lines.push(Line::from(vec![
                    dim(format!("  {:<9}", row.path_id)),
                    Span::styled(ip_s.to_string(), Style::default().fg(Color::White)),
                ]));
                if let Some(loc) = &row.cdn_location {
                    let short = shorten_geo(loc);
                    lines.push(Line::from(vec![
                        dim(format!("  {:<9}", "")),
                        green(short),
                    ]));
                }
            }
        }
    } else if let Some(loc) = &state.location {
        lines.push(Line::from(vec![]));
        lines.push(Line::from(vec![cyan_bold("  Location")]));
        lines.push(Line::from(vec![dim("  "), green(shorten_geo(loc))]));
    }

    f.render_widget(Paragraph::new(lines), inner);
}

// ── Multi-path full-width progress table ────────────────────────────────────────────────

fn draw_multipath_progress(f: &mut Frame, area: Rect, state: &AppState) {
    let tick = state.tick;

    let block = Block::default()
        .title(Line::from(vec![
            Span::raw(" "),
            cyan_bold("Speed Progress"),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .padding(Padding::horizontal(1));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let width = inner.width as usize;
    let mut lines: Vec<Line> = vec![];

    // Header: Path(10) Status(16) CDN Node(22) Ping/TCP(16) Download(14) Upload(10)
    let hdr = format!(
        "{:<10}  {:<16}  {:<22}  {:>16}  {:>14}  {:>10}",
        "Path", "Status", "CDN Node", "Ping/TCP", "Download", "Upload"
    );
    lines.push(Line::from(vec![dim(hdr)]));
    lines.push(Line::from(vec![dim("─".repeat(width.min(98)))]));

    for row in &state.paths {
        let (status_span, pid_col) = if row.done && row.current_stage == "merged" {
            (
                dim(format!("{:<16}", "→ merged")),
                dim(format!("{:<10}", row.path_id)),
            )
        } else if row.done && row.error.is_some() {
            (
                Span::styled(format!("{:<16}", "✗ failed"), Style::default().fg(Color::Red)),
                red(format!("{:<10}", row.path_id)),
            )
        } else if row.done {
            (
                Span::styled(format!("{:<16}", "✓ done"), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                green_bold(format!("{:<10}", row.path_id)),
            )
        } else {
            let stage_s = format!("{} {}", spin(tick), row.current_stage);
            (
                Span::styled(format!("{:<16}", stage_s), Style::default().fg(Color::Cyan)),
                cyan_bold(format!("{:<10}", row.path_id)),
            )
        };

        // CDN Node: IP(max15) + abbreviated city
        let cdn_raw = match (&row.cdn_ip, &row.cdn_location) {
            (Some(ip), Some(loc)) => format!("{} {}", ip, shorten_geo(loc)),
            (Some(ip), None)      => ip.clone(),
            _ => if row.done && row.error.is_some() {
                row.error.as_deref().unwrap_or("err").chars().take(22).collect()
            } else {
                "...".to_string()
            }
        };
        let cdn_truncated: String = cdn_raw.chars().take(22).collect();
        let cdn_span = if row.cdn_ip.is_some() {
            Span::styled(format!("{:<22}", cdn_truncated), Style::default().fg(Color::White))
        } else if row.done && row.error.is_some() {
            Span::styled(format!("{:<22}", cdn_truncated), Style::default().fg(Color::Red))
        } else {
            dim(format!("{:<22}", cdn_truncated))
        };

        // Ping/TCP column (16 chars): show HTTP RTT and TCP RTT
        let ping_span = match (row.rtt_ms, row.tcp_rtt_ms) {
            (Some(http), Some(tcp)) => Span::styled(
                format!("{:>6.1}ms/{:>5.1}ms", http, tcp),
                Style::default().fg(rtt_color(http)).add_modifier(Modifier::BOLD),
            ),
            (Some(http), None)      => rtt_span(format!("{:>6.2}ms {:>8}", http, ""), http),
            (None, Some(tcp))       => Span::styled(
                format!("{:>8} tcp{:>4.1}ms", "", tcp),
                Style::default().fg(rtt_color(tcp)),
            ),
            (None, None)            => dim(format!("{:>16}", if row.done { "-" } else { "" })),
        };

        // Download
        let dl_span = if let Some(mbps) = row.dl_mbps {
            let bar = speed_bar(mbps, 1000.0, 4);
            Span::styled(
                format!("{bar} {:>5.1}Mbps", mbps),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )
        } else {
            let is_dl = row.current_stage.contains("download");
            if !row.done && is_dl {
                Span::styled(format!("{:<14}", format!("{} ...", spin(tick))), Style::default().fg(Color::Cyan))
            } else {
                dim(format!("{:>14}", if row.done { if row.error.is_some() { "✗" } else { "-" } } else { "" }))
            }
        };

        // Upload
        let ul_span = if let Some(mbps) = row.ul_mbps {
            Span::styled(format!("{:>4.1}Mbps", mbps), Style::default().fg(Color::White))
        } else {
            let is_ul = row.current_stage.contains("upload");
            if !row.done && is_ul {
                Span::styled(format!("{:<10}", format!("{} ...", spin(tick))), Style::default().fg(Color::Cyan))
            } else {
                dim(format!("{:>10}", if row.done { if row.error.is_some() { "✗" } else { "-" } } else { "" }))
            }
        };

        lines.push(Line::from(vec![
            pid_col, dim("  "), status_span, dim("  "),
            cdn_span, dim("  "), ping_span, dim("  "),
            dl_span, dim("  "), ul_span,
        ]));
    }

    // Overall progress bar
    let total = state.paths.len();
    let done  = state.paths.iter().filter(|r| r.done).count();
    if total > 0 {
        lines.push(Line::from(vec![]));
        let bar_filled = (done * 20) / total;
        let bar = format!(
            "[{}{}]  {done}/{total} paths done",
            "█".repeat(bar_filled),
            "░".repeat(20 - bar_filled),
        );
        lines.push(Line::from(vec![
            Span::styled(bar, Style::default().fg(if done == total { Color::Green } else { Color::Cyan })),
        ]));
    }

    f.render_widget(Paragraph::new(lines), inner);
}


// ── Footer ───────────────────────────────────────────────────────

/// Derive (stage_label, done_steps, total_steps) from live AppState.
///
/// Stage sequence (multi-path full mode):
///   egress(1) → resolve+geo(2) → [paths: each = 1 unit] → connectivity probe(+1) → done
/// Other multi-path subcommands (ping/download/upload):
///   egress(1) → resolve+geo(2) → [paths: each = 1 unit]
/// Single-path (cloudflare):
///   egress(1) → resolve(2) → ping(3) → download(4) → upload(5)
fn running_progress(state: &AppState) -> (&'static str, usize, usize) {
    // Multi-path mode
    if !state.paths.is_empty() {
        let path_total  = state.paths.len();
        let paths_done  = state.paths.iter().filter(|r| r.done).count();
        let is_full     = state.mode == "full";
        // full mode has an extra connectivity probe step after all paths complete
        let total = 2 + path_total + if is_full { 1 } else { 0 };
        let pre_done = if state.egress_done { 1 } else { 0 }
            + if state.resolved_ip.is_some() || state.egress_done { 1 } else { 0 };
        let probe_done = if !state.probe_results.is_empty() { 1 }
            else if state.probe_progress.is_none() && paths_done == path_total && is_full { 0 }
            else { 0 };
        let done = pre_done.min(2) + paths_done + probe_done;

        let label: &'static str = if !state.egress_done {
            "detecting egress"
        } else if paths_done < path_total {
            "speed test"
        } else if is_full && state.probe_results.is_empty() {
            "connectivity probe"
        } else {
            "finishing"
        };
        return (label, done.min(total), total);
    }
    // Single-path mode (cloudflare / simple)
    use crate::tui::state::StageStatus;
    let total = 5usize;
    let egress  = if state.egress_done { 1 } else { 0 };
    let resolve = if state.resolved_ip.is_some() { 1 } else { 0 };
    let ping    = if state.ping_status.is_done() { 1 } else { 0 };
    let dl      = if state.download_status.is_done() { 1 } else { 0 };
    let ul      = if state.upload_status.is_done() { 1 } else { 0 };
    let done = egress + resolve + ping + dl + ul;
    let label: &'static str = match &state.ping_status {
        StageStatus::Running => "latency",
        _ => match &state.download_status {
            StageStatus::Running => "download",
            _ => match &state.upload_status {
                StageStatus::Running => "upload",
                _ => if state.egress_done { "speed test" } else { "detecting egress" },
            }
        }
    };
    (label, done, total)
}

fn draw_footer(f: &mut Frame, area: Rect, state: &AppState) {
    if state.finished {
        let code = state.exit_code;
        let (col, text) = if code == 0 {
            (Color::Green, "✓  done".to_string())
        } else {
            (Color::Yellow, format!("⚠  exit {code}"))
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(col));
        let para = Paragraph::new(Line::from(vec![
            Span::styled(format!(" {text}   press q / Q / Esc to quit"), Style::default().fg(col).add_modifier(Modifier::BOLD)),
        ])).block(block);
        f.render_widget(para, area);
        return;
    }

    // Compute current stage label + (done, total) from AppState
    let (stage_label, done_steps, total_steps) = running_progress(state);
    let bar_w: usize = 16;
    let filled = if total_steps > 0 { (done_steps * bar_w) / total_steps } else { 0 };
    let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(bar_w - filled));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray));
    let para = Paragraph::new(Line::from(vec![
        Span::raw(" "),
        Span::styled(spin(state.tick).to_string(), Style::default().fg(Color::Cyan)),
        Span::raw("  "),
        Span::styled(bar, Style::default().fg(Color::Cyan)),
        dim(format!("  {done_steps}/{total_steps}  {stage_label}   q to quit")),
    ])).block(block);
    f.render_widget(para, area);
}


// ═══════════════════════════════════════════════════════════════
// Result page: draw_result_table()
// Layout: Header(dynamic) + Speed Results(full-width) + Footer(3)
// ═══════════════════════════════════════════════════════════════

pub fn draw_result_table(f: &mut Frame, state: &AppState) {
    let Some(report) = &state.final_report else {
        return draw(f, state);
    };

    let area = f.area();
    let header_h = header_height(state);
    let has_probe = !state.probe_results.is_empty();
    let probing   = state.probe_progress.is_some() && !has_probe;

    // When routing probe results are present: upper half Speed Results, lower half Connectivity
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(header_h), Constraint::Min(0), Constraint::Length(3)])
        .margin(1)
        .split(area);

    draw_header(f, outer[0], state);

    if has_probe {
        // Both panels use Min so they each grow when the terminal is taller.
        // speed_result_height() is the *minimum* needed; extra space is shared between panels.
        let speed_h = speed_result_height(report);
        let body_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(speed_h), Constraint::Min(4)])
            .split(outer[1]);
        draw_result_body(f, body_chunks[0], state, report);
        draw_probe_panel(f, body_chunks[1], state, &state.probe_results);
    } else if probing {
        let speed_h = speed_result_height(report);
        let body_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(speed_h), Constraint::Min(3)])
            .split(outer[1]);
        draw_result_body(f, body_chunks[0], state, report);
        draw_probe_progress_panel(f, body_chunks[1], state);
    } else {
        draw_result_body(f, outer[1], state, report);
    }

    // Footer
    let code = state.exit_code;
    let (ec_col, ec_text) = if code == 0 {
        (Color::Green,  format!("✓  done  Exit {code}   Tab: switch panel  ↑↓/jk: scroll  q: quit"))
    } else {
        (Color::Yellow, format!("⚠  partial  Exit {code}   Tab: switch panel  ↑↓/jk: scroll  q: quit"))
    };
    let footer_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ec_col));
    let footer_para = Paragraph::new(Line::from(vec![
        Span::styled(format!(" {ec_text}"), Style::default().fg(ec_col).add_modifier(Modifier::BOLD)),
    ])).block(footer_block);
    f.render_widget(footer_para, outer[2]);
}

fn draw_result_body(f: &mut Frame, area: Rect, state: &AppState, report: &crate::report::Report) {
    let inner_h = area.height.saturating_sub(2) as usize; // subtract borders

    // Build lines first so we know total count before rendering the block
    let mut lines: Vec<Line> = vec![];

    // ── Multipath results ──
    if !report.paths.is_empty() {
        // Region row
        let region_s = report.resolver_country.as_deref()
            .map(|cc| if cc == "CN" { format!("{cc} (Mainland CN)") } else { cc.to_string() })
            .unwrap_or_default();
        let has_region = !region_s.is_empty();
        if has_region {
            lines.push(Line::from(vec![
                dim(format!("{:<10}", "Region")),
                Span::styled(region_s, Style::default().fg(Color::White)),
            ]));
        }
        // Split routing warning
        let split_routing = report.egress_consistent == Some(false);
        if split_routing {
            lines.push(Line::from(vec![
                Span::styled(
                    "  ⚠  CN ≠ Global egress detected — split routing active",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        if has_region || split_routing {
            lines.push(Line::from(vec![]));
        }

        // Header: Path(9) CDN IP(16) Location(24) Ping(8) TCP(8) Download(11) Upload(11)
        lines.push(Line::from(vec![dim(format!(
            "{:<9}  {:<16}  {:<24}  {:>8}  {:>7}  {:>11}  {:>11}",
            "Path", "CDN IP", "Location", "HTTP-RTT", "TCP-RTT", "Download", "Upload"
        ))]));
        lines.push(Line::from(vec![dim("─".repeat(95))]));

        for path in &report.paths {
            let cdn_ip_s: String = path.cdn_ip.as_deref().unwrap_or("-").chars().take(16).collect();
            let location_s: String = path.cdn_location.as_deref()
                .map(shorten_geo)
                .unwrap_or_else(|| "-".to_string())
                .chars().take(24).collect();

            let dl_s = path.download_mbps
                .map(|v| format!("{v:>8.2}Mbps"))
                .unwrap_or_else(|| format!("{:>11}", if path.error.is_some() { "✗" } else { "-" }));

            let ul_s = path.upload_mbps
                .map(|v| format!("{v:>8.2}Mbps"))
                .unwrap_or_else(|| format!("{:>11}", if path.error.is_some() { "✗" } else { "-" }));

            let has_err = path.error.is_some();
            let (pid_span, ip_span, loc_span, ping_span, tcp_span, dl_span, ul_span) = if has_err {
                (red(format!("{:<9}", path.path_id)),
                 red(format!("{:<16}", cdn_ip_s)),
                 dim(format!("{:<24}", location_s)),
                 dim(format!("{:>8}", "-")),
                 dim(format!("{:>7}", "-")),
                 red(format!("{:>11}", "✗")),
                 red(format!("{:>11}", "✗")))
            } else {
                let ping_span = path.rtt_ms
                    .map(|v| rtt_span(format!("{v:>6.2}ms"), v))
                    .unwrap_or_else(|| dim(format!("{:>8}", "-")));
                let tcp_span = path.ping_stats.as_ref()
                    .and_then(|ps| ps.tcp_rtt_ms)
                    .map(|v| Span::styled(format!("{v:>5.1}ms"), Style::default().fg(rtt_color(v))))
                    .unwrap_or_else(|| dim(format!("{:>7}", "-")));
                (cyan_bold(format!("{:<9}", path.path_id)),
                 Span::styled(format!("{:<16}", cdn_ip_s), Style::default().fg(Color::White)),
                 green(format!("{:<24}", location_s)),
                 ping_span,
                 tcp_span,
                 Span::styled(dl_s.clone(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                 Span::styled(ul_s.clone(), Style::default().fg(Color::White)))
            };

            lines.push(Line::from(vec![
                pid_span, dim("  "), ip_span, dim("  "), loc_span,
                dim("  "), ping_span, dim("  "), tcp_span,
                dim("  "), dl_span, dim("  "), ul_span,
            ]));

            if !has_err {
                // HTTP-RTT sparkline + latency stats
                // Sub-row indent = 11 chars (path_id 9 + sep 2) to align under CDN IP column
                const INDENT: &str = "           "; // 11 spaces
                const LBL: usize = 10; // label width within sub-row
                if let Some(ps) = &path.ping_stats {
                    if ps.samples.len() >= 2 {
                        let spark = ping_sparkline(&ps.samples);
                        lines.push(Line::from(vec![
                            dim(format!("{}{:<LBL$}", INDENT, "HTTP-RTT")),
                            dim("(http)  "),
                            Span::styled(spark, Style::default().fg(Color::Cyan)),
                        ]));
                        lines.push(Line::from(vec![
                            dim(format!("{}{:<LBL$}", INDENT, "")),
                            dim(format!(
                                "min {:.1}ms  avg {:.1}ms  max {:.1}ms  p95 {:.1}ms  jitter {:.1}ms",
                                ps.min_ms, ps.avg_ms, ps.max_ms, ps.p95_ms, ps.jitter_ms
                            )),
                        ]));
                    } else if let Some(v) = path.rtt_ms {
                        lines.push(Line::from(vec![
                            dim(format!("{}{:<LBL$}", INDENT, "HTTP-RTT")),
                            dim(format!("(http)  {:.1}ms", v)),
                        ]));
                    }
                    if let Some(tcp) = ps.tcp_rtt_ms {
                        lines.push(Line::from(vec![
                            dim(format!("{}{:<LBL$}", INDENT, "TCP-RTT")),
                            dim(format!("(tcp-connect)  {:.1}ms", tcp)),
                        ]));
                    }
                }

                // Download stage bar chart
                if path.download_mbps.is_some() {
                    let valid: Vec<_> = path.download_stages.iter().filter(|s| s.mbps.is_some()).collect();
                    if valid.len() >= 2 {
                        let max_mbps = valid.iter().filter_map(|s| s.mbps).fold(0.0_f64, f64::max).max(1.0);
                        const BAR_W: usize = 14;
                        lines.push(Line::from(vec![
                            dim(format!("{}{:<LBL$}", INDENT, "Download")),
                            dim("stages (Mbps)"),
                        ]));
                        for s in &valid {
                            let mbps = s.mbps.unwrap_or(0.0);
                            let filled = ((mbps / max_mbps) * BAR_W as f64).round() as usize;
                            lines.push(Line::from(vec![
                                dim(format!("{}{:<LBL$}", INDENT, s.name)),
                                Span::styled("█".repeat(filled.min(BAR_W)), Style::default().fg(Color::Cyan)),
                                Span::styled("░".repeat(BAR_W - filled.min(BAR_W)), Style::default().fg(Color::DarkGray)),
                                dim(format!("  {:.1} Mbps", mbps)),
                            ]));
                        }
                        if let Some(ds) = &path.download_stats {
                            lines.push(Line::from(vec![
                                dim(format!("{}{:<LBL$}", INDENT, "stats")),
                                dim(format!(
                                    "avg {:.1}Mbps  max {:.1}Mbps  p90 {:.1}Mbps  stddev ±{:.1}Mbps",
                                    ds.avg_mbps, ds.max_mbps, ds.p90_mbps, ds.stddev_mbps
                                )),
                            ]));
                        }
                    }
                }
            }
            lines.push(Line::from(vec![dim("─".repeat(95))]));
        }
    }

    // ── Single-path results (Cloudflare backend) ──
    if report.paths.is_empty() {
        // resolver_country is derived from the client's egress IP — label it clearly
        let region_label = if report.target_host.contains("cloudflare") { "Your Region" } else { "Region" };
        let region_s = report.resolver_country.as_deref()
            .map(|cc| if cc == "CN" { format!("{cc} (Mainland CN)") } else { cc.to_string() });
        if let Some(r) = region_s {
            lines.push(Line::from(vec![dim(format!("{:<12}", region_label)), color(r, Color::White)]));
        }
        if let Some(ip) = &report.selected_ip {
            let family = report.selected_family.as_deref().unwrap_or("-");
            let loc = report.selected_location.as_deref().map(shorten_geo).unwrap_or_default();
            lines.push(Line::from(vec![
                dim(format!("{:<10}", "CDN Node")),
                white_bold(ip.clone()),
                dim(format!("  {family}  ")),
                if !loc.is_empty() { green(loc) } else { Span::raw("") },
            ]));
        }
        lines.push(Line::from(vec![]));

        let metric_row = |ok: bool, label: &str, value: String| -> Line<'static> {
            let (icon, val_span) = if ok {
                (Span::styled("✓", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                 Span::styled(value, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)))
            } else {
                (Span::styled("✗", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                 Span::styled(value, Style::default().fg(Color::Red)))
            };
            Line::from(vec![icon, Span::raw("  "), dim(format!("{label:<12}")), val_span])
        };

        if report.mode == "ping" || report.mode == "full" {
            let latency_line = if let Some(v) = report.rtt_ms {
                Line::from(vec![
                    Span::styled("✓", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    Span::raw("  "),
                    dim(format!("{:<12}", "Latency")),
                    rtt_span(format!("{v:.2} ms"), v),
                ])
            } else {
                metric_row(false, "Latency", "failed".to_string())
            };
            lines.push(latency_line);
            if let Some(ps) = &report.ping_stats {
                if ps.samples.len() >= 2 {
                    let spark = ping_sparkline(&ps.samples);
                    lines.push(Line::from(vec![
                        dim("   Trend "),
                        Span::styled(spark, Style::default().fg(Color::Cyan)),
                    ]));
                    let tcp_info = ps.tcp_rtt_ms.map(|v| format!("  tcp {v:.1}ms")).unwrap_or_default();
                    lines.push(Line::from(vec![
                        dim(format!("   min {:.1}  avg {:.1}  med {:.1}  max {:.1}  jitter {:.1} ms{tcp_info}",
                            ps.min_ms, ps.avg_ms, ps.median_ms, ps.max_ms, ps.jitter_ms)),
                    ]));
                }
            }
            lines.push(Line::raw(""));
        }

        if report.mode == "download" || report.mode == "full" {
            let (ok, val) = if let Some(v) = report.download_mbps {
                (true, format!("{v:.2} Mbps"))
            } else {
                (false, "failed".to_string())
            };
            lines.push(metric_row(ok, "Download", val));
            let valid_stages: Vec<_> = report.download_stages.iter().filter(|s| s.mbps.is_some()).collect();
            if valid_stages.len() >= 2 {
                let max_mbps = valid_stages.iter().filter_map(|s| s.mbps).fold(0.0_f64, f64::max).max(1.0);
                const BAR_W: usize = 20;
                for s in &valid_stages {
                    let mbps = s.mbps.unwrap_or(0.0);
                    let filled = ((mbps / max_mbps) * BAR_W as f64).round() as usize;
                    lines.push(Line::from(vec![
                        dim(format!("   {:<8}", s.name)),
                        Span::styled("█".repeat(filled.min(BAR_W)), Style::default().fg(Color::Cyan)),
                        Span::styled("░".repeat(BAR_W - filled.min(BAR_W)), Style::default().fg(Color::DarkGray)),
                        Span::styled(format!(" {:.1} Mbps", mbps), Style::default().fg(Color::White)),
                    ]));
                }
            }
            if let Some(ds) = &report.download_stats {
                lines.push(Line::from(vec![
                    dim(format!("   avg {:.1}  max {:.1}  p90 {:.1}  ±{:.1} Mbps  ({:.2} MB/s)",
                        ds.avg_mbps, ds.max_mbps, ds.p90_mbps, ds.stddev_mbps, ds.avg_mbs)),
                ]));
            }
            lines.push(Line::raw(""));
        }

        if report.mode == "upload" || report.mode == "full" {
            let (ok, val) = if let Some(v) = report.upload_mbps {
                (true, format!("{v:.2} Mbps"))
            } else {
                (false, "failed".to_string())
            };
            lines.push(metric_row(ok, "Upload", val));
            if let Some(us) = &report.upload_stats {
                lines.push(Line::from(vec![
                    dim(format!("   avg {:.1}  max {:.1} Mbps  ({:.2} MB/s)",
                        us.avg_mbps, us.max_mbps, us.avg_mbs)),
                ]));
            }
        }
    }

    // Build block — focus border + scroll hint in bottom-right corner
    let total_lines = lines.len();
    let is_focused  = state.result_focus == crate::tui::state::ResultFocus::Speed;

    // Clamp scroll to valid range
    let max_scroll  = total_lines.saturating_sub(inner_h) as u16;
    let offset      = state.scroll_speed.min(max_scroll);
    let has_above   = offset > 0;
    let has_below   = (offset as usize) + inner_h < total_lines;

    let scroll_hint = match (has_above, has_below) {
        (true,  true)  => Some(" ↑↓ more "),
        (true,  false) => Some(" ↑ top    "),
        (false, true)  => Some(" ↓ more   "),
        (false, false) => None,
    };

    let border_col = if is_focused { Color::Cyan } else { Color::DarkGray };
    let mut block = Block::default()
        .title(Line::from(vec![Span::raw(" "), cyan_bold("Speed Results"), Span::raw(" ")]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_col))
        .padding(Padding::horizontal(1));

    if let Some(hint) = scroll_hint {
        block = block.title_bottom(
            Line::from(Span::styled(hint, Style::default().fg(Color::DarkGray)))
                .right_aligned()
        );
    }

    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(Paragraph::new(lines).scroll((offset, 0)), inner);
}

// ── Compute height of speed results panel ─────────────────────

fn speed_result_height(report: &crate::report::Report) -> u16 {
    if !report.paths.is_empty() {
        // border(2) + region(2) + header+sep(2) + per-path rows
        let mut rows: u16 = 6;
        for path in &report.paths {
            rows += 1; // main data row
            if path.error.is_none() {
                if let Some(ps) = &path.ping_stats {
                    if ps.samples.len() >= 2 { rows += 1; }
                }
                if path.download_mbps.is_some() {
                    let valid = path.download_stages.iter().filter(|s| s.mbps.is_some()).count();
                    if valid >= 2 {
                        rows += valid as u16;
                        if path.download_stats.is_some() { rows += 1; }
                    }
                }
            }
            rows += 1; // separator
        }
        rows.min(40)
    } else {
        // Single-path: headers + stages + optional details
        let mut rows: u16 = 6;
        if report.rtt_ms.is_some() {
            if let Some(ps) = &report.ping_stats {
                if ps.samples.len() >= 2 { rows += 2; }
            }
            rows += 1;
        }
        if report.download_mbps.is_some() {
            let valid = report.download_stages.iter().filter(|s| s.mbps.is_some()).count();
            if valid >= 2 { rows += valid as u16; }
            if report.download_stats.is_some() { rows += 1; }
            rows += 1;
        }
        if report.upload_mbps.is_some() {
            if report.upload_stats.is_some() { rows += 1; }
        }
        (rows + 4).min(30)
    }
}

// ── Connectivity probe in-progress panel ─────────────────────────────────────

fn draw_probe_progress_panel(f: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .title(Line::from(vec![Span::raw(" "), cyan_bold("Connectivity"), dim("  probing..."), Span::raw(" ")]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .padding(Padding::horizontal(1));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let (done, total) = state.probe_progress.unwrap_or((0, 0));
    let bar_w = 24_usize;
    let filled = if total > 0 { (done * bar_w) / total } else { 0 };
    let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(bar_w - filled));
    let lines = vec![
        Line::from(vec![
            Span::styled(spin(state.tick).to_string(), Style::default().fg(Color::Cyan)),
            dim("  probing connectivity..."),
        ]),
        Line::from(vec![
            Span::styled(bar, Style::default().fg(Color::Cyan)),
            dim(format!("  {done}/{total}")),
        ]),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}

// ── Connectivity probe results panel ─────────────────────────

const PROBE_CATEGORY_ORDER: &[&str] = &[
    "ai", "social", "streaming", "search", "news",
    "game", "dev", "cloud", "crypto", "nsfw", "cn",
];

fn probe_category_label(cat: &str) -> &str {
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

fn draw_probe_panel(
    f: &mut Frame,
    area: Rect,
    state: &AppState,
    results: &[crate::probe::types::ProbeResult],
) {
    let inner_h = area.height.saturating_sub(2) as usize;
    let is_focused = state.result_focus == crate::tui::state::ResultFocus::Connectivity;

    let inner_w   = area.width.saturating_sub(4) as usize; // approximate inner width
    let total     = results.len();
    let reachable = results.iter().filter(|r| r.reachable).count();
    let blocked   = total - reachable;

    const CAT_W: usize = 10;
    const GAP: usize = 2;

    let mut lines: Vec<Line> = vec![];

    // Summary row
    lines.push(Line::from(vec![
        dim(format!("{total} sites  ")),
        Span::styled(format!("✓{reachable} ok"), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        dim("  "),
        if blocked > 0 {
            Span::styled(
                format!("✗{blocked} blocked"),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled("all reachable", Style::default().fg(Color::DarkGray))
        },
    ]));

    // Per-category word-wrap layout.
    // Each entry: ●/○ + space + name + optional space + ms + GAP
    for &cat in PROBE_CATEGORY_ORDER {
        let group: Vec<&crate::probe::types::ProbeResult> =
            results.iter().filter(|r| r.category == cat).collect();
        if group.is_empty() { continue; }

        let label   = probe_category_label(cat);
        let cat_pad = format!("{:<CAT_W$}", label);
        let indent  = format!("{:<CAT_W$}", "");

        // First line starts with the bold category label
        let mut cur_spans: Vec<Span> = vec![
            Span::styled(
                cat_pad.clone(),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
        ];
        let mut cur_w: usize = CAT_W;

        for r in &group {
            let name = r.name.as_str();
            // Country code: prefer loc (from /cdn-cgi/trace), then geo.country_code
            let cc: Option<String> = r.loc.clone()
                .filter(|s| !s.is_empty())
                .or_else(|| r.geo.as_ref().and_then(|g| g.country_code.clone()));

            // ● (U+25CF filled) = reachable, ○ (U+25CB open) = blocked
            let (circle, ms_str, circle_sty, name_sty, cc_sty) = if r.reachable {
                let ms = r.ttfb_ms.map(|v| format!("{:.0}", v)).unwrap_or_default();
                (
                    "●",
                    ms,
                    Style::default().fg(Color::Green),
                    Style::default().fg(Color::Green),
                    Style::default().fg(Color::Green).add_modifier(Modifier::DIM),
                )
            } else {
                (
                    "○",
                    String::new(),
                    Style::default().fg(Color::Red).add_modifier(Modifier::DIM),
                    Style::default().fg(Color::Red).add_modifier(Modifier::DIM),
                    Style::default().fg(Color::Red).add_modifier(Modifier::DIM),
                )
            };
            // Visual width: circle(1) + space(1) + name + optional(space+cc) + optional(space+ms) + GAP
            let cc_part = cc.as_deref().unwrap_or("");
            let entry_w = 2 + name.chars().count()
                + if cc_part.is_empty() { 0 } else { 1 + cc_part.len() }
                + if ms_str.is_empty() { 0 } else { 1 + ms_str.len() }
                + GAP;

            // Wrap when overflow (but never before first entry on a line)
            if cur_w + entry_w > inner_w && cur_w > CAT_W {
                lines.push(Line::from(cur_spans));
                cur_spans = vec![dim(indent.clone())];
                cur_w = CAT_W;
            }

            cur_spans.push(Span::styled(circle.to_string(), circle_sty));
            cur_spans.push(dim(" "));
            cur_spans.push(Span::styled(name.to_string(), name_sty));
            if !cc_part.is_empty() {
                cur_spans.push(dim(" "));
                cur_spans.push(Span::styled(cc_part.to_string(), cc_sty));
            }
            if !ms_str.is_empty() {
                // Color ms by actual latency threshold
                let ms_col = r.ttfb_ms.map(rtt_color).unwrap_or(Color::DarkGray);
                cur_spans.push(dim(" "));
                cur_spans.push(Span::styled(ms_str.clone(), Style::default().fg(ms_col)));
            }
            cur_spans.push(dim("  "));
            cur_w += entry_w;
        }
        if cur_w > CAT_W {
            lines.push(Line::from(cur_spans));
        }
    }

    // Clamp scroll + build block with focus border and scroll hint
    let total_lines = lines.len();
    let max_scroll  = total_lines.saturating_sub(inner_h) as u16;
    let offset      = state.scroll_conn.min(max_scroll);
    let has_above   = offset > 0;
    let has_below   = (offset as usize) + inner_h < total_lines;

    let scroll_hint = match (has_above, has_below) {
        (true,  true)  => Some(" ↑↓ more "),
        (true,  false) => Some(" ↑ top    "),
        (false, true)  => Some(" ↓ more   "),
        (false, false) => None,
    };

    let border_col = if is_focused { Color::Cyan } else { Color::DarkGray };
    let mut block = Block::default()
        .title(Line::from(vec![
            Span::raw(" "),
            cyan_bold("Connectivity"),
            dim("  ● ok  ○ blocked  ms = TTFB  country from cdn-cgi/trace or GeoIP"),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_col))
        .padding(Padding::horizontal(1));

    if let Some(hint) = scroll_hint {
        block = block.title_bottom(
            Line::from(Span::styled(hint, Style::default().fg(Color::DarkGray)))
                .right_aligned()
        );
    }

    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(Paragraph::new(lines).scroll((offset, 0)), inner);
}



