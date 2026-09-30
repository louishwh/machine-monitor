/// Integration test: shell toggle + run-shell + audit + revoke, end-to-end.
///
/// Setup:
///   1. Spin up a test server (in-memory SQLite, pre-paired).
///   2. Connect a "fake agent" that performs the Hello handshake and then loops,
///      responding to RunShell messages with a canned CommandResult.
///   3. Assert shell-disabled → 403, then enable → run-shell → 200 with stdout "hi".
///   4. Assert GET /api/audit contains the shell entry.
///   5. POST revoke → assert the fake agent's WS gets closed / Reject frame.
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chrono::Utc;
use ed25519_dalek::SigningKey;
use futures_util::{SinkExt, StreamExt};
use fw_proto::token::{sign_identity, IdentityPayload};
use tokio_tungstenite::tungstenite::Message;

const MACHINE_ID: &str = "m-shell-flow";

/// Build a signed GET request.
fn signed_get(sk: &SigningKey, addr: &str, path: &str, query: &str) -> reqwest::RequestBuilder {
    let ts = Utc::now().to_rfc3339();
    let request_target = if query.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{query}")
    };
    let sig = fw_proto::auth::sign_request(sk, "GET", &request_target, &ts, b"");
    let url = if query.is_empty() {
        format!("http://{addr}{path}")
    } else {
        format!("http://{addr}{path}?{query}")
    };
    reqwest::Client::new()
        .get(url)
        .header("x-fw-timestamp", &ts)
        .header("x-fw-signature", &sig)
}

/// Build a signed POST request.
/// The body JSON is serialized once and used both for the signature and for
/// the actual request body so they match exactly.
fn signed_post(
    sk: &SigningKey,
    addr: &str,
    path: &str,
    body: serde_json::Value,
) -> reqwest::RequestBuilder {
    let body_str = serde_json::to_string(&body).unwrap();
    let body_bytes = body_str.as_bytes();
    let ts = Utc::now().to_rfc3339();
    let sig = fw_proto::auth::sign_request(sk, "POST", path, &ts, body_bytes);
    reqwest::Client::new()
        .post(format!("http://{addr}{path}"))
        .header("x-fw-timestamp", &ts)
        .header("x-fw-signature", &sig)
        .header("content-type", "application/json")
        .body(body_str)
}

/// Build a signed PATCH request.
fn signed_patch(
    sk: &SigningKey,
    addr: &str,
    path: &str,
    body: serde_json::Value,
) -> reqwest::RequestBuilder {
    let body_str = serde_json::to_string(&body).unwrap();
    let body_bytes = body_str.as_bytes();
    let ts = Utc::now().to_rfc3339();
    let sig = fw_proto::auth::sign_request(sk, "PATCH", path, &ts, body_bytes);
    reqwest::Client::new()
        .patch(format!("http://{addr}{path}"))
        .header("x-fw-timestamp", &ts)
        .header("x-fw-signature", &sig)
        .header("content-type", "application/json")
        .body(body_str)
}

