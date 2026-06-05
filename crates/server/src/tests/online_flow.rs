use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chrono::Utc;
use ed25519_dalek::SigningKey;
use fw_proto::token::{sign_identity, IdentityPayload};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

/// Build a signed GET request for control-plane endpoints.
fn signed_get(sk: &SigningKey, addr: &str, path: &str) -> reqwest::RequestBuilder {
    let ts = Utc::now().to_rfc3339();
    let sig = fw_proto::auth::sign_request(sk, "GET", path, &ts, b"");
    reqwest::Client::new()
        .get(format!("http://{addr}{path}"))
        .header("x-fw-timestamp", &ts)
        .header("x-fw-signature", &sig)
}

#[tokio::test]
async fn agent_hello_marks_machine_online() {
    let sk = SigningKey::from_bytes(&[3u8; 32]);
    let pubkey_b64 = B64.encode(sk.verifying_key().as_bytes());

    // Start server (in-memory db), listening on 127.0.0.1:0
    let addr = fleetwatch_server::test_support::spawn_test_server(pubkey_b64).await;

    // Agent side: sign identity token and connect via ws
    let token = sign_identity(&sk, &IdentityPayload {
        machine_id: "m-1".into(),
        name: "web-01".into(),
        issued_at: chrono::Utc::now().to_rfc3339(),
    });
    let url = format!("ws://{addr}/agent");
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let hello = serde_json::json!({
        "type": "hello",
        "identity_token": token,
        "hostname": "web-01",
        "os": "ubuntu",
        "agent_version": "0.1.0"
    });
    ws.send(Message::Text(hello.to_string())).await.unwrap();

    // Read HelloAck
    let reply = ws.next().await.unwrap().unwrap();
    assert!(reply.into_text().unwrap().contains("hello_ack"));

    // Query /api/machines — must be signed
    let body: serde_json::Value = signed_get(&sk, &addr, "/api/machines")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["online"], true);
}
