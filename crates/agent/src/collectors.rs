use fw_proto::messages::StatusSummary;
use serde_json::{json, Value};
use sysinfo::{Disks, Networks, System};

/// Collect a lightweight summary suitable for sending in every heartbeat.
pub fn collect_summary() -> StatusSummary {
    // sysinfo 0.32: new_all() initialises and refreshes everything.
    // We call refresh_cpu_usage() after a brief pause for accurate CPU %.
    // On some hosts (macOS guest, containers) global_cpu_usage may return 0.0
    // on the first call — that is valid; we never panic.
    let mut sys = System::new_all();
    // Second refresh so the CPU delta is non-zero.
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    sys.refresh_cpu_usage();

    let cpu_pct = sys.global_cpu_usage().clamp(0.0, 100.0);

    let mem_total = sys.total_memory();
    let mem_used = sys.used_memory();
    let mem_pct = if mem_total > 0 {
        (mem_used as f32 / mem_total as f32 * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };

    // Use the first non-removable disk as the "root" disk proxy.
    let disks = Disks::new_with_refreshed_list();
    let root = disks.list().iter().find(|d| !d.is_removable());
    let disk_total = root.map(|d| d.total_space()).unwrap_or(0);
    let disk_pct = root
        .map(|d| {
            let total = d.total_space();
            if total > 0 {
                let used = total.saturating_sub(d.available_space());
                (used as f32 / total as f32 * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            }
        })
        .unwrap_or(0.0);

    StatusSummary {
        cpu_pct,
        mem_pct,
        disk_pct,
        uptime_secs: System::uptime(),
        logical_cores: sys.cpus().len() as u32,
        mem_total_bytes: mem_total,
        disk_total_bytes: disk_total,
    }
}

/// Collect detailed information for a given `kind`.
/// Returns a JSON object; unknown kinds return `{"error": "..."}`.
pub fn collect_detail(kind: &str, arg: Option<String>) -> Value {
    match kind {
        "host" => collect_host(),
        "cpu" => collect_cpu(),
        "mem" => collect_mem(),
        "disk" => collect_disk(),
        "net" => collect_net(),
        "proc" => collect_proc(),
        "service" => collect_service(arg),
        other => json!({ "error": format!("未知采集类型: {other}") }),
    }
}

// ── host ─────────────────────────────────────────────────────────────────────

fn collect_host() -> Value {
    json!({
        "hostname": System::host_name().unwrap_or_else(|| "unknown".into()),
        "os": System::long_os_version().unwrap_or_else(|| System::name().unwrap_or_else(|| "unknown".into())),
        "kernel": System::kernel_version().unwrap_or_else(|| "unknown".into()),
        "uptime_secs": System::uptime(),
        "cpu_arch": System::cpu_arch().unwrap_or_else(|| "unknown".into()),
    })
}

// ── cpu ──────────────────────────────────────────────────────────────────────

fn collect_cpu() -> Value {
    let mut sys = System::new_all();
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    sys.refresh_cpu_usage();

    let cores: Vec<Value> = sys
        .cpus()
        .iter()
        .enumerate()
        .map(|(i, c)| {
            json!({
                "core": i,
                "name": c.name(),
                "usage_pct": c.cpu_usage(),
                "frequency_mhz": c.frequency(),
            })
        })
        .collect();

    let load = System::load_average();
    json!({
        "global_usage_pct": sys.global_cpu_usage(),
        "physical_cores": sys.physical_core_count().unwrap_or(0),
        "logical_cores": sys.cpus().len(),
        "load_avg": { "one": load.one, "five": load.five, "fifteen": load.fifteen },
        "cores": cores,
    })
}

// ── mem ──────────────────────────────────────────────────────────────────────

fn collect_mem() -> Value {
    let mut sys = System::new_all();
    sys.refresh_memory();
    json!({
        "total_bytes": sys.total_memory(),
        "used_bytes": sys.used_memory(),
        "free_bytes": sys.free_memory(),
        "available_bytes": sys.available_memory(),
        "total_swap_bytes": sys.total_swap(),
        "used_swap_bytes": sys.used_swap(),
        "free_swap_bytes": sys.free_swap(),
    })
}

// ── disk ─────────────────────────────────────────────────────────────────────

fn collect_disk() -> Value {
    let disks = Disks::new_with_refreshed_list();
    let list: Vec<Value> = disks
        .list()
        .iter()
        .map(|d| {
            json!({
                "name": d.name().to_string_lossy(),
                "mount": d.mount_point().to_string_lossy(),
                "fs": d.file_system().to_string_lossy(),
                "total_bytes": d.total_space(),
                "available_bytes": d.available_space(),
                "used_bytes": d.total_space().saturating_sub(d.available_space()),
                "removable": d.is_removable(),
                "read_only": d.is_read_only(),
            })
        })
        .collect();
    json!({ "disks": list })
}

// ── net ──────────────────────────────────────────────────────────────────────

fn collect_net() -> Value {
    let networks = Networks::new_with_refreshed_list();
    let list: Vec<Value> = networks
        .list()
        .iter()
        .map(|(name, data)| {
            json!({
                "interface": name,
                "rx_bytes": data.total_received(),
                "tx_bytes": data.total_transmitted(),
                "rx_packets": data.total_packets_received(),
                "tx_packets": data.total_packets_transmitted(),
                "rx_errors": data.total_errors_on_received(),
                "tx_errors": data.total_errors_on_transmitted(),
                "mac": data.mac_address().to_string(),
            })
        })
        .collect();
    json!({ "interfaces": list })
}

// ── proc ─────────────────────────────────────────────────────────────────────

fn collect_proc() -> Value {
    let mut sys = System::new_all();
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    // Top 10 by CPU usage
    let mut procs: Vec<_> = sys.processes().values().collect();
    procs.sort_by(|a, b| b.cpu_usage().partial_cmp(&a.cpu_usage()).unwrap_or(std::cmp::Ordering::Equal));
    procs.truncate(10);

    let list: Vec<Value> = procs
        .iter()
        .map(|p| {
            json!({
                "pid": p.pid().as_u32(),
                "name": p.name().to_string_lossy(),
                "cpu_pct": p.cpu_usage(),
                "mem_bytes": p.memory(),
            })
        })
        .collect();

    json!({ "top_by_cpu": list, "total_processes": sys.processes().len() })
}

// ── service ──────────────────────────────────────────────────────────────────

fn collect_service(arg: Option<String>) -> Value {
    let svc = match arg {
        Some(ref s) if !s.is_empty() => s.clone(),
        _ => return json!({ "error": "service kind requires arg (service name)" }),
    };

    // Try systemctl first (Linux), then launchctl (macOS).
    let status = try_systemctl(&svc)
        .or_else(|| try_launchctl(&svc))
        .unwrap_or_else(|| "unknown".into());

    json!({ "service": svc, "status": status })
}

fn try_systemctl(svc: &str) -> Option<String> {
    let out = std::process::Command::new("systemctl")
        .args(["is-active", svc])
        .output()
        .ok()?;
    // systemctl exits non-zero for inactive, but stdout still has the state string.
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn try_launchctl(svc: &str) -> Option<String> {
    // `launchctl list` prints tab-separated: PID  LastExitStatus  Label
    let out = std::process::Command::new("launchctl")
        .arg("list")
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Look for a line whose label column contains the service name.
    for line in stdout.lines() {
        let cols: Vec<&str> = line.splitn(3, '\t').collect();
        if cols.len() == 3 && cols[2].contains(svc) {
            // PID == "-" means not running; a number means running.
            let running = cols[0] != "-";
            return Some(if running { "active" } else { "inactive" }.into());
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_values_in_range() {
        let s = collect_summary();
        assert!(s.cpu_pct >= 0.0 && s.cpu_pct <= 100.0);
        assert!(s.mem_pct >= 0.0 && s.mem_pct <= 100.0);
        assert!(s.disk_pct >= 0.0 && s.disk_pct <= 100.0);
    }

    #[test]
    fn detail_host_has_fields() {
        let v = collect_detail("host", None);
        assert!(v.get("hostname").is_some());
        assert!(v.get("os").is_some());
    }

    #[test]
    fn unknown_kind_returns_error_json() {
        let v = collect_detail("nope", None);
        assert!(v.get("error").is_some());
    }
}
