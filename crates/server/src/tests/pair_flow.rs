use base64::{engine::general_purpose::STANDARD as B64, Engine};

const TOKEN: &str = "secret";

fn valid_pubkey_b64() -> String {
    B64.encode([7u8; 32])
}

#[tokio::test]
async fn pair_success_then_409_then_wrong_token_401() {
    let addr = fleetwatch_server::test_support::spawn_unpaired(TOKEN.into()).await;
    let client = reqwest::Client::new();
    let base = format!("http://{addr}");

    // 1. First POST with correct token + valid 32-byte pubkey → 200 {"ok":true}
    let resp = client
        .post(format!("{base}/api/pair"))
        .json(&serde_json::json!({
            "pairing_token": TOKEN,
            "console_public_key_b64": valid_pubkey_b64(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "expected 200 on first pair");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["ok"], true);

    // 2. Second POST → 409 Conflict (already paired)
    let resp = client
        .post(format!("{base}/api/pair"))
        .json(&serde_json::json!({
            "pairing_token": TOKEN,
            "console_public_key_b64": valid_pubkey_b64(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 409, "expected 409 on second pair");
}

#[tokio::test]
async fn pair_wrong_token_401() {
    let addr = fleetwatch_server::test_support::spawn_unpaired(TOKEN.into()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://{addr}/api/pair"))
        .json(&serde_json::json!({
            "pairing_token": "wrong-token",
            "console_public_key_b64": valid_pubkey_b64(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401, "expected 401 for wrong token");
}

#[tokio::test]
async fn pair_bad_pubkey_400() {
    let addr = fleetwatch_server::test_support::spawn_unpaired(TOKEN.into()).await;
    let client = reqwest::Client::new();

    // 16 bytes — wrong length
    let short_key = B64.encode([1u8; 16]);
    let resp = client
        .post(format!("http://{addr}/api/pair"))
        .json(&serde_json::json!({
            "pairing_token": TOKEN,
            "console_public_key_b64": short_key,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 400, "expected 400 for bad pubkey length");
}
