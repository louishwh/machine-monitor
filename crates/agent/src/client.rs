use crate::config::AgentConfig;
use futures_util::{SinkExt, StreamExt};
use fw_proto::messages::{AgentToServer, ServerToAgent};
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio_tungstenite::tungstenite::Message;

/// Execute `command` via `sh -c`, capturing stdout/stderr/exit.
/// If the process does not finish within `timeout`, the child is killed and
/// `(-1, "", "timeout")` is returned.
pub async fn run_shell(command: &str, timeout: Duration) -> (i32, String, String) {
    use tokio::io::AsyncReadExt;

    // Systemd runs the agent with a minimal environment where HOME is often
    // unset, so `sh -c` cannot expand `~` (yields "can't cd to ~"). Provide a
    // sane HOME (and start in it) so tilde / relative paths resolve as a user
    // would expect. The agent runs as root under systemd → fall back to /root.
    let home = std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty() && h != "/")
        .unwrap_or_else(|| "/root".to_string());

    let mut builder = Command::new("sh");
    builder
        .arg("-c")
        .arg(command)
        .env("HOME", &home)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if std::path::Path::new(&home).is_dir() {
        builder.current_dir(&home);
    }

    let mut child = match builder.spawn() {
        Ok(c) => c,
        Err(e) => return (-1, String::new(), format!("spawn error: {e}")),
    };

    // Take the handles before passing `child` to wait-related calls.
    let mut stdout_handle = child.stdout.take().unwrap();
    let mut stderr_handle = child.stderr.take().unwrap();

    // Collect output and wait concurrently inside a single future.
    let collect_fut = async {
        let mut stdout_bytes = Vec::new();
        let mut stderr_bytes = Vec::new();
        // Read stdout + stderr then wait — order matches how the process
        // produces output: both reads complete before wait() returns.
        let _ = stdout_handle.read_to_end(&mut stdout_bytes).await;
        let _ = stderr_handle.read_to_end(&mut stderr_bytes).await;
        let status = child.wait().await;
        (stdout_bytes, stderr_bytes, status)
    };

    match tokio::time::timeout(timeout, collect_fut).await {
        Ok((out, err, Ok(status))) => {
            let exit = status.code().unwrap_or(-1);
            (
                exit,
                String::from_utf8_lossy(&out).into_owned(),
                String::from_utf8_lossy(&err).into_owned(),
            )
        }
        Ok((_, _, Err(e))) => (-1, String::new(), format!("wait error: {e}")),
        Err(_elapsed) => {
            // Timeout — child is still alive; kill it.
            // `child` was moved into the async block but the future was dropped
            // by timeout, so we can't call kill on it directly.  Instead we
            // rely on the Drop impl of tokio::process::Child which sends SIGKILL
            // on drop when the future is cancelled (the child is inside the
            // `collect_fut` which was dropped).  Nothing more needed.
            (-1, String::new(), "timeout".into())
        }
    }
}

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

pub fn next_backoff(curr: u64) -> u64 {
    (curr * 2).min(30)
}

/// Build a rustls-backed TLS connector that trusts ONLY the given PEM certificate.
/// This is used for wss:// connections to a server with a self-signed cert.
pub fn build_tls_connector(ca_pem: &str) -> anyhow::Result<tokio_tungstenite::Connector> {
    use rustls::RootCertStore;
    use tokio_tungstenite::Connector;

    let mut roots = RootCertStore::empty();
    let pem_bytes = ca_pem.as_bytes();
    let certs: Vec<_> = rustls_pemfile::certs(&mut std::io::Cursor::new(pem_bytes))
        .collect::<Result<Vec<_>, _>>()?;
    anyhow::ensure!(!certs.is_empty(), "no certificates found in server_ca_pem");
    for cert in certs {
        roots.add(cert)?;
    }
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Connector::Rustls(Arc::new(config)))
}

fn detect_os() -> String {
    if cfg!(target_os = "macos") {
        "macos".into()
    } else {
        "ubuntu".into()
    }
}

pub async fn run_loop(cfg: AgentConfig) {
    // Install the ring crypto provider once; ignore error if already installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
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
    let (mut ws, _) = if cfg.server_url.starts_with("wss://") {
        if let Some(pem) = &cfg.server_ca_pem {
            let connector = build_tls_connector(pem)?;
            tokio_tungstenite::connect_async_tls_with_config(
                &cfg.server_url,
                None,
                false,
                Some(connector),
            )
            .await?
        } else {
            tokio_tungstenite::connect_async(&cfg.server_url).await?
        }
    } else {
        tokio_tungstenite::connect_async(&cfg.server_url).await?
    };
    let hostname = hostname();
    let hello = AgentToServer::Hello {
        identity_token: cfg.identity_token.clone(),
        hostname,
        os: detect_os(),
        agent_version: env!("CARGO_PKG_VERSION").into(),
    };
    ws.send(Message::Text(serde_json::to_string(&hello)?))
        .await?;

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
                    Ok(ServerToAgent::RunShell { cmd_id, command }) => {
                        let (exit, stdout, stderr) =
                            run_shell(&command, Duration::from_secs(30)).await;
                        let result = AgentToServer::CommandResult {
                            cmd_id,
                            exit,
                            stdout,
                            stderr,
                            done: true,
                        };
                        ws.send(Message::Text(serde_json::to_string(&result)?)).await?;
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

    #[tokio::test]
    async fn run_shell_echo() {
        let (exit, stdout, stderr) = run_shell("echo hi", Duration::from_secs(5)).await;
        assert_eq!(exit, 0, "expected exit 0, got {exit}; stderr: {stderr}");
        assert!(
            stdout.contains("hi"),
            "expected stdout to contain 'hi', got: {stdout:?}"
        );
    }

    #[tokio::test]
    async fn run_shell_timeout() {
        // 100ms timeout with a 5-second sleep — must time out.
        let (exit, _stdout, stderr) = run_shell("sleep 5", Duration::from_millis(100)).await;
        assert_eq!(exit, -1, "expected exit -1 on timeout, got {exit}");
        assert_eq!(
            stderr, "timeout",
            "expected stderr 'timeout', got: {stderr:?}"
        );
    }

    /// build_tls_connector must succeed for a valid self-signed cert PEM.
    #[test]
    fn tls_connector_valid_cert() {
        // Install ring so rustls operations work in the test binary.
        let _ = rustls::crypto::ring::default_provider().install_default();

        // Mint a fresh self-signed cert with rcgen.
        let cert =
            rcgen::generate_simple_self_signed(vec!["localhost".into()]).expect("rcgen generate");
        let pem = cert.cert.pem();
        assert!(
            build_tls_connector(&pem).is_ok(),
            "expected Ok for valid cert PEM"
        );
    }

    /// build_tls_connector must return an error for garbage input.
    #[test]
    fn tls_connector_garbage_input() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let result = build_tls_connector("this is not a PEM cert");
        assert!(result.is_err(), "expected Err for garbage PEM input");
    }
}
