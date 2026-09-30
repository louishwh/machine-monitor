/// Integration test: end-to-end RunStatus round-trip through a live WebSocket.
///
/// Setup:
///   1. Spin up a test server (in-memory SQLite).
///   2. Connect a fake agent that performs the Hello handshake.
///   3. The fake agent listens for `RunStatus{kind:"host"}` and replies with a
///      canned `CommandResult` containing `{"hostname":"web-01"}`.
///   4. Meanwhile, call `GET /api/machines/:id/status?kind=host` from an HTTP
///      client (signed with the console key) and assert the response JSON
///      contains `hostname == "web-01"`.
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chrono::Utc;
use ed25519_dalek::SigningKey;
use futures_util::{SinkExt, StreamExt};
use fw_proto::token::{sign_identity, IdentityPayload};
use tokio_tungstenite::tungstenite::Message;

const MACHINE_ID: &str = "m-status-1";

/// Build a signed GET request for control-plane endpoints.
/// The path and query must match the request target the server sees.
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

    // --- call GET /api/machines/:id/status?kind=host (signed) ---
    let path = format!("/api/machines/{MACHINE_ID}/status");
    let resp: serde_json::Value = signed_get(&sk, &addr, &path, "kind=host")
        .send()
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

    let path = "/api/machines/anything/status";
    let status = signed_get(&sk, &addr, path, "kind=badkind")
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status.as_u16(), 400, "expected 400 for unknown kind");
}

#[tokio::test]
async fn service_status_without_name_returns_400() {
    let sk = SigningKey::from_bytes(&[17u8; 32]);
    let addr = fleetwatch_server::test_support::spawn_test_server(
        B64.encode(sk.verifying_key().as_bytes()),
    )
    .await;
    let status = signed_get(&sk, &addr, "/api/machines/anything/status", "kind=service")
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status.as_u16(), 400);
}

#[tokio::test]
async fn status_offline_machine_returns_error() {
    let sk = SigningKey::from_bytes(&[9u8; 32]);
    let pubkey_b64 = B64.encode(sk.verifying_key().as_bytes());
    let addr = fleetwatch_server::test_support::spawn_test_server(pubkey_b64).await;

    let path = "/api/machines/ghost/status";
    let status = signed_get(&sk, &addr, path, "kind=host")
        // Use a short timeout to avoid waiting 30s for the real dispatch timeout.
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status.as_u16(), 502, "expected 502 for offline machine");
}
