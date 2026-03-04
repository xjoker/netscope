use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Padding, Paragraph},
};

use crate::tui::state::AppState;

/// Theme accent color — 256-color teal (#008787), legible on both light and dark backgrounds.
const ACCENT: Color = Color::Indexed(30);

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn spin(tick: u64) -> &'static str {
    SPINNER[(tick as usize) % SPINNER.len()]
}


fn cyan_bold(s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
}
fn white_bold(s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), Style::default().fg(Color::Reset).add_modifier(Modifier::BOLD))
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

fn scroll_hint_text(has_above: bool, has_below: bool) -> Option<&'static str> {
    match (has_above, has_below) {
        (true,  true)  => Some(" ↑↓ more "),
        (true,  false) => Some(" ↑ top    "),
        (false, true)  => Some(" ↓ more   "),
        (false, false) => None,
    }
}

/// 公共滚动面板渲染：计算 scroll clamp → 构建 Block → 渲染 Paragraph
/// 返回 clamped offset 供调用方回写到 state
fn render_scrollable_panel(
    f: &mut Frame,
    area: Rect,
    title: Line<'static>,
    lines: Vec<Line<'static>>,
    scroll_offset: u16,
    is_focused: bool,
) -> u16 {
    let inner_h = area.height.saturating_sub(2) as usize;
    let total_lines = lines.len();
    let max_scroll = total_lines.saturating_sub(inner_h) as u16;
    let offset = scroll_offset.min(max_scroll);
    let has_above = offset > 0;
    let has_below = (offset as usize) + inner_h < total_lines;

    let scroll_hint = scroll_hint_text(has_above, has_below);

    let border_col = if is_focused { ACCENT } else { Color::DarkGray };
    let mut block = Block::default()
        .title(title)
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
    offset
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
// Header: adaptive horizontal layout — wraps to 1~3 lines based on terminal width
// ═══════════════════════════════════════════════════════════════

/// Compute the visible character width of a Vec<Span>.
fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.content.chars().count()).sum()
}

/// Build the three logical segments of the header bar.
/// Returns (title_spans, v4_spans, v6_spans).
fn build_header_segments(state: &AppState) -> (Vec<Span<'static>>, Vec<Span<'static>>, Vec<Span<'static>>) {
    let ver = env!("APP_VERSION");

    let backend_upper = state.backend.to_uppercase();
    let (badge_text, badge_col) = if state.backend == "cloudflare" {
        (format!(" {backend_upper} "), Color::Yellow)
    } else {
        (format!(" {backend_upper} "), ACCENT)
    };

    let cn_badge: Option<(&str, Color)> = match state.cn_mode {
        Some(true)  => Some((" CN Mode ", Color::LightRed)),
        Some(false) => Some((" Global  ", Color::Indexed(67))),
        None        => None,
    };

    // ── Title segment ──
    let mut title: Vec<Span> = vec![
        white_bold("netscope"),
        dim(format!("  {ver}  ")),
        Span::styled(badge_text, Style::default().fg(Color::Black).bg(badge_col).add_modifier(Modifier::BOLD)),
    ];
    if let Some((txt, col)) = cn_badge {
        title.push(Span::raw(" "));
        title.push(Span::styled(txt.to_string(), Style::default().fg(Color::Black).bg(col).add_modifier(Modifier::BOLD)));
    }
    title.push(dim(format!("  mode {}", state.mode)));
    if let Some(ref p) = state.proxy {
        title.push(dim(format!("  proxy {p}")));
    }

    // ── v4 segment ──
    let mut v4: Vec<Span> = Vec::new();
    if !state.egress_done {
        v4.push(dim("  "));
        v4.push(Span::styled(spin(state.tick).to_string(), Style::default().fg(ACCENT)));
        v4.push(dim(" detecting..."));
    } else {
        egress_inline_spans(&mut v4, "v4",
            state.egress_v4_cn.as_deref(), state.egress_v4_global.as_deref(),
            state.egress_v4_cn_geo.as_deref(), state.egress_v4_global_geo.as_deref());
    }

    // ── v6 segment ──
    let mut v6: Vec<Span> = Vec::new();
    if state.egress_done && (state.egress_v6_cn.is_some() || state.egress_v6_global.is_some()) {
        egress_inline_spans(&mut v6, "v6",
            state.egress_v6_cn.as_deref(), state.egress_v6_global.as_deref(),
            state.egress_v6_cn_geo.as_deref(), state.egress_v6_global_geo.as_deref());
    }

    (title, v4, v6)
}

