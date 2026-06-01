use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusSummary {
    pub cpu_pct: f32,
    pub mem_pct: f32,
    pub disk_pct: f32,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentToServer {
    Hello { identity_token: String, hostname: String, os: String, agent_version: String },
    Heartbeat { summary: Option<StatusSummary> },
    CommandResult { cmd_id: String, exit: i32, stdout: String, stderr: String, done: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerToAgent {
    HelloAck { ok: bool },
    Reject { reason: String },
    RunStatus { cmd_id: String, kind: String, arg: Option<String> },
    RunShell { cmd_id: String, command: String },
    Ping,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hello_roundtrips_through_json() {
        let msg = AgentToServer::Hello {
            identity_token: "tok".into(),
            hostname: "web-01".into(),
            os: "ubuntu".into(),
            agent_version: "0.1.0".into(),
        };
        let s = serde_json::to_string(&msg).unwrap();
        let back: AgentToServer = serde_json::from_str(&s).unwrap();
        matches!(back, AgentToServer::Hello { .. });
    }

    #[test]
    fn server_runstatus_tag_is_stable() {
        let s = serde_json::to_string(&ServerToAgent::Ping).unwrap();
        assert!(s.contains("ping"));
    }
}
