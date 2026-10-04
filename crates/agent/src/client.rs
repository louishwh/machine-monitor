use crate::config::AgentConfig;
use anyhow::Context as _;
use futures_util::{SinkExt, StreamExt};
use fw_proto::messages::{AgentToServer, ServerToAgent};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio_tungstenite::tungstenite::Message;

const MAX_SHELL_OUTPUT_BYTES: usize = 1024 * 1024;

/// Bound each output stream so a command cannot exhaust agent memory or send
/// an unbounded WebSocket frame back to the server.
async fn read_limited<R: AsyncRead + Unpin>(mut reader: R) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            return Ok(output);
        }
        if output.len() + n > MAX_SHELL_OUTPUT_BYTES {
            return Err(std::io::Error::other(
                "shell output exceeded 1 MiB per stream",
            ));
        }
        output.extend_from_slice(&buf[..n]);
    }
}

/// Kill the command's Unix process group if execution is timed out, errors,
/// or its task is cancelled. The direct child is still explicitly reaped.
struct ProcessGroupGuard {
    #[cfg(unix)]
    pgid: Option<i32>,
}

impl ProcessGroupGuard {
    fn new(pid: Option<u32>) -> Self {
        #[cfg(unix)]
        {
            Self {
                pgid: pid.and_then(|id| i32::try_from(id).ok()),
            }
        }
        #[cfg(not(unix))]
        {
            let _ = pid;
            Self {}
        }
    }

    fn disarm(&mut self) {
        #[cfg(unix)]
        {
            self.pgid = None;
        }
    }

