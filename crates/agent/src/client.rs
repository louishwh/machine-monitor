use std::time::Duration;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use fw_proto::messages::{AgentToServer, ServerToAgent};
use crate::config::AgentConfig;

/// Build a CommandResult by collecting detailed status for `kind`.
/// This is a pure (sync, blocking) function — callers in async context must use
/// `tokio::task::spawn_blocking`.
pub fn build_status_result(cmd_id: String, kind: String, arg: Option<String>) -> AgentToServer {
    let detail = crate::collectors::collect_detail(&kind, arg);
    let stdout = serde_json::to_string(&detail)
        .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}).to_string());
    AgentToServer::CommandResult {
        cmd_id,
        exit: 0,
        stdout,
        stderr: String::new(),
        done: true,
    }
}

pub fn next_backoff(curr: u64) -> u64 { (curr * 2).min(30) }

fn detect_os() -> String {
    if cfg!(target_os = "macos") { "macos".into() } else { "ubuntu".into() }
}

pub async fn run_loop(cfg: AgentConfig) {
    let mut backoff = 1u64;
    loop {
        match connect_once(&cfg).await {
            Ok(()) => backoff = 1,
            Err(e) => tracing::warn!(error=%e, "agent session ended"),
        }
        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = next_backoff(backoff);
    }
}

async fn connect_once(cfg: &AgentConfig) -> anyhow::Result<()> {
    let (mut ws, _) = tokio_tungstenite::connect_async(&cfg.server_url).await?;
    let hostname = hostname();
    let hello = AgentToServer::Hello {
        identity_token: cfg.identity_token.clone(),
        hostname,
        os: detect_os(),
        agent_version: env!("CARGO_PKG_VERSION").into(),
    };
    ws.send(Message::Text(serde_json::to_string(&hello)?)).await?;

    let mut hb = tokio::time::interval(Duration::from_secs(15));
    loop {
        tokio::select! {
            _ = hb.tick() => {
                let summary = tokio::task::spawn_blocking(crate::collectors::collect_summary)
                    .await
                    .ok();
                let m = AgentToServer::Heartbeat { summary };
                ws.send(Message::Text(serde_json::to_string(&m)?)).await?;
            }
            msg = ws.next() => {
                let Some(msg) = msg else { break };
                let Message::Text(txt) = msg? else { continue };
                match serde_json::from_str::<ServerToAgent>(&txt) {
                    Ok(ServerToAgent::Ping) => { /* keepalive */ }
                    Ok(ServerToAgent::RunStatus { cmd_id, kind, arg }) => {
                        match tokio::task::spawn_blocking(move || build_status_result(cmd_id, kind, arg)).await {
                            Ok(result) => {
                                ws.send(Message::Text(serde_json::to_string(&result)?)).await?;
                            }
                            Err(e) => tracing::warn!(error=%e, "spawn_blocking join error for RunStatus"),
                        }
                    }
                    Ok(ServerToAgent::RunShell { .. }) => {
                        // M4 — shell execution not yet implemented, silently ignore.
                    }
                    Ok(_) | Err(_) => {}
                }
            }
        }
    }
    Ok(())
}

fn hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(next_backoff(1), 2);
        assert_eq!(next_backoff(2), 4);
        assert_eq!(next_backoff(20), 30); // capped at 30
    }

    #[test]
    fn build_status_result_carries_cmd_id() {
        let r = build_status_result("c-1".into(), "host".into(), None);
        match r {
            fw_proto::messages::AgentToServer::CommandResult { cmd_id, done, .. } => {
                assert_eq!(cmd_id, "c-1");
                assert!(done);
            }
            _ => panic!("expected CommandResult"),
        }
    }
}
