pub mod render;
pub mod state;

use std::io;
use std::sync::OnceLock;
use std::time::Duration;

use crossterm::event::{self, Event as CEvent, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen,
    disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;

use crate::tui::render::draw_unified;
use crate::tui::state::{AppState, Event, ResultFocus, RetestCmd, StageStatus};

/// Global sender — output.rs uses this to send events without passing it as a parameter
static TX: OnceLock<mpsc::UnboundedSender<Event>> = OnceLock::new();

pub fn global_tx() -> Option<&'static mpsc::UnboundedSender<Event>> {
    TX.get()
}

pub fn send(ev: Event) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(ev);
    }
}

/// Initialise the TUI and return a receiver for use by run_tui_loop.
/// Must be called inside a tokio runtime, before the Terminal is initialised.
pub fn init_channel() -> mpsc::UnboundedReceiver<Event> {
    let (tx, rx) = mpsc::unbounded_channel();
    TX.set(tx).expect("TUI channel already initialized");
    rx
}

/// Start the TUI rendering loop, blocking until the task finishes and the user confirms exit.
/// Must be called on a dedicated OS thread (not a tokio thread) because crossterm event polling is synchronous.
///
/// `retest_tx` sends retest commands back to main thread when user presses R/r on the result page.
pub fn run_tui_loop(
    mut rx: mpsc::UnboundedReceiver<Event>,
    mode: String,
    proxy: Option<String>,
    backend: String,
    abort_handle: tokio::task::AbortHandle,
    retest_tx: std::sync::mpsc::Sender<RetestCmd>,
) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let cb      = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(cb)?;

    let mut state = AppState::new(&mode, proxy, backend);
    let tick_ms   = Duration::from_millis(80);

    let result = 'outer: loop {
        // Drain all pending events from the queue
        loop {
            match rx.try_recv() {
                Ok(ev) => apply_event(&mut state, ev),
                Err(_) => break,
            }
        }

        state.tick = state.tick.wrapping_add(1);

        // Render — always use unified view
        term.draw(|f| draw_unified(f, &mut state))?;

        // Wait for a keyboard event (with timeout)
        if event::poll(tick_ms)? {
            if let CEvent::Key(key) = event::read()? {
                // Windows emits both Press and Release events; only handle Press.
                if key.kind != KeyEventKind::Press { continue; }
                match key.code {
                    // q/Q/Esc exit at any time; abort the task if speed test is still running
                    KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                        abort_handle.abort();
                        break 'outer Ok(());
                    }
                    // Tab: toggle focus between Speed Results and Connectivity
                    KeyCode::Tab => {
                        state.result_focus = match state.result_focus {
                            ResultFocus::Speed        => ResultFocus::Connectivity,
                            ResultFocus::Connectivity => ResultFocus::Speed,
                        };
                    }
                    // R/r: retest the focused panel (only when that panel has completed)
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        let cmd = match state.result_focus {
                            ResultFocus::Speed if state.speed_done && !state.retesting_speed => {
                                Some(RetestCmd::Speed)
                            }
                            ResultFocus::Connectivity if state.probe_done && !state.retesting_probe => {
                                Some(RetestCmd::Probe)
                            }
                            _ => None, // panel not yet completed or already retesting, ignore
                        };
                        if let Some(cmd) = cmd {
                            // Send first — only modify state if the channel is still open
                            if retest_tx.send(cmd.clone()).is_ok() {
                                match &cmd {
                                    RetestCmd::Speed | RetestCmd::SpeedWithBackend(_) => {
                                        state.speed_done = false;
                                        state.finished = false;
                                        state.retesting_speed = true;
                                        state.final_report = None;
                                        state.paths.clear();
                                        state.scroll_speed = 0;
                                        state.ping_status = StageStatus::Waiting;
                                        state.download_status = StageStatus::Waiting;
                                        state.upload_status = StageStatus::Waiting;
                                    }
                                    RetestCmd::Probe => {
                                        state.probe_results.clear();
                                        state.partial_probe_results.clear();
                                        state.probe_targets.clear();
                                        state.probe_progress = Some((0, 0));
                                        state.probe_done = false;
                                        state.finished = false;
                                        state.scroll_conn = 0;
                                        state.retesting_probe = true;
                                    }
                                }
                            }
                        }
                    }
                    // b/B: switch backend and retest speed (only when speed test is done)
                    KeyCode::Char('b') | KeyCode::Char('B') => {
                        if state.speed_done && !state.retesting_speed {
                            let new_backend = if state.backend == "apple" {
                                "cloudflare".to_string()
                            } else {
                                "apple".to_string()
                            };
                            let cmd = RetestCmd::SpeedWithBackend(new_backend.clone());
                            if retest_tx.send(cmd).is_ok() {
                                state.backend = new_backend;
                                state.speed_done = false;
                                state.finished = false;
                                state.retesting_speed = true;
                                state.final_report = None;
                                state.paths.clear();
                                state.scroll_speed = 0;
                                state.ping_status = StageStatus::Waiting;
                                state.download_status = StageStatus::Waiting;
                                state.upload_status = StageStatus::Waiting;
                            }
                        }
                    }
                    // Scroll down in focused panel (clamped to prevent "key blackhole")
                    KeyCode::Down | KeyCode::Char('j') => {
                        match state.result_focus {
                            ResultFocus::Speed        => state.scroll_speed = state.scroll_speed.saturating_add(1).min(500),
                            ResultFocus::Connectivity => state.scroll_conn  = state.scroll_conn.saturating_add(1).min(500),
                        }
                    }
                    // Scroll up in focused panel
                    KeyCode::Up | KeyCode::Char('k') => {
                        match state.result_focus {
                            ResultFocus::Speed        => state.scroll_speed = state.scroll_speed.saturating_sub(1),
                            ResultFocus::Connectivity => state.scroll_conn  = state.scroll_conn.saturating_sub(1),
                        }
                    }
                    _ => {}
                }
            }
        }
    };

    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen)?;
    term.show_cursor()?;
    result
}