    fn kill(&mut self) {
        #[cfg(unix)]
        if let Some(pgid) = self.pgid.take() {
            // Negative PID targets the process group created with process_group(0).
            unsafe { libc::kill(-pgid, libc::SIGKILL) };
        }
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Execute `command` via `sh -c`, capturing stdout/stderr/exit.
/// If the process does not finish within `timeout`, it is killed (and reaped)
/// and `(-1, "", "timeout")` is returned.
pub async fn run_shell(command: &str, timeout: Duration) -> (i32, String, String) {
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
        // Belt-and-braces: tokio does NOT kill a dropped Child by default, so
        // any path that drops a live handle would leak the process as an
        // orphan. The timeout branch below also kills explicitly so the child
        // is reaped before this function returns.
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(unix)]
    builder.process_group(0);
    if std::path::Path::new(&home).is_dir() {
        builder.current_dir(&home);
    }

    let mut child = match builder.spawn() {
        Ok(c) => c,
        Err(e) => return (-1, String::new(), format!("spawn error: {e}")),
    };

    let mut group = ProcessGroupGuard::new(child.id());
    let mut stdout_handle = child.stdout.take().unwrap();
    let mut stderr_handle = child.stderr.take().unwrap();

    // Drain both pipes concurrently; otherwise a full stderr pipe can block
    // the child while the agent is still waiting for stdout to reach EOF.
    // The timeout covers both I/O and waiting for process exit.
    let execution = async {
        let (stdout_bytes, stderr_bytes) = tokio::try_join!(
            read_limited(&mut stdout_handle),
            read_limited(&mut stderr_handle)
        )?;
        let status = child.wait().await?;
        Ok::<_, std::io::Error>((status, stdout_bytes, stderr_bytes))
    };

    match tokio::time::timeout(timeout, execution).await {
        Ok(Ok((status, out, err))) => {
            group.disarm();
            (
                status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out).into_owned(),
                String::from_utf8_lossy(&err).into_owned(),
            )
        }
        outcome => {
            group.kill();
            let _ = child.start_kill();
            let _ = child.wait().await;
            let reason = match outcome {
                Ok(Err(e)) => e.to_string(),
                Err(_) => "timeout".to_string(),
                Ok(Ok(_)) => unreachable!(),
            };
            (-1, String::new(), reason)
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
    use rustls::pki_types::{pem::PemObject, CertificateDer};
    use rustls::RootCertStore;
    use tokio_tungstenite::Connector;

    let mut roots = RootCertStore::empty();
    let certs: Vec<_> =
        CertificateDer::pem_slice_iter(ca_pem.as_bytes()).collect::<Result<Vec<_>, _>>()?;
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

/// A session that stayed up at least this long counts as healthy, and its
/// clean close may reset the reconnect backoff. The server closes the socket
/// immediately after rejecting an agent, so the clean close of a *fresh*
/// connection must not be mistaken for a healthy one — otherwise a rejected
/// agent would reconnect at full speed forever.
const MIN_HEALTHY_SESSION: Duration = Duration::from_secs(60);

/// Fixed reconnect backoff after an explicit server `Reject`. The cause
/// (revoked/invalid identity, unpaired server) only changes when the operator
/// acts, so retrying at the normal cadence would keep a TLS-handshake storm
/// alive on both ends indefinitely.
const REJECT_BACKOFF_SECS: u64 = 300;

/// Reconnect backoff for the next attempt, given the previous backoff, how the
/// session ended, and how long it stayed up.
fn next_backoff_after(prev: u64, end: &SessionEnd, elapsed: Duration) -> u64 {
    match end {
        SessionEnd::Rejected(_) => REJECT_BACKOFF_SECS,
        SessionEnd::Closed if elapsed >= MIN_HEALTHY_SESSION => 1,
        SessionEnd::Closed => next_backoff(prev),
    }
}

pub async fn run_loop(cfg: AgentConfig) {
    // Install the ring crypto provider once; ignore error if already installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut backoff = 1u64;
    loop {
        let started = Instant::now();
        let end = match connect_once(&cfg, Duration::from_secs(15)).await {
            Ok(SessionEnd::Rejected(reason)) => {
                tracing::warn!(
                    %reason,
                    backoff_secs = REJECT_BACKOFF_SECS,
                    "server rejected this agent's identity"
                );
                SessionEnd::Rejected(reason)
            }
            Ok(end) => end,
            Err(e) => {
                tracing::warn!(error=%e, "agent session ended");
                SessionEnd::Closed
            }
        };
        backoff = next_backoff_after(backoff, &end, started.elapsed());
        tokio::time::sleep(Duration::from_secs(backoff)).await;
    }
}

/// How one session (one WebSocket connection) ended. Drives the reconnect
/// backoff in `run_loop`.
#[derive(Debug)]
enum SessionEnd {
    /// Socket closed without an explicit server rejection (dial error,
    /// network drop, server restart, …).
    Closed,
    /// The server rejected this identity — invalid token, revoked machine, or
    /// an unpaired server. Retrying cannot succeed until the operator acts.
    Rejected(String),
}

/// The session's write half, shared between the loop and the per-command
/// tasks so results can be sent without blocking the read loop.
type Sink = Arc<
    tokio::sync::Mutex<
        futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            Message,
        >,
    >,
>;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Use the same TLS and Hello path for enrollment checks and normal sessions.
async fn open_session(cfg: &AgentConfig) -> anyhow::Result<Socket> {
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
    Ok(ws)
}

pub async fn verify_connection(cfg: &AgentConfig) -> anyhow::Result<()> {
    verify_connection_with_timeout(cfg, Duration::from_secs(15)).await
}

async fn verify_connection_with_timeout(
    cfg: &AgentConfig,
    timeout: Duration,
) -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    tokio::time::timeout(timeout, async {
        let mut ws = open_session(cfg).await?;
        while let Some(frame) = ws.next().await {
            match frame? {
                Message::Text(text) => match serde_json::from_str::<ServerToAgent>(&text)? {
                    ServerToAgent::HelloAck { ok: true } => {
                        let _ = ws.close(None).await;
                        return Ok(());
                    }
                    ServerToAgent::HelloAck { ok: false } => {
                        anyhow::bail!("server declined this machine's identity")
                    }
                    ServerToAgent::Reject { reason } => {
                        anyhow::bail!("server rejected enrollment: {reason}")
                    }
                    _ => {}
                },
                Message::Close(_) => break,
                _ => {}
            }
        }
        anyhow::bail!("server closed the connection before accepting this identity")
    })
    .await
    .context("server enrollment check timed out after 15 seconds")?
}

async fn connect_once(
    cfg: &AgentConfig,
    heartbeat_interval: Duration,
) -> anyhow::Result<SessionEnd> {
    let ws = open_session(cfg).await?;

    // Split the socket so command handlers run as independent tasks: a 30s
    // shell used to block this loop, stalling heartbeats and delaying
    // detection of a revocation kick.
    let (sink, mut stream) = ws.split();
    let sink: Sink = Arc::new(tokio::sync::Mutex::new(sink));

    let mut hb = tokio::time::interval(heartbeat_interval);
    hb.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut commands = tokio::task::JoinSet::new();
    let end = loop {
        tokio::select! {
            // Slow collectors can leave a heartbeat tick overdue. Read pending
            // server messages first so rejection is not delayed by catch-up work.
            biased;
            msg = stream.next() => {
                let Some(msg) = msg else { break SessionEnd::Closed };
                let msg = match msg {
                    Ok(msg) => msg,
                    Err(e) => {
                        tracing::debug!(error=%e, "agent socket read failed");
                        break SessionEnd::Closed;
                    }
                };
                let Message::Text(txt) = msg else { continue };
                match serde_json::from_str::<ServerToAgent>(&txt) {
                    Ok(ServerToAgent::Ping) => { /* keepalive */ }
                    Ok(ServerToAgent::RunStatus { cmd_id, kind, arg }) => {
                        let sink = Arc::clone(&sink);
                        let cmd = cmd_id.clone();
                        commands.spawn(async move {
                            let result = match tokio::task::spawn_blocking(move || build_status_result(cmd, kind, arg)).await {
                                Ok(result) => result,
                                Err(e) => AgentToServer::CommandResult {
                                    cmd_id,
                                    exit: -1,
                                    stdout: String::new(),
                                    stderr: format!("collector task failed: {e}"),
                                    done: true,
                                },
                            };
                            if let Err(e) = send_msg(&sink, &result).await {
                                tracing::debug!(error=%e, "session closed before status result was sent");
                            }
                        });
                    }
                    Ok(ServerToAgent::RunShell { cmd_id, command }) => {
                        let sink = Arc::clone(&sink);
                        commands.spawn(async move {
                            let (exit, stdout, stderr) =
                                run_shell(&command, Duration::from_secs(30)).await;
                            let result = AgentToServer::CommandResult {
                                cmd_id,
                                exit,
                                stdout,
                                stderr,
                                done: true,
                            };
                            if let Err(e) = send_msg(&sink, &result).await {
                                tracing::debug!(error=%e, "session closed before shell result was sent");
                            }
                        });
                    }
                    Ok(ServerToAgent::HelloAck { ok: false }) => {
                        break SessionEnd::Rejected("server declined agent handshake".into());
                    }
                    Ok(ServerToAgent::HelloAck { ok: true }) => {
                        tracing::info!("server accepted this agent's identity");
                    }
                    Ok(ServerToAgent::Reject { reason }) => break SessionEnd::Rejected(reason),
                    Err(_) => {}
                }
            }
            finished = commands.join_next(), if !commands.is_empty() => {
                if let Some(Err(e)) = finished {
                    tracing::warn!(error=%e, "agent command task failed");
                }
            }
            _ = hb.tick() => {
                let summary = tokio::task::spawn_blocking(crate::collectors::collect_summary)
                    .await
                    .ok();
                let m = AgentToServer::Heartbeat { summary };
                if let Err(e) = send_msg(&sink, &m).await {
                    tracing::debug!(error=%e, "heartbeat send failed");
                    break SessionEnd::Closed;
                }
            }
        }
    };
    // A disconnected or revoked session must not leave old shell commands
    // running after the next connection starts. Dropping run_shell kills its
    // process group through ProcessGroupGuard.
    commands.abort_all();
    while commands.join_next().await.is_some() {}
    Ok(end)
}

/// Send one message over the shared sink (heartbeat, command results).
async fn send_msg(sink: &Sink, msg: &AgentToServer) -> anyhow::Result<()> {
    let json = serde_json::to_string(msg)?;
    sink.lock().await.send(Message::Text(json)).await?;
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

    async fn probe_server(
        reply: Option<ServerToAgent>,
    ) -> (AgentConfig, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let cfg = AgentConfig {
            server_url: format!("ws://{}/agent", listener.local_addr().unwrap()),
            identity_token: "test-identity".into(),
            server_ca_pem: None,
        };
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let hello = ws.next().await.unwrap().unwrap().into_text().unwrap();
            assert!(
                matches!(serde_json::from_str::<AgentToServer>(&hello).unwrap(), AgentToServer::Hello { identity_token, .. } if identity_token == "test-identity")
            );
            if let Some(reply) = reply {
                ws.send(Message::Text(serde_json::to_string(&reply).unwrap()))
                    .await
                    .unwrap();
            }
            while let Some(Ok(frame)) = ws.next().await {
                if matches!(frame, Message::Close(_)) {
                    break;
                }
            }
        });
        (cfg, server)
    }

