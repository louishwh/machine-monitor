use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chrono::Utc;
use ed25519_dalek::SigningKey;
use futures_util::{SinkExt, StreamExt};
use fw_proto::token::{sign_identity, IdentityPayload};
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

fn signed_delete(sk: &SigningKey, addr: &str, path: &str) -> reqwest::RequestBuilder {
    let ts = Utc::now().to_rfc3339();
    let sig = fw_proto::auth::sign_request(sk, "DELETE", path, &ts, b"");
    reqwest::Client::new()
        .delete(format!("http://{addr}{path}"))
        .header("x-fw-timestamp", ts)
        .header("x-fw-signature", sig)
}

#[tokio::test]
async fn agent_hello_marks_machine_online() {
    let sk = SigningKey::from_bytes(&[3u8; 32]);
    let pubkey_b64 = B64.encode(sk.verifying_key().as_bytes());

    // Start server (in-memory db), listening on 127.0.0.1:0
    let addr = fleetwatch_server::test_support::spawn_test_server(pubkey_b64).await;

    // Agent side: sign identity token and connect via ws
    let token = sign_identity(
        &sk,
        &IdentityPayload {
            machine_id: "m-1".into(),
            name: "web-01".into(),
            issued_at: chrono::Utc::now().to_rfc3339(),
        },
    );
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

#[tokio::test]
async fn old_connection_close_keeps_replacement_online() {
    let sk = SigningKey::from_bytes(&[13u8; 32]);
    let addr = fleetwatch_server::test_support::spawn_test_server(
        B64.encode(sk.verifying_key().as_bytes()),
    )
    .await;
    let token = sign_identity(
        &sk,
        &IdentityPayload {
            machine_id: "m-reconnect".into(),
            name: "web-reconnect".into(),
            issued_at: Utc::now().to_rfc3339(),
        },
    );
    let hello = serde_json::json!({
        "type": "hello",
        "identity_token": token,
        "hostname": "web-reconnect",
        "os": "ubuntu",
        "agent_version": "0.1.0"
    });
    let url = format!("ws://{addr}/agent");
    let (mut old, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    old.send(Message::Text(hello.to_string())).await.unwrap();
    old.next().await.unwrap().unwrap();

    let (mut replacement, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    replacement
        .send(Message::Text(hello.to_string()))
        .await
        .unwrap();
    replacement.next().await.unwrap().unwrap();

    let replaced = tokio::time::timeout(std::time::Duration::from_secs(1), old.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .into_text()
        .unwrap();
    assert!(replaced.contains("reject"));
    assert!(replaced.contains("连接被新会话替换"));
    let body: serde_json::Value = signed_get(&sk, &addr, "/api/machines")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body[0]["online"], true);
    replacement.close(None).await.unwrap();
}

#[tokio::test]
async fn deleted_machine_token_cannot_reconnect() {
    let sk = SigningKey::from_bytes(&[23u8; 32]);
    let addr = fleetwatch_server::test_support::spawn_test_server(
        B64.encode(sk.verifying_key().as_bytes()),
    )
    .await;
    let token = sign_identity(
        &sk,
        &IdentityPayload {
            machine_id: "m-deleted".into(),
            name: "deleted".into(),
            issued_at: Utc::now().to_rfc3339(),
        },
    );
    let hello = serde_json::json!({
        "type": "hello",
        "identity_token": token,
        "hostname": "deleted",
        "os": "ubuntu",
        "agent_version": "0.1.0"
    });
    let url = format!("ws://{addr}/agent");
    let (mut first, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    first.send(Message::Text(hello.to_string())).await.unwrap();
    assert!(first
        .next()
        .await
        .unwrap()
        .unwrap()
        .into_text()
        .unwrap()
        .contains("hello_ack"));

    let status = signed_delete(&sk, &addr, "/api/machines/m-deleted")
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status.as_u16(), 200);

    let (mut second, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    second.send(Message::Text(hello.to_string())).await.unwrap();
    let response = second.next().await.unwrap().unwrap().into_text().unwrap();
    assert!(
        response.contains("reject"),
        "old identity should be rejected: {response}"
    );
    let machines: serde_json::Value = signed_get(&sk, &addr, "/api/machines")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(machines.as_array().unwrap().len(), 0);
}
