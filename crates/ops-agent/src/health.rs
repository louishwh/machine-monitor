//! Pure fleet-health logic: threshold evaluation + state-change diffing.
//! Kept side-effect-free so it's unit-testable without a live server.

use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Summary {
    #[serde(default)]
    pub cpu_pct: f64,
    #[serde(default)]
    pub mem_pct: f64,
    #[serde(default)]
    pub disk_pct: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Machine {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub online: bool,
    #[serde(default)]
    pub summary: Option<Summary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thresholds {
    pub cpu: u8,
    pub mem: u8,
    pub disk: u8,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self { cpu: 90, mem: 90, disk: 90 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Warn,
    Crit,
}

impl Severity {
    pub fn tag(self) -> &'static str {
        match self {
            Severity::Warn => "WARN",
            Severity::Crit => "CRIT",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub machine_id: String,
    pub machine_name: String,
    /// Stable kind: "offline" | "cpu" | "mem" | "disk"
    pub kind: String,
    pub severity: Severity,
    pub detail: String,
}

impl Finding {
    /// Stable identity for state-change tracking (one breach per machine+kind).
    pub fn key(&self) -> String {
        format!("{}:{}", self.machine_id, self.kind)
    }
}

/// Evaluate the current fleet snapshot against thresholds.
/// Offline machines yield only an `offline` finding (their summary is stale).
pub fn evaluate(machines: &[Machine], th: Thresholds) -> Vec<Finding> {
    let mut out = Vec::new();
    for m in machines {
        if !m.online {
            out.push(Finding {
                machine_id: m.id.clone(),
                machine_name: m.name.clone(),
                kind: "offline".into(),
                severity: Severity::Crit,
                detail: "agent offline (no recent heartbeat)".into(),
            });
            continue;
        }
        let Some(s) = &m.summary else { continue };
        let mut check = |kind: &str, val: f64, limit: u8, label: &str| {
            if val >= limit as f64 {
                out.push(Finding {
                    machine_id: m.id.clone(),
                    machine_name: m.name.clone(),
                    kind: kind.into(),
                    severity: if val >= 95.0 { Severity::Crit } else { Severity::Warn },
                    detail: format!("{label} {:.0}% ≥ {}%", val, limit),
                });
            }
        };
        check("cpu", s.cpu_pct, th.cpu, "CPU");
        check("mem", s.mem_pct, th.mem, "memory");
        check("disk", s.disk_pct, th.disk, "disk");
    }
    out
}

/// Diff against the previously-active finding keys. Returns (newly_firing, resolved).
pub fn diff(prev: &BTreeSet<String>, current: &[Finding]) -> (Vec<Finding>, Vec<String>) {
    let cur_keys: BTreeSet<String> = current.iter().map(|f| f.key()).collect();
    let newly: Vec<Finding> = current
        .iter()
        .filter(|f| !prev.contains(&f.key()))
        .cloned()
        .collect();
    let resolved: Vec<String> = prev.difference(&cur_keys).cloned().collect();
    (newly, resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: &str, online: bool, cpu: f64, mem: f64, disk: f64) -> Machine {
        Machine {
            id: id.into(),
            name: id.into(),
            online,
            summary: Some(Summary { cpu_pct: cpu, mem_pct: mem, disk_pct: disk }),
        }
    }

    #[test]
    fn flags_breaches_and_offline() {
        let th = Thresholds::default(); // 90/90/90
        let fleet = vec![
            m("ok", true, 10.0, 20.0, 30.0),
            m("hot", true, 92.0, 50.0, 50.0),
            m("full", true, 50.0, 50.0, 97.0),
            m("down", false, 0.0, 0.0, 0.0),
        ];
        let f = evaluate(&fleet, th);
        let keys: BTreeSet<String> = f.iter().map(|x| x.key()).collect();
        assert!(keys.contains("hot:cpu"));
        assert!(keys.contains("full:disk"));
        assert!(keys.contains("down:offline"));
        assert!(!keys.contains("ok:cpu"));
        // 97% disk is critical, 92% cpu is warn
        assert_eq!(f.iter().find(|x| x.key() == "full:disk").unwrap().severity, Severity::Crit);
        assert_eq!(f.iter().find(|x| x.key() == "hot:cpu").unwrap().severity, Severity::Warn);
    }

    #[test]
    fn diff_reports_new_and_resolved() {
        let prev: BTreeSet<String> = ["a:cpu".to_string(), "b:offline".to_string()].into_iter().collect();
        let current = vec![
            Finding { machine_id: "a".into(), machine_name: "a".into(), kind: "cpu".into(), severity: Severity::Warn, detail: "".into() },
            Finding { machine_id: "c".into(), machine_name: "c".into(), kind: "disk".into(), severity: Severity::Crit, detail: "".into() },
        ];
        let (newly, resolved) = diff(&prev, &current);
        // a:cpu still firing (not new), c:disk is new, b:offline resolved
        assert_eq!(newly.len(), 1);
        assert_eq!(newly[0].key(), "c:disk");
        assert_eq!(resolved, vec!["b:offline".to_string()]);
    }
}