fn apply_event(state: &mut AppState, ev: Event) {
    match ev {
        Event::CnMode(is_cn) => {
            state.cn_mode = Some(is_cn);
        }
        Event::EgressDone { v4_cn, v4_global, v6_cn, v6_global,
                            v4_cn_geo, v4_global_geo, v6_cn_geo, v6_global_geo } => {
            state.egress_v4_cn         = v4_cn;
            state.egress_v4_global     = v4_global;
            state.egress_v6_cn         = v6_cn;
            state.egress_v6_global     = v6_global;
            state.egress_done          = true;
            state.egress_v4_cn_geo     = v4_cn_geo;
            state.egress_v4_global_geo = v4_global_geo;
            state.egress_v6_cn_geo     = v6_cn_geo;
            state.egress_v6_global_geo = v6_global_geo;
        }
        Event::ResolveDone { ip, family, source } => {
            state.resolved_ip     = Some(ip);
            state.resolved_family = Some(family);
            state.resolved_source = Some(source);
        }
        Event::GeoDone { location } => {
            state.location = Some(location);
        }
        Event::StageUpdate { stage, status } => {
            match stage {
                "ping"     => state.ping_status     = status,
                "download" => state.download_status = status,
                "upload"   => state.upload_status   = status,
                _ => {}
            }
        }
        Event::PathsInit { paths } => {
            state.paths = paths;
        }
        Event::PathUpdate { path_id, current_stage, cdn_ip, cdn_location, rtt_ms, tcp_rtt_ms, dl_mbps, ul_mbps, error, done } => {
            if let Some(row) = state.paths.iter_mut().find(|r| r.path_id == path_id) {
                // Paths already marked done do not accept state rollbacks
                if row.done { return; }
                row.current_stage = current_stage;
                if cdn_ip.is_some() { row.cdn_ip = cdn_ip; }
                if cdn_location.is_some() { row.cdn_location = cdn_location; }
                if rtt_ms.is_some() { row.rtt_ms = rtt_ms; }
                if tcp_rtt_ms.is_some() { row.tcp_rtt_ms = tcp_rtt_ms; }
                if dl_mbps.is_some() { row.dl_mbps = dl_mbps; }
                if ul_mbps.is_some() { row.ul_mbps = ul_mbps; }
                if error.is_some() { row.error = error; }
                row.done = done;
            }
        }
        Event::Done { report, code } => {
            state.exit_code    = code;
            state.final_report = Some(report);
            state.speed_done   = true;
            state.retesting_speed = false;
            state.recompute_finished();
            // Sync stage statuses to their final values
            if !state.ping_status.is_done() {
                state.ping_status = StageStatus::Fail("skipped".to_string());
            }
            if !state.download_status.is_done() {
                state.download_status = StageStatus::Fail("skipped".to_string());
            }
            if !state.upload_status.is_done() {
                state.upload_status = StageStatus::Fail("skipped".to_string());
            }
        }
        Event::ProbeInit { targets } => {
            state.probe_progress = Some((0, targets.len()));
            state.probe_targets = targets;
            state.partial_probe_results.clear();
        }
        Event::ProbePartial { result } => {
            state.partial_probe_results.push(result);
        }
        Event::ProbeDone { results } => {
            state.probe_results = results;
            state.probe_progress = None;
            state.partial_probe_results.clear();
            state.probe_targets.clear();
            state.probe_done = true;
            state.retesting_probe = false;
            state.recompute_finished();
        }
        Event::ProbeProgress { done, total } => {
            state.probe_progress = Some((done, total));
            // All probes finished — clear the retesting flag immediately so the footer
            // stops showing "retesting connectivity..." before ProbeDone arrives from main.
            if total > 0 && done >= total {
                state.retesting_probe = false;
            }
        }
        Event::Fatal(msg) => {
            state.speed_done   = true;
            state.probe_done   = true;
            state.exit_code    = 2;
            state.ping_status  = StageStatus::Fail(msg);
            state.recompute_finished();
        }
    }
}