/// Compute the number of header lines needed for the given terminal width.
fn header_lines(state: &AppState, width: u16) -> u16 {
    let (title, v4, v6) = build_header_segments(state);
    let w = width as usize;
    let tw = spans_width(&title);
    let v4w = spans_width(&v4);
    let v6w = spans_width(&v6);

    if v6w == 0 {
        // No v6: title + v4 on one line, or split to 2
        if tw + v4w <= w { 1 } else { 2 }
    } else if tw + v4w + v6w <= w {
        1
    } else if tw + v4w <= w {
        // title+v4 fit on line 1, v6 on line 2
        2
    } else {
        // Each segment on its own line: title on line 1, v4+v6 merged on line 2
        2
    }
}

fn draw_header(f: &mut Frame, area: Rect, state: &AppState) {
    let (title, v4, v6) = build_header_segments(state);
    let w = area.width as usize;
    let tw = spans_width(&title);
    let v4w = spans_width(&v4);
    let v6w = spans_width(&v6);

    let mut lines: Vec<Line> = Vec::new();

    if v6w == 0 {
        if tw + v4w <= w {
            // All on one line
            let mut all = title;
            all.extend(v4);
            lines.push(Line::from(all));
        } else {
            lines.push(Line::from(title));
            lines.push(Line::from(v4));
        }
    } else if tw + v4w + v6w <= w {
        // All on one line
        let mut all = title;
        all.extend(v4);
        all.extend(v6);
        lines.push(Line::from(all));
    } else if tw + v4w <= w {
        // title+v4 on line 1, v6 on line 2
        let mut line1 = title;
        line1.extend(v4);
        lines.push(Line::from(line1));
        lines.push(Line::from(v6));
    } else {
        // Each on its own line
        lines.push(Line::from(title));
        let mut ip_line = v4;
        ip_line.extend(v6);
        lines.push(Line::from(ip_line));
    }

    f.render_widget(Paragraph::new(lines), area);
}

/// Append egress IP info as inline spans for a single IP family.
fn egress_inline_spans(
    spans: &mut Vec<Span<'static>>,
    label: &'static str,
    cn: Option<&str>,
    gl: Option<&str>,
    cn_geo: Option<&str>,
    gl_geo: Option<&str>,
) {
    let mismatch = cn.is_some() && gl.is_some() && cn != gl;
    spans.push(dim(format!("  {label} ")));

    if mismatch {
        let cn_s = cn.unwrap_or("-");
        let gl_s = gl.unwrap_or("-");
        spans.push(dim("CN "));
        spans.push(yellow(cn_s.to_string()));
        if let Some(g) = cn_geo { spans.push(dim(format!(" {}", shorten_geo(g)))); }
        spans.push(dim(" GL "));
        spans.push(yellow(gl_s.to_string()));
        if let Some(g) = gl_geo { spans.push(dim(format!(" {}", shorten_geo(g)))); }
        spans.push(yellow(" ⚠".to_string()));
    } else {
        let ip_s = cn.or(gl).unwrap_or("-");
        if cn.is_some() {
            spans.push(green(ip_s.to_string()));
        } else {
            spans.push(dim(ip_s.to_string()));
        }
        let geo_s = cn_geo.or(gl_geo).map(shorten_geo).unwrap_or_default();
        if !geo_s.is_empty() {
            spans.push(dim(format!(" {geo_s}")));
        }
    }
}

