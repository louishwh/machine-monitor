/// Integration test: end-to-end RunStatus round-trip through a live WebSocket.
///
/// Setup:
///   1. Spin up a test server (in-memory SQLite).
///   2. Connect a fake agent that performs the Hello handshake.
///   3. The fake agent listens for `RunStatus{kind:"host"}` and replies with a
///      canned `CommandResult` containing `{"hostname":"web-01"}`.
///   4. Meanwhile, call `GET /api/machines/:id/status?kind=host` from an HTTP
///      client and assert the response JSON contains `hostname == "web-01"`.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::SigningKey;
use fw_proto::token::{sign_identity, IdentityPayload};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

const MACHINE_ID: &str = "m-status-1";

#[tokio::test]
async fn status_flow_round_trip() {
    // --- keying ---
    let sk = SigningKey::from_bytes(&[42u8; 32]);
    let pubkey_b64 = B64.encode(sk.verifying_key().as_bytes());

    // --- start server ---
    let addr = fleetwatch_server::test_support::spawn_test_server(pubkey_b64).await;

    // --- connect fake agent ---
    let token = sign_identity(
        &sk,
        &IdentityPayload {
            machine_id: MACHINE_ID.into(),
            name: "web-01".into(),
            issued_at: chrono::Utc::now().to_rfc3339(),
        },
    );
    let ws_url = format!("ws://{addr}/agent");
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url).await.unwrap();

    // Send Hello
    let hello = serde_json::json!({
        "type": "hello",
        "identity_token": token,
        "hostname": "web-01",
        "os": "ubuntu",
        "agent_version": "0.1.0"
    });
    ws.send(Message::Text(hello.to_string())).await.unwrap();

    // Expect HelloAck
    let ack = ws.next().await.unwrap().unwrap();
    assert!(
        ack.into_text().unwrap().contains("hello_ack"),
        "expected hello_ack"
    );

    // Spawn a task that acts as the fake agent: receives RunStatus and replies
    // with a canned CommandResult.
    let agent_task = tokio::spawn(async move {
        // Wait for the RunStatus message from the server.
        let raw = ws.next().await.unwrap().unwrap();
        let text = raw.into_text().unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["type"], "run_status", "expected run_status, got {text}");
        let cmd_id = v["cmd_id"].as_str().unwrap().to_string();

        // Reply with a canned CommandResult.
        let reply = serde_json::json!({
            "type": "command_result",
            "cmd_id": cmd_id,
            "exit": 0,
            "stdout": r#"{"hostname":"web-01"}"#,
            "stderr": "",
            "done": true
        });
        ws.send(Message::Text(reply.to_string())).await.unwrap();
    });

    // Give the fake agent a brief moment to be fully registered before we hit
    // the HTTP endpoint (the ws handler registers in the Conns table right after
    // HelloAck is sent, so by the time we reach here it's already done).
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // --- call GET /api/machines/:id/status?kind=host ---
    let resp: serde_json::Value = reqwest::get(format!(
        "http://{addr}/api/machines/{MACHINE_ID}/status?kind=host"
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    assert_eq!(
        resp["hostname"].as_str().unwrap_or(""),
        "web-01",
        "response JSON: {resp}"
    );

    agent_task.await.unwrap();
}

#[tokio::test]
async fn status_unknown_kind_returns_400() {
    let sk = SigningKey::from_bytes(&[7u8; 32]);
    let pubkey_b64 = B64.encode(sk.verifying_key().as_bytes());
    let addr = fleetwatch_server::test_support::spawn_test_server(pubkey_b64).await;

    let status = reqwest::get(format!(
        "http://{addr}/api/machines/anything/status?kind=badkind"
    ))
    .await
    .unwrap()
    .status();
    assert_eq!(status.as_u16(), 400, "expected 400 for unknown kind");
}

#[tokio::test]
async fn status_offline_machine_returns_error() {
    let sk = SigningKey::from_bytes(&[9u8; 32]);
    let pubkey_b64 = B64.encode(sk.verifying_key().as_bytes());
    let addr = fleetwatch_server::test_support::spawn_test_server(pubkey_b64).await;

    let status = reqwest::Client::new()
        .get(format!(
            "http://{addr}/api/machines/ghost/status?kind=host"
        ))
        // Use a short timeout to avoid waiting 30s for the real dispatch timeout.
        // The dispatch should fail immediately (no connection), so this is ample.
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status.as_u16(), 502, "expected 502 for offline machine");
}
