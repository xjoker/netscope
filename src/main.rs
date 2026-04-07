mod cli;
mod network;
mod output;
mod probe;
mod report;
mod speed;
mod tui;
mod util;

use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, Command, sanitize_proxy_display};
use crate::network::egress::detect_egress_profile;
use crate::probe::target::all_targets;
use crate::probe::types::ProbeReport;
use crate::probe::{output_probe_report, run_probe};
use crate::report::output_report;
use crate::speed::runner::run;
use crate::tui::state::RetestCmd;

/// Default concurrency for connectivity probe
const PROBE_CONCURRENCY: usize = 8;
/// Default timeout (seconds) for each probe target
const PROBE_TIMEOUT: u64 = 10;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let is_json = cli.json;

    // Check for probe subcommand (runs independently without speed test)
    if let Some(Command::Probe {
        concurrency,
        probe_timeout,
        category,
        site,
        skip_geo,
    }) = &cli.command
    {
        let proxy = cli.proxy.as_deref();
        let concurrency = *concurrency;
        let probe_timeout = *probe_timeout;
        let skip_geo = *skip_geo;

        // Retrieve egress IPs (for display)
        let egress = detect_egress_profile(cli.timeout, proxy).await;

        // Concurrently look up geolocation for each egress IP
        let (geo_v4_cn, geo_v4_global, geo_v6_cn, geo_v6_global) = {
            use std::collections::BTreeSet;
            use std::net::IpAddr;
            let unique_ips: BTreeSet<IpAddr> = [
                egress.ipv4_cn,
                egress.ipv4_global,
                egress.ipv6_cn,
                egress.ipv6_global,
            ]
            .into_iter()
            .flatten()
            .collect();
            let geo_timeout = cli.timeout.max(3).min(6);
            let mut geo_tasks: tokio::task::JoinSet<(IpAddr, Option<String>)> =
                tokio::task::JoinSet::new();
            for ip in unique_ips {
                let proxy_owned2 = proxy.map(str::to_string);
                let t = geo_timeout;
                geo_tasks.spawn(async move {
                    let loc = crate::network::geo::lookup_ip_location(ip, t, proxy_owned2.as_deref())
                        .await
                        .ok()
                        .flatten();
                    (ip, loc)
                });
            }
            let mut egress_geo: std::collections::HashMap<std::net::IpAddr, Option<String>> =
                std::collections::HashMap::new();
            while let Some(Ok((ip, loc))) = geo_tasks.join_next().await {
                egress_geo.insert(ip, loc);
            }
            let gv4cn = egress.ipv4_cn.and_then(|ip| egress_geo.get(&ip).cloned().flatten());
            let gv4gl = egress.ipv4_global.and_then(|ip| egress_geo.get(&ip).cloned().flatten());
            let gv6cn = egress.ipv6_cn.and_then(|ip| egress_geo.get(&ip).cloned().flatten());
            let gv6gl = egress.ipv6_global.and_then(|ip| egress_geo.get(&ip).cloned().flatten());
            (gv4cn, gv4gl, gv6cn, gv6gl)
        };

        // Filter targets
        let mut targets = all_targets();
        if let Some(cat) = category {
            let cats: Vec<&str> = cat.split(',').map(str::trim).collect();
            targets.retain(|t| cats.contains(&t.category));
        }
        if let Some(s) = site {
            targets.retain(|t| t.name.to_lowercase().contains(&s.to_lowercase()));
        }

        let results = run_probe(targets, concurrency, probe_timeout, proxy, skip_geo).await;

        let total = results.len();
        let reachable = results.iter().filter(|r| r.reachable).count();
        let report = ProbeReport {
            proxy: proxy.map(sanitize_proxy_display),
            egress_ipv4: egress.ipv4.map(|v| v.to_string()),
            egress_ipv4_cn: egress.ipv4_cn.map(|v| v.to_string()),
            egress_ipv4_cn_geo: geo_v4_cn,
            egress_ipv4_global: egress.ipv4_global.map(|v| v.to_string()),
            egress_ipv4_global_geo: geo_v4_global,
            egress_ipv6: egress.ipv6.map(|v| v.to_string()),
            egress_ipv6_cn: egress.ipv6_cn.map(|v| v.to_string()),
            egress_ipv6_cn_geo: geo_v6_cn,
            egress_ipv6_global: egress.ipv6_global.map(|v| v.to_string()),
            egress_ipv6_global_geo: geo_v6_global,
            total,
            reachable,
            unreachable: total - reachable,
            results,
        };
        output_probe_report(&report, is_json);
        return ExitCode::SUCCESS;
    }

    // Speed-test flow: launch TUI in non-JSON mode
    if !is_json {
        let rx = tui::init_channel();

        let tui_mode = crate::cli::command_name(
            cli.command.as_ref().unwrap_or(&Command::Full {
                count: 8, duration: 20, ul_mib: 16, ul_repeat: 3,
            })
        ).to_string();
        let tui_proxy   = cli.proxy.clone();
        let tui_backend = cli.backend.clone();

        let (retest_tx, retest_rx) = std::sync::mpsc::channel::<RetestCmd>();

        // Launch TUI immediately — no tasks spawned yet, user presses s to start
        let tui_handle = std::thread::spawn(move || {
            tui::run_tui_loop(rx, tui_mode, tui_proxy, tui_backend, retest_tx)
        });

        // Helper closures for dispatching commands
        let spawn_speed = |cli: &Cli| -> tokio::task::JoinHandle<(report::Report, u8)> {
            let cli_clone = cli.clone();
            tokio::task::spawn(async move {
                match run(cli_clone).await {
                    Ok((report, code)) => (report, code),
                    Err(err) => {
                        tui::send(crate::tui::state::Event::Fatal(format!("{err:#}")));
                        (report::Report::default(), 2)
                    }
                }
            })
        };
        let spawn_speed_backend = |cli: &Cli, backend: String| -> tokio::task::JoinHandle<(report::Report, u8)> {
            let mut cli_clone = cli.clone();
            cli_clone.backend = backend;
            tokio::task::spawn(async move {
                match run(cli_clone).await {
                    Ok((report, code)) => (report, code),
                    Err(err) => {
                        tui::send(crate::tui::state::Event::Fatal(format!("{err:#}")));
                        (report::Report::default(), 2)
                    }
                }
            })
        };
        let spawn_probe = |cli: &Cli| -> tokio::task::JoinHandle<Vec<crate::probe::types::ProbeResult>> {
            let proxy = cli.proxy.clone();
            tokio::task::spawn(async move {
                let targets = all_targets();
                run_probe(targets, PROBE_CONCURRENCY, PROBE_TIMEOUT, proxy.as_deref(), false).await
            })
        };

        let mut exit_code: u8 = 0;
        let mut speed_task: Option<tokio::task::JoinHandle<(report::Report, u8)>> = None;
        let mut probe_task: Option<tokio::task::JoinHandle<Vec<crate::probe::types::ProbeResult>>> = None;

        // Unified command loop — handles initial start and retests
        'cmd: loop {
            // Drain pending commands
            loop {
                match retest_rx.try_recv() {
                    Ok(cmd) => {
                        match cmd {
                            RetestCmd::StartAll => {
                                if speed_task.is_none() { speed_task = Some(spawn_speed(&cli)); }
                                if probe_task.is_none() { probe_task = Some(spawn_probe(&cli)); }
                            }
                            RetestCmd::Speed if speed_task.is_none() => {
                                speed_task = Some(spawn_speed(&cli));
                            }
                            RetestCmd::SpeedWithBackend(ref backend) if speed_task.is_none() => {
                                speed_task = Some(spawn_speed_backend(&cli, backend.clone()));
                            }
                            RetestCmd::Probe if probe_task.is_none() => {
                                probe_task = Some(spawn_probe(&cli));
                            }
                            _ => {}
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break 'cmd,
                }
            }

            // Check completed tasks
            if speed_task.as_ref().map_or(false, |t| t.is_finished()) {
                let task = speed_task.take().unwrap();
                match task.await {
                    Ok((report, code)) => {
                        exit_code = code;
                        tui::send(crate::tui::state::Event::Done {
                            report: Box::new(report), code,
                        });
                    }
                    Err(_) => {
                        tui::send(crate::tui::state::Event::Fatal("speed task panic".to_string()));
                        exit_code = 2;
                    }
                }
            }
            if probe_task.as_ref().map_or(false, |t| t.is_finished()) {
                let task = probe_task.take().unwrap();
                match task.await {
                    Ok(results) => tui::send(crate::tui::state::Event::ProbeDone { results }),
                    Err(_) => tui::send(crate::tui::state::Event::ProbeDone { results: vec![] }),
                }
            }

            // Wait for next event
            if speed_task.is_none() && probe_task.is_none() {
                match retest_rx.recv_timeout(std::time::Duration::from_millis(50)) {
                    Ok(cmd) => match cmd {
                        RetestCmd::StartAll => {
                            speed_task = Some(spawn_speed(&cli));
                            probe_task = Some(spawn_probe(&cli));
                        }
                        RetestCmd::Speed => { speed_task = Some(spawn_speed(&cli)); }
                        RetestCmd::SpeedWithBackend(ref b) => { speed_task = Some(spawn_speed_backend(&cli, b.clone())); }
                        RetestCmd::Probe => { probe_task = Some(spawn_probe(&cli)); }
                    },
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break 'cmd,
                }
            } else {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }

        let _ = tui_handle.join();
        ExitCode::from(exit_code)
    } else {
        // JSON mode: original flow
        let verbose = cli.verbose;
        match run(cli).await {
            Ok((report, code)) => {
                output_report(&report, code, true, verbose);
                ExitCode::from(code)
            }
            Err(err) => {
                eprintln!("fatal: {err:#}");
                ExitCode::from(2)
            }
        }
    }
}
