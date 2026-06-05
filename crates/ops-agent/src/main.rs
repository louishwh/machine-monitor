//! ops-agent — FleetWatch read-only fleet health observer (Phase 1).
//!
//! Polls the signed control plane, evaluates thresholds, and emits alerts on
//! state changes (new breach / resolved) plus a periodic health report.
//! Strictly read-only: no shell, no writes — zero blast radius.

mod client;
mod health;

use anyhow::Result;
use clap::Parser;
use std::collections::BTreeSet;
use std::path::PathBuf;

use client::Client;
use health::{diff, evaluate, Finding, Machine, Severity, Thresholds};

#[derive(Parser)]
#[command(
    name = "ops-agent",
    about = "FleetWatch read-only fleet health observer"
)]
struct Cli {
    /// Control-plane base URL (e.g. https://api.example.com/fleet)
    #[arg(long)]
    server: String,
    /// Operator key file (default: ~/.fleetwatch/master.key)
    #[arg(long, default_value = "")]
    key: String,
    /// Path to server CA PEM (for self-signed https)
    #[arg(long)]
    server_ca: Option<String>,
    /// Poll interval seconds
    #[arg(long, default_value_t = 30)]
    interval: u64,
    /// Print a full health report every N polls (0 = never)
    #[arg(long, default_value_t = 10)]
    report_every: u64,
    /// CPU / memory / disk alert thresholds (percent)
    #[arg(long, default_value_t = 90)]
    cpu: u8,
    #[arg(long, default_value_t = 90)]
    mem: u8,
    #[arg(long, default_value_t = 90)]
    disk: u8,
    /// Print one report and exit
    #[arg(long, default_value_t = false)]
    once: bool,
}

fn key_path(s: &str) -> PathBuf {
    if !s.is_empty() {
        return PathBuf::from(s);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".fleetwatch").join("master.key")
}

fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn full_report(machines: &[Machine], findings: &[Finding]) -> String {
    let total = machines.len();
    let online = machines.iter().filter(|m| m.online).count();
    let mut s = format!(
        "[{}] fleet: {total} machines · {online} online · {} offline · {} findings\n",
        now(),
        total - online,
        findings.len()
    );
    for m in machines {
        let sum = m.summary.as_ref();
        let badge = if m.online { "●" } else { "○" };
        match sum {
            Some(x) if m.online => s.push_str(&format!(
                "  {badge} {:<14} cpu {:>3.0}%  mem {:>3.0}%  disk {:>3.0}%\n",
                m.name, x.cpu_pct, x.mem_pct, x.disk_pct
            )),
            _ => s.push_str(&format!("  {badge} {:<14} (offline)\n", m.name)),
        }
    }
    if !findings.is_empty() {
        s.push_str("  findings:\n");
        for f in findings {
            s.push_str(&format!(
                "    [{}] {} — {}\n",
                f.severity.tag(),
                f.machine_name,
                f.detail
            ));
        }
    }
    s
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let th = Thresholds {
        cpu: cli.cpu,
        mem: cli.mem,
        disk: cli.disk,
    };
    let client = Client::new(&cli.server, &key_path(&cli.key), cli.server_ca.as_deref())?;

    // One-shot mode
    if cli.once {
        let machines = client.list_machines().await?;
        let findings = evaluate(&machines, th);
        print!("{}", full_report(&machines, &findings));
        return Ok(());
    }

    eprintln!(
        "[{}] ops-agent watching {} every {}s (thresholds cpu/mem/disk = {}/{}/{}%) — read-only",
        now(),
        cli.server,
        cli.interval,
        th.cpu,
        th.mem,
        th.disk
    );

    let mut active: BTreeSet<String> = BTreeSet::new();
    let mut tick: u64 = 0;
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(cli.interval.max(1)));

    loop {
        ticker.tick().await;
        tick += 1;
        let machines = match client.list_machines().await {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[{}] poll failed: {e}", now());
                continue;
            }
        };
        let findings = evaluate(&machines, th);
        let (newly, resolved) = diff(&active, &findings);

        for f in &newly {
            let mark = if f.severity == Severity::Crit {
                "🔴"
            } else {
                "🟠"
            };
            println!(
                "[{}] {mark} ALERT [{}] {} — {}",
                now(),
                f.severity.tag(),
                f.machine_name,
                f.detail
            );
        }
        for key in &resolved {
            let name = key.split(':').next().unwrap_or(key);
            println!("[{}] 🟢 RESOLVED {key} ({name})", now());
        }

        active = findings.iter().map(|f| f.key()).collect();

        if cli.report_every > 0 && tick.is_multiple_of(cli.report_every) {
            print!("{}", full_report(&machines, &findings));
        }
    }
}
