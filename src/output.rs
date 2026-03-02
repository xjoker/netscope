use std::io::IsTerminal;

use reqwest::Url;

use crate::util::now_unix;

#[derive(Clone, Copy)]
pub enum StatusKind {
    Info,
    Ok,
    Error,
}

pub fn use_color() -> bool {
    std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
}

pub struct Colors {
    pub bold:   &'static str,
    pub dim:    &'static str,
    pub cyan:   &'static str,
    pub green:  &'static str,
    pub yellow: &'static str,
    pub red:    &'static str,
    pub white:  &'static str,
    pub reset:  &'static str,
}

impl Colors {
    pub fn new() -> Self {
        if use_color() {
            Colors {
                bold:   "\x1b[1m",
                dim:    "\x1b[2m",
                cyan:   "\x1b[96m",
                green:  "\x1b[92m",
                yellow: "\x1b[93m",
                red:    "\x1b[91m",
                white:  "\x1b[97m",
                reset:  "\x1b[0m",
            }
        } else {
            Colors { bold: "", dim: "", cyan: "", green: "", yellow: "", red: "", white: "", reset: "" }
        }
    }
}

// Box-drawing widths
const W: usize = 74; // inner width

pub fn box_top(c: &Colors) {
    println!("  {}╭{}╮{}", c.cyan, "─".repeat(W), c.reset);
}
fn box_mid(c: &Colors) {
    println!("  {}├{}┤{}", c.dim, "─".repeat(W), c.reset);
}
pub fn box_bot(c: &Colors) {
    println!("  {}╰{}╯{}", c.cyan, "─".repeat(W), c.reset);
}
/// Print a box row: │  content  │ (content is already the raw string, no ANSI for padding calc)
pub fn box_row(c: &Colors, content: &str, colored_content: &str) {
    // visual width of content (no ANSI)
    let vlen = content.chars().count();
    let pad = if vlen + 2 < W { W - 2 - vlen } else { 0 };
    println!("  {}│{}  {}{}{:>pad$}{}│{}", c.cyan, c.reset, colored_content, c.reset, "", c.cyan, c.reset, pad = pad);
}
pub fn box_sep_row(c: &Colors) {
    box_mid(c);
}

pub fn emit_status(is_json: bool, kind: StatusKind, step: &str, message: &str) {
    if is_json {
        let tag = match kind { StatusKind::Info => "INFO", StatusKind::Ok => "OK  ", StatusKind::Error => "ERR " };
        eprintln!("[{tag}] {step:<10} {message}");
        return;
    }
    // Silent in TUI mode: already rendered on alternate screen, skip stdout
    if crate::tui::global_tx().is_some() {
        return;
    }

    let c = Colors::new();
    let (sym, col) = match kind {
        StatusKind::Info  => ("◆", c.cyan),
        StatusKind::Ok    => ("✓", c.green),
        StatusKind::Error => ("✗", c.red),
    };
    // step label fixed 10 chars (raw), then message
    println!("  {col}{sym}{} {}{step:<10}{}  {message}", c.reset, c.dim, c.reset);
}

pub fn emit_start_banner(is_json: bool, mode: &str, base_url: &Url, timeout: u64) {
    if is_json {
        eprintln!("[INFO] starting mode={mode} target={base_url} timeout={timeout}s");
        return;
    }
    // Skip banner in TUI mode
    if crate::tui::global_tx().is_some() {
        return;
    }

    let c = Colors::new();
    let ver = env!("APP_VERSION");
    let target = base_url.host_str().unwrap_or("-");

    box_top(&c);

    // Title row
    let title_raw  = format!("netscope  v{ver}");
    let mode_raw   = format!("mode: {mode}");
    let title_col  = format!("{}{}netscope{} {}{}v{ver}{}", c.bold, c.white, c.reset, c.dim, c.cyan, c.reset);
    let mode_col   = format!("{}mode: {}{}{mode}{}", c.dim, c.reset, c.cyan, c.reset);
    let inner_raw  = format!("{title_raw}  {mode_raw}");
    let inner_col  = format!("{title_col}  {mode_col}");
    box_row(&c, &inner_raw, &inner_col);

    box_sep_row(&c);

    // Info row
    let info_raw = format!("target: {target:<34}  timeout: {timeout}s");
    let info_col = format!(
        "{}target:{} {}{target:<34}{}  {}timeout:{} {timeout}s",
        c.dim, c.reset, c.white, c.reset, c.dim, c.reset,
    );
    box_row(&c, &info_raw, &info_col);

    let ts = now_unix();
    let ts_raw = format!("start:  {ts}");
    let ts_col = format!("{}start:{} {}{ts}{}", c.dim, c.reset, c.dim, c.reset);
    box_row(&c, &ts_raw, &ts_col);

    box_bot(&c);
    println!();
}

pub fn short_message(s: &str) -> String {
    let first = s.lines().next().unwrap_or("-");
    if first.chars().count() <= 40 {
        first.to_string()
    } else {
        first.chars().take(37).collect::<String>() + "..."
    }
}