/// Simplify a geo string: keep only country/city + the last parenthesised segment (ISP)
/// e.g. "China/Fujian/Fuzhou (AS4134 | Chinanet)" → "Fuzhou (Chinanet)"
fn shorten_geo(geo: &str) -> String {
    // Extract parenthesised content (ISP)
    let isp = if let (Some(l), Some(r)) = (geo.find('('), geo.rfind(')')) {
        if l < r {
            let inner = &geo[l+1..r];
            // If " | " is present, take the second half (provider name)
            if let Some(pos) = inner.find(" | ") {
                inner[pos+3..].trim().to_string()
            } else {
                inner.trim().to_string()
            }
        } else {
            String::new()
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
// Unified view: draw_unified()
// Header(dynamic height) / Panel area(Min) / Footer(3)
// Panel area: Speed Panel (progress or results) + Connectivity Panel (full mode)
// ═══════════════════════════════════════════════════════════════

pub fn draw_unified(f: &mut Frame, state: &AppState) {
    let area = f.area();

    // Header: single horizontal line (no block borders)
    let footer_h: u16 = 3;
    let header_h = header_lines(state, area.width.saturating_sub(2)); // subtract margin

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_h),
            Constraint::Min(0),
            Constraint::Length(footer_h),
        ])
        .margin(1)
        .split(area);

    draw_header(f, chunks[0], state);

    // Panel area: full mode → split 50/50 when Connectivity has content
    let has_conn_content = !state.probe_results.is_empty()
        || !state.partial_probe_results.is_empty()
        || state.probe_progress.is_some();
    if state.mode == "full" && has_conn_content {
        let body_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1]);
        draw_speed_panel(f, body_chunks[0], state);
        draw_connectivity_panel(f, body_chunks[1], state);
    } else {
        draw_speed_panel(f, chunks[1], state);
    }

    draw_unified_footer(f, chunks[2], state);
}

// ── Speed Panel: progress and results in the same "Speed Results" block ──