#[tokio::test]
async fn shell_flow_full() {
    // --- keying ---
    let sk = SigningKey::from_bytes(&[55u8; 32]);
    let pubkey_b64 = B64.encode(sk.verifying_key().as_bytes());

    // --- start server ---
    let addr = fleetwatch_server::test_support::spawn_test_server(pubkey_b64).await;

    // --- connect fake agent ---
    let token = sign_identity(
        &sk,
        &IdentityPayload {
            machine_id: MACHINE_ID.into(),
            name: "shell-box".into(),
            issued_at: Utc::now().to_rfc3339(),
        },
    );
    let ws_url = format!("ws://{addr}/agent");
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();

    // Send Hello
    let hello = serde_json::json!({
        "type": "hello",
        "identity_token": token,
        "hostname": "shell-box",
        "os": "ubuntu",
        "agent_version": "0.1.0"
    });
    ws.send(Message::Text(hello.to_string())).await.unwrap();

    // Wait for HelloAck
    let ack = ws.next().await.unwrap().unwrap();
    assert!(
        ack.into_text().unwrap().contains("hello_ack"),
        "expected hello_ack"
    );

    // Give the server a moment to finish registering the connection.
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // ── Test 1: run-shell with shell DISABLED → 403 ──────────────────────────
    let shell_path = format!("/api/machines/{MACHINE_ID}/shell");
    let run_path = format!("/api/machines/{MACHINE_ID}/run-shell");

    let status = signed_post(
        &sk,
        &addr,
        &run_path,
        serde_json::json!({"command": "echo hi"}),
    )
    .send()
    .await
    .unwrap()
    .status();
    assert_eq!(status.as_u16(), 403, "expected 403 when shell is disabled");

    // ── Test 2: enable shell → run-shell → 200 with stdout "hi" ─────────────
    let patch_resp = signed_patch(
        &sk,
        &addr,
        &shell_path,
        serde_json::json!({"enabled": true}),
    )
    .send()
    .await
    .unwrap();
    assert_eq!(
        patch_resp.status().as_u16(),
        200,
        "expected 200 from PATCH shell"
    );

    // Spawn a task that drives the fake-agent side: read RunShell, reply CommandResult.
    // We need the ws to stay alive across the revoke test as well, so we use a
    // tokio oneshot to signal the agent task to stop only after we've done the run.
    let (agent_done_tx, mut agent_done_rx) = tokio::sync::mpsc::channel::<()>(1);

    let agent_task = tokio::spawn(async move {
        // Loop: handle RunShell messages and then detect close/Reject.
        loop {
            let raw = match ws.next().await {
                Some(Ok(m)) => m,
                _ => break, // closed or error
            };
            let text = match raw.into_text() {
                Ok(t) => t,
                Err(_) => continue,
            };
            // Check for Reject (revoke) — our cue that the connection was kicked.
            if text.contains("reject") {
                break;
            }
            let v: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if v["type"] == "run_shell" {
                let cmd_id = v["cmd_id"].as_str().unwrap_or("").to_string();
                let reply = serde_json::json!({
                    "type": "command_result",
                    "cmd_id": cmd_id,
                    "exit": 0,
                    "stdout": "hi",
                    "stderr": "",
                    "done": true
                });
                if ws.send(Message::Text(reply.to_string())).await.is_err() {
                    break;
                }
                // Notify that we've handled at least one RunShell.
                let _ = agent_done_tx.try_send(());
            }
        }
    });

    // Now fire run-shell — shell is enabled.
    let resp: serde_json::Value = signed_post(
        &sk,
        &addr,
        &run_path,
        serde_json::json!({"command": "echo hi"}),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    assert_eq!(
        resp["stdout"].as_str().unwrap_or(""),
        "hi",
        "run-shell response: {resp}"
    );
    assert_eq!(resp["exit"], 0);

    // Wait for the agent task to have handled the RunShell (with timeout).
    tokio::time::timeout(tokio::time::Duration::from_secs(5), agent_done_rx.recv())
        .await
        .expect("agent task did not handle RunShell in time");

    // ── Test 3: GET /api/audit contains the shell entry ──────────────────────
    let audit_path = "/api/audit";
    let audit: serde_json::Value =
        signed_get(&sk, &addr, audit_path, &format!("machine_id={MACHINE_ID}"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

    let arr = audit.as_array().expect("audit should be an array");
    assert!(!arr.is_empty(), "audit should contain at least one entry");
    let first = &arr[0];
    assert_eq!(first["kind"], "shell", "audit entry kind should be 'shell'");
    assert_eq!(first["machine_id"], MACHINE_ID);
    assert!(
        first["request"].as_str().unwrap_or("").contains("echo hi"),
        "audit entry request should contain 'echo hi'"
    );

    // ── Test 4: POST revoke → agent WS receives Reject / gets closed ─────────
    let revoke_path = format!("/api/machines/{MACHINE_ID}/revoke");
    let revoke_resp = signed_post(&sk, &addr, &revoke_path, serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        revoke_resp.status().as_u16(),
        200,
        "expected 200 from revoke"
    );

    // The agent task should terminate (ws closed or Reject received) within 2 s.
    tokio::time::timeout(tokio::time::Duration::from_secs(2), agent_task)
        .await
        .expect("agent task did not terminate after revoke within timeout")
        .expect("agent task panicked");

    // A failed dispatch is still recorded as an attempted shell command.
    let failed_status = signed_post(
        &sk,
        &addr,
        &run_path,
        serde_json::json!({"command":"echo after revoke"}),
    )
    .send()
    .await
    .unwrap()
    .status();
    assert_eq!(failed_status.as_u16(), 502);
    let audit: serde_json::Value =
        signed_get(&sk, &addr, audit_path, &format!("machine_id={MACHINE_ID}"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    assert_eq!(audit.as_array().unwrap().len(), 2);
    assert_eq!(audit[0]["request"], "echo after revoke");
    assert!(audit[0]["exit"].is_null());
}

/// Ensure unsigned requests to admin endpoints are rejected.
#[tokio::test]
async fn admin_unsigned_returns_401() {
    let sk = SigningKey::from_bytes(&[56u8; 32]);
    let pubkey_b64 = B64.encode(sk.verifying_key().as_bytes());
    let addr = fleetwatch_server::test_support::spawn_test_server(pubkey_b64).await;

    // PATCH /shell without signature headers.
    let status = reqwest::Client::new()
        .patch(format!("http://{addr}/api/machines/ghost/shell"))
        .header("content-type", "application/json")
        .body(r#"{"enabled":true}"#)
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status.as_u16(), 401, "expected 401 without signature");

    // POST /run-shell without signature headers.
    let status = reqwest::Client::new()
        .post(format!("http://{addr}/api/machines/ghost/run-shell"))
        .header("content-type", "application/json")
        .body(r#"{"command":"echo hi"}"#)
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status.as_u16(), 401, "expected 401 without signature");

    // GET /api/audit without signature headers.
    let status = reqwest::Client::new()
        .get(format!("http://{addr}/api/audit"))
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status.as_u16(), 401, "expected 401 without signature");
}