    #[tokio::test]
    async fn enrollment_check_waits_for_identity_acceptance() {
        let (cfg, server) = probe_server(Some(ServerToAgent::HelloAck { ok: true })).await;
        verify_connection(&cfg).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn enrollment_check_rejects_invalid_identity() {
        let (cfg, server) = probe_server(Some(ServerToAgent::Reject {
            reason: "revoked".into(),
        }))
        .await;
        assert!(verify_connection(&cfg)
            .await
            .unwrap_err()
            .to_string()
            .contains("revoked"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn enrollment_check_has_a_handshake_deadline() {
        let (cfg, server) = probe_server(None).await;
        assert!(
            verify_connection_with_timeout(&cfg, Duration::from_millis(100))
                .await
                .unwrap_err()
                .to_string()
                .contains("timed out")
        );
        server.await.unwrap();
    }

    #[cfg(unix)]
    async fn wait_for_pid(path: &std::path::Path) -> u32 {
        for _ in 0..50 {
            if let Ok(text) = std::fs::read_to_string(path) {
                return text.trim().parse().unwrap();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("command did not write its child PID");
    }

    #[cfg(unix)]
    async fn assert_not_running(pid: u32) {
        for _ in 0..50 {
            let output = std::process::Command::new("ps")
                .args(["-o", "stat=", "-p", &pid.to_string()])
                .output()
                .unwrap();
            let state = String::from_utf8_lossy(&output.stdout);
            if state.trim().is_empty() || state.trim().starts_with('Z') {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("command child {pid} was still running after cancellation");
    }
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
    async fn session_heartbeats_during_shell_and_stops_on_reject() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/agent", listener.local_addr().unwrap());
        let config = AgentConfig {
            server_url: url,
            identity_token: "test-identity".into(),
            server_ca_pem: None,
        };
        let agent = tokio::spawn(async move {
            connect_once(&config, Duration::from_millis(100))
                .await
                .unwrap()
        });
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        let hello = ws.next().await.unwrap().unwrap().into_text().unwrap();
        assert!(matches!(
            serde_json::from_str::<AgentToServer>(&hello).unwrap(),
            AgentToServer::Hello { .. }
        ));
        ws.send(Message::Text(
            serde_json::to_string(&ServerToAgent::HelloAck { ok: true }).unwrap(),
        ))
        .await
        .unwrap();
        ws.send(Message::Text(
            serde_json::to_string(&ServerToAgent::RunShell {
                cmd_id: "long-command".into(),
                command: "sleep 5".into(),
            })
            .unwrap(),
        ))
        .await
        .unwrap();

        let mut heartbeats = 0;
        while heartbeats < 2 {
            let frame = tokio::time::timeout(Duration::from_secs(5), ws.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .into_text()
                .unwrap();
            match serde_json::from_str::<AgentToServer>(&frame).unwrap() {
                AgentToServer::Heartbeat { .. } => heartbeats += 1,
                AgentToServer::CommandResult { .. } => {
                    panic!("shell result arrived before the second heartbeat")
                }
                AgentToServer::Hello { .. } => panic!("unexpected second hello"),
            }
        }
        ws.send(Message::Text(
            serde_json::to_string(&ServerToAgent::Reject {
                reason: "test rejection".into(),
            })
            .unwrap(),
        ))
        .await
        .unwrap();
        let end = tokio::time::timeout(Duration::from_secs(2), agent)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(end, SessionEnd::Rejected(reason) if reason == "test rejection"));
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

    #[tokio::test]
    async fn run_shell_drains_both_output_streams() {
        // More than a pipe buffer on stderr must not block stdout collection.
        let (exit, stdout, stderr) = run_shell(
            "head -c 131072 /dev/zero >&2; printf done",
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(exit, 0, "{stderr:?}");
        assert_eq!(stdout, "done");
        assert_eq!(stderr.len(), 131072);
    }

    #[tokio::test]
    async fn run_shell_timeout_includes_wait_after_pipes_close() {
        let started = Instant::now();
        let (exit, _, stderr) =
            run_shell("exec 1>&- 2>&-; sleep 5", Duration::from_millis(150)).await;
        assert_eq!(exit, -1);
        assert_eq!(stderr, "timeout");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn run_shell_bounds_output() {
        let (exit, _, stderr) =
            run_shell("head -c 1100000 /dev/zero", Duration::from_secs(5)).await;
        assert_eq!(exit, -1);
        assert!(stderr.contains("exceeded 1 MiB"), "{stderr}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_shell_timeout_kills_background_child() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("child.pid");
        let command = format!("sleep 30 & echo $! > {}; wait", pid_file.display());
        let (exit, _, stderr) = run_shell(&command, Duration::from_millis(500)).await;
        assert_eq!(exit, -1);
        assert_eq!(stderr, "timeout");
        assert_not_running(wait_for_pid(&pid_file).await).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancelling_shell_task_kills_background_child() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("child.pid");
        let command = format!("sleep 30 & echo $! > {}; wait", pid_file.display());
        let task = tokio::spawn(async move { run_shell(&command, Duration::from_secs(30)).await });
        let pid = wait_for_pid(&pid_file).await;
        task.abort();
        let _ = task.await;
        assert_not_running(pid).await;
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

    #[test]
    fn backoff_resets_only_after_a_healthy_session() {
        // Long-lived clean close → fast retry again.
        assert_eq!(
            next_backoff_after(30, &SessionEnd::Closed, Duration::from_secs(61)),
            1
        );
        // Fresh connection that dies immediately (e.g. rejected then closed
        // before the Reject frame was read) → keep growing, never reset.
        assert_eq!(
            next_backoff_after(1, &SessionEnd::Closed, Duration::from_secs(2)),
            2
        );
        assert_eq!(
            next_backoff_after(30, &SessionEnd::Closed, Duration::from_secs(2)),
            30
        );
    }

    #[test]
    fn rejection_backs_off_hard() {
        assert_eq!(
            next_backoff_after(1, &SessionEnd::Rejected("已吊销".into()), Duration::ZERO),
            REJECT_BACKOFF_SECS
        );
    }

    /// Regression: a timed-out direct child must be killed and reaped.
    #[cfg(unix)]
    #[tokio::test]
    async fn run_shell_timeout_kills_child() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("child.pid");
        let command = format!("echo $$ > {}; exec sleep 30", pid_file.display());
        let (exit, _, stderr) = run_shell(&command, Duration::from_millis(150)).await;
        assert_eq!(exit, -1);
        assert_eq!(stderr, "timeout");
        assert_not_running(wait_for_pid(&pid_file).await).await;
    }
}