fn draw_speed_panel(f: &mut Frame, area: Rect, state: &AppState) {
    if state.speed_done {
        if let Some(report) = &state.final_report {
            draw_result_body(f, area, state, report);
            return;
        }
    }
    // Still in progress — render live data inside a "Speed Results" panel
    let is_focused = state.result_focus == crate::tui::state::ResultFocus::Speed;
    let tick = state.tick;

    let mut lines: Vec<Line> = vec![];

    // Multi-path mode: show progress table
    if !state.paths.is_empty() {
        let result_width = area.width.saturating_sub(4) as usize;

        // Header: Path(10) Status(16) CDN Node(auto) Ping/TCP(16) Download(14) Upload(10)
        let fixed_cols = 10 + 16 + 16 + 14 + 10 + 10;
        let available_cdn = result_width.saturating_sub(fixed_cols).max(8);
        let max_cdn_content = state.paths.iter().map(|r| {
            match (&r.cdn_ip, &r.cdn_location) {
                (Some(ip), Some(loc)) => ip.len() + 1 + shorten_geo(loc).chars().count(),
                (Some(ip), None) => ip.len(),
                _ if r.done && r.error.is_some() => r.error.as_deref().map(|e| e.chars().count().min(40)).unwrap_or(3),
                _ => 3,
            }
        }).max().unwrap_or(8).max(8);
        let cdn_w = max_cdn_content.min(available_cdn);

        let hdr = format!(
            "{:<10}  {:<16}  {:<cdn_w$}  {:>16}  {:>14}  {:>10}",
            "Path", "Status", "CDN Node", "Ping/TCP", "Download", "Upload"
        );
        lines.push(Line::from(vec![dim(hdr)]));
        lines.push(Line::from(vec![dim("─".repeat(result_width))]));

        for row in &state.paths {
            let (status_span, pid_col) = if row.done && row.current_stage == "merged" {
                (dim(format!("{:<16}", "→ merged")), dim(format!("{:<10}", row.path_id)))
            } else if row.done && row.error.is_some() {
                (Span::styled(format!("{:<16}", "✗ failed"), Style::default().fg(Color::Red)),
                 red(format!("{:<10}", row.path_id)))
            } else if row.done {
                (Span::styled(format!("{:<16}", "✓ done"), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                 green_bold(format!("{:<10}", row.path_id)))
            } else {
                let stage_s = format!("{} {}", spin(tick), row.current_stage);
                (Span::styled(format!("{:<16}", stage_s), Style::default().fg(ACCENT)),
                 cyan_bold(format!("{:<10}", row.path_id)))
            };

            let cdn_raw = match (&row.cdn_ip, &row.cdn_location) {
                (Some(ip), Some(loc)) => format!("{} {}", ip, shorten_geo(loc)),
                (Some(ip), None)      => ip.clone(),
                _ => if row.done && row.error.is_some() {
                    row.error.as_deref().unwrap_or("err").chars().take(cdn_w).collect()
                } else { "...".to_string() }
            };
            let cdn_truncated: String = cdn_raw.chars().take(cdn_w).collect();
            let cdn_span = if row.cdn_ip.is_some() {
                Span::styled(format!("{:<cdn_w$}", cdn_truncated), Style::default().fg(Color::Reset))
            } else if row.done && row.error.is_some() {
                Span::styled(format!("{:<cdn_w$}", cdn_truncated), Style::default().fg(Color::Red))
            } else {
                dim(format!("{:<cdn_w$}", cdn_truncated))
            };

            let ping_span = match (row.rtt_ms, row.tcp_rtt_ms) {
                (Some(http), Some(tcp)) => Span::styled(
                    format!("{:>6.1}ms/{:>5.1}ms", http, tcp),
                    Style::default().fg(rtt_color(http)).add_modifier(Modifier::BOLD),
                ),
                (Some(http), None) => rtt_span(format!("{:>6.2}ms {:>8}", http, ""), http),
                (None, Some(tcp)) => Span::styled(
                    format!("{:>8} tcp{:>4.1}ms", "", tcp),
                    Style::default().fg(rtt_color(tcp)),
                ),
                (None, None) => dim(format!("{:>16}", if row.done { "-" } else { "" })),
            };

            let dl_span = if let Some(mbps) = row.dl_mbps {
                let bar = speed_bar(mbps, 1000.0, 4);
                Span::styled(format!("{bar} {:>5.1}Mbps", mbps),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            } else {
                let is_dl = row.current_stage.contains("download");
                if !row.done && is_dl {
                    Span::styled(format!("{:<14}", format!("{} ...", spin(tick))), Style::default().fg(ACCENT))
                } else {
                    dim(format!("{:>14}", if row.done { if row.error.is_some() { "✗" } else { "-" } } else { "" }))
                }
            };

            let ul_span = if let Some(mbps) = row.ul_mbps {
                Span::styled(format!("{:>4.1}Mbps", mbps), Style::default().fg(Color::Reset))
            } else {
                let is_ul = row.current_stage.contains("upload");
                if !row.done && is_ul {
                    Span::styled(format!("{:<10}", format!("{} ...", spin(tick))), Style::default().fg(ACCENT))
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
        let done = state.paths.iter().filter(|r| r.done).count();
        if total > 0 {
            lines.push(Line::from(vec![]));
            let bar_filled = (done * 20) / total;
            let bar = format!(
                "[{}{}]  {done}/{total} paths done",
                "█".repeat(bar_filled),
                "░".repeat(20 - bar_filled),
            );
            lines.push(Line::from(vec![
                Span::styled(bar, Style::default().fg(if done == total { Color::Green } else { ACCENT })),
            ]));
        }
    } else {
        // Pre-test or single-path: show waiting/resolving status
        lines.push(Line::from(vec![
            Span::styled(spin(tick).to_string(), Style::default().fg(ACCENT)),
            dim("  preparing speed test..."),
        ]));
        if state.backend == "cloudflare" {
            lines.push(Line::from(vec![dim("  target: speed.cloudflare.com")]));
        } else if let Some(ip) = &state.resolved_ip {
            lines.push(Line::from(vec![dim("  CDN: "), white_bold(ip.clone())]));
        }
    }

    // Build block with same styling as result view
    let (stage_label, done_steps, total_steps) = running_progress(state);
    let title = Line::from(vec![
        Span::raw(" "),
        cyan_bold("Speed Results"),
        dim(format!("  {} {}/{}", stage_label, done_steps, total_steps)),
        Span::raw(" "),
    ]);
    render_scrollable_panel(f, area, title, lines, state.scroll_speed, is_focused);
}

// ── Connectivity Panel: routes between progress / results ──

fn draw_connectivity_panel(f: &mut Frame, area: Rect, state: &AppState) {
    if !state.probe_results.is_empty() {
        draw_probe_panel(f, area, state, &state.probe_results);
    } else if let Some((done, total)) = state.probe_progress {
        if total > 0 && done >= total && !state.partial_probe_results.is_empty() {
            // All probes finished via ProbePartial but ProbeDone not yet received
            draw_probe_panel(f, area, state, &state.partial_probe_results);
        } else {
            draw_probe_progress_panel(f, area, state);
        }
    } else if !state.partial_probe_results.is_empty() {
        // Fallback: partial results exist but no progress tracking — render what we have
        draw_probe_panel(f, area, state, &state.partial_probe_results);
    } else {
        // Empty fallback: render an empty bordered panel so the area isn't blank
        let is_focused = state.result_focus == crate::tui::state::ResultFocus::Connectivity;
        let border_col = if is_focused { ACCENT } else { Color::DarkGray };
        let block = Block::default()
            .title(Line::from(vec![
                Span::raw(" "),
                cyan_bold("Connectivity"),
                dim("  waiting..."),
                Span::raw(" "),
            ]))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_col));
        f.render_widget(block, area);
    }
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

fn draw_unified_footer(f: &mut Frame, area: Rect, state: &AppState) {
    if state.finished && !state.retesting_speed && !state.retesting_probe {
        let code = state.exit_code;
        let (col, text) = if code == 0 {
            (Color::Green, format!("✓  done  Exit {code}   Tab: switch panel  r: retest  ↑↓/jk: scroll  q: quit"))
        } else {
            (Color::Yellow, format!("⚠  partial  Exit {code}   Tab: switch panel  r: retest  ↑↓/jk: scroll  q: quit"))
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(col));
        let para = Paragraph::new(Line::from(vec![
            Span::styled(format!(" {text}"), Style::default().fg(col).add_modifier(Modifier::BOLD)),
        ])).block(block);
        f.render_widget(para, area);
        return;
    }

    if state.retesting_speed || state.retesting_probe {
        let label = match (state.retesting_speed, state.retesting_probe) {
            (true, true)   => "retesting speed + connectivity...",
            (true, false)  => "retesting speed...",
            (false, true)  => "retesting connectivity...",
            _              => "",
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(ACCENT));
        let para = Paragraph::new(Line::from(vec![
            Span::raw(" "),
            Span::styled(spin(state.tick).to_string(), Style::default().fg(ACCENT)),
            dim(format!("  {label}   q to quit")),
        ])).block(block);
        f.render_widget(para, area);
        return;
    }

    // Running progress
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
        Span::styled(spin(state.tick).to_string(), Style::default().fg(ACCENT)),
        Span::raw("  "),
        Span::styled(bar, Style::default().fg(ACCENT)),
        dim(format!("  {done_steps}/{total_steps}  {stage_label}   q to quit")),
    ])).block(block);
    f.render_widget(para, area);
}


// ═══════════════════════════════════════════════════════════════
// Speed result body (reused by draw_speed_panel when speed_done)
// ═══════════════════════════════════════════════════════════════

fn draw_result_body(f: &mut Frame, area: Rect, state: &AppState, report: &crate::report::Report) {
    let result_width = area.width.saturating_sub(4) as usize; // subtract borders + padding

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
                Span::styled(region_s, Style::default().fg(Color::Reset)),
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

        // Header: Path(9) CDN IP(16) Location(auto) Ping(8) TCP(7) Download(11) Upload(11)
        let res_fixed = 9 + 16 + 8 + 7 + 11 + 11 + 12; // fixed columns + separators (6×2)
        let available_loc = result_width.saturating_sub(res_fixed).max(8);
        let max_loc_content = report.paths.iter().map(|p| {
            p.cdn_location.as_deref()
                .map(|l| shorten_geo(l).chars().count())
                .unwrap_or(1)
        }).max().unwrap_or(8).max(8); // min 8 for "Location" header
        let loc_w = max_loc_content.min(available_loc);
        lines.push(Line::from(vec![dim(format!(
            "{:<9}  {:<16}  {:<loc_w$}  {:>8}  {:>7}  {:>11}  {:>11}",
            "Path", "CDN IP", "Location", "HTTP-RTT", "TCP-RTT", "Download", "Upload"
        ))]));
        lines.push(Line::from(vec![dim("─".repeat(result_width))]));

        for path in &report.paths {
            let cdn_ip_s: String = path.cdn_ip.as_deref().unwrap_or("-").chars().take(16).collect();
            let location_s: String = path.cdn_location.as_deref()
                .map(shorten_geo)
                .unwrap_or_else(|| "-".to_string())
                .chars().take(loc_w).collect();

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
                 dim(format!("{:<loc_w$}", location_s)),
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
                 Span::styled(format!("{:<16}", cdn_ip_s), Style::default().fg(Color::Reset)),
                 green(format!("{:<loc_w$}", location_s)),
                 ping_span,
                 tcp_span,
                 Span::styled(dl_s.clone(), Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
                 Span::styled(ul_s.clone(), Style::default().fg(Color::Reset)))
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
                            Span::styled(spark, Style::default().fg(ACCENT)),
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
                                Span::styled("█".repeat(filled.min(BAR_W)), Style::default().fg(ACCENT)),
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
            lines.push(Line::from(vec![dim("─".repeat(result_width))]));
        }
    }

    // ── Single-path results (Cloudflare backend) ──
    if report.paths.is_empty() {
        // resolver_country is derived from the client's egress IP — label it clearly
        let region_label = if report.target_host.contains("cloudflare") { "Your Region" } else { "Region" };
        let region_s = report.resolver_country.as_deref()
            .map(|cc| if cc == "CN" { format!("{cc} (Mainland CN)") } else { cc.to_string() });
        if let Some(r) = region_s {
            lines.push(Line::from(vec![dim(format!("{:<12}", region_label)), color(r, Color::Reset)]));
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
                 Span::styled(value, Style::default().fg(Color::Reset).add_modifier(Modifier::BOLD)))
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
                        Span::styled(spark, Style::default().fg(ACCENT)),
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
                        Span::styled("█".repeat(filled.min(BAR_W)), Style::default().fg(ACCENT)),
                        Span::styled("░".repeat(BAR_W - filled.min(BAR_W)), Style::default().fg(Color::DarkGray)),
                        Span::styled(format!(" {:.1} Mbps", mbps), Style::default().fg(Color::Reset)),
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
    let is_focused = state.result_focus == crate::tui::state::ResultFocus::Speed;
    let title = Line::from(vec![Span::raw(" "), cyan_bold("Speed Results"), Span::raw(" ")]);
    render_scrollable_panel(f, area, title, lines, state.scroll_speed, is_focused);
}

// ── Connectivity probe in-progress panel ─────────────────────────────────────

fn draw_probe_progress_panel(f: &mut Frame, area: Rect, state: &AppState) {
    let is_focused = state.result_focus == crate::tui::state::ResultFocus::Connectivity;
    let inner_w = area.width.saturating_sub(4) as usize;

    let (done, total) = state.probe_progress.unwrap_or((0, 0));

    // Build a lookup set of completed probe names for O(1) check
    let completed: std::collections::HashMap<&str, &crate::probe::types::ProbeResult> =
        state.partial_probe_results.iter().map(|r| (r.name.as_str(), r)).collect();

    const CAT_W: usize = 10;
    const GAP: usize = 2;

    let mut lines: Vec<Line> = vec![];

    // Summary row with live progress
    let bar_w = 20_usize;
    let filled = if total > 0 { (done * bar_w) / total } else { 0 };
    let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(bar_w - filled));
    lines.push(Line::from(vec![
        Span::styled(spin(state.tick).to_string(), Style::default().fg(ACCENT)),
        dim("  "),
        Span::styled(bar, Style::default().fg(ACCENT)),
        dim(format!("  {done}/{total}  probing...")),
    ]));

    // Per-category layout: same as draw_probe_panel but pending sites show as dim spinner
    for &cat in PROBE_CATEGORY_ORDER {
        let group: Vec<&(String, String)> = state.probe_targets.iter()
            .filter(|(_, c)| c == cat)
            .collect();
        if group.is_empty() { continue; }

        let label = probe_category_label(cat);
        let cat_pad = format!("{:<CAT_W$}", label);
        let indent = format!("{:<CAT_W$}", "");

        let mut cur_spans: Vec<Span> = vec![
            Span::styled(
                cat_pad.clone(),
                Style::default().fg(Color::Reset).add_modifier(Modifier::BOLD),
            ),
        ];
        let mut cur_w: usize = CAT_W;

        for (name, _) in &group {
            if let Some(r) = completed.get(name.as_str()) {
                // Completed: show ● or ○ with result
                let (circle, ms_str, circle_sty, name_sty) = if r.reachable {
                    let ms = r.ttfb_ms.map(|v| format!("{:.0}", v)).unwrap_or_default();
                    ("●", ms, Style::default().fg(Color::Green), Style::default().fg(Color::Green))
                } else {
                    ("○", String::new(),
                     Style::default().fg(Color::Red).add_modifier(Modifier::DIM),
                     Style::default().fg(Color::Red).add_modifier(Modifier::DIM))
                };

                let entry_w = 2 + name.chars().count()
                    + if ms_str.is_empty() { 0 } else { 1 + ms_str.len() }
                    + GAP;

                if cur_w + entry_w > inner_w && cur_w > CAT_W {
                    lines.push(Line::from(cur_spans));
                    cur_spans = vec![dim(indent.clone())];
                    cur_w = CAT_W;
                }

                cur_spans.push(Span::styled(circle.to_string(), circle_sty));
                cur_spans.push(dim(" "));
                cur_spans.push(Span::styled(name.to_string(), name_sty));
                if !ms_str.is_empty() {
                    let ms_col = r.ttfb_ms.map(rtt_color).unwrap_or(Color::DarkGray);
                    cur_spans.push(dim(" "));
                    cur_spans.push(Span::styled(ms_str, Style::default().fg(ms_col)));
                }
                cur_spans.push(dim("  "));
                cur_w += entry_w;
            } else {
                // Pending: dim name with spinner
                let entry_w = 2 + name.chars().count() + GAP;

                if cur_w + entry_w > inner_w && cur_w > CAT_W {
                    lines.push(Line::from(cur_spans));
                    cur_spans = vec![dim(indent.clone())];
                    cur_w = CAT_W;
                }

                cur_spans.push(Span::styled(
                    spin(state.tick).to_string(),
                    Style::default().fg(Color::DarkGray),
                ));
                cur_spans.push(dim(" "));
                cur_spans.push(dim(name.to_string()));
                cur_spans.push(dim("  "));
                cur_w += entry_w;
            }
        }
        if cur_w > CAT_W {
            lines.push(Line::from(cur_spans));
        }
    }

    // Clamp scroll + build block
    let title = Line::from(vec![
        Span::raw(" "),
        cyan_bold("Connectivity"),
        dim(format!("  probing {done}/{total}...")),
        Span::raw(" "),
    ]);
    render_scrollable_panel(f, area, title, lines, state.scroll_conn, is_focused);
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
                Style::default().fg(Color::Reset).add_modifier(Modifier::BOLD),
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
    let title = Line::from(vec![
        Span::raw(" "),
        cyan_bold("Connectivity"),
        dim("  ● ok  ○ blocked  ms = TTFB  country from cdn-cgi/trace or GeoIP"),
        Span::raw(" "),
    ]);
    render_scrollable_panel(f, area, title, lines, state.scroll_conn, is_focused);
}



