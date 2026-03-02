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
        // Initialise the channel before starting tokio
        let rx = tui::init_channel();

        let tui_mode    = crate::cli::command_name(
            cli.command.as_ref().unwrap_or(&Command::Full {
                count: 3,
                duration: 12,
                ul_mib: 16,
                ul_repeat: 2,
            })
        ).to_string();
        let tui_target  = match cli.backend.as_str() {
            "cloudflare" => "speed.cloudflare.com".to_string(),
            _            => "mensura.cdn-apple.com".to_string(),
        };
        let tui_timeout = cli.timeout;
        let tui_proxy   = cli.proxy.clone();
        let tui_backend = cli.backend.clone();

        // Wrap the speed-test task inside a tokio task so the TUI can abort it immediately on q
        let speed_task = tokio::task::spawn(async move { run(cli).await });
        let abort_handle = speed_task.abort_handle();

        // Launch the TUI rendering loop on a dedicated OS thread (crossterm event polling is synchronous/blocking)
        let tui_handle = std::thread::spawn(move || {
            tui::run_tui_loop(rx, tui_mode, tui_target, tui_timeout, tui_proxy, tui_backend, abort_handle)
        });

        let exit_code = match speed_task.await {
            Ok(Ok((report, code))) => {
                tui::send(crate::tui::state::Event::Done {
                    report: Box::new(report),
                    code,
                });
                code
            }
            Ok(Err(err)) => {
                tui::send(crate::tui::state::Event::Fatal(format!("{err:#}")));
                2
            }
            // Task was aborted (user pressed q during test)
            Err(e) if e.is_cancelled() => 130,
            Err(e) => {
                tui::send(crate::tui::state::Event::Fatal(format!("task panic: {e}")));
                2
            }
        };

        // Wait for the TUI thread to exit (user confirms with a keypress)
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
