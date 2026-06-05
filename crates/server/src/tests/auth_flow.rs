/// Integration test: control-plane signature auth middleware.
///
/// Verifies that:
///   - Unsigned requests to protected routes → 401
///   - Requests with a garbage signature → 401
///   - Correctly signed requests → 200
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chrono::Utc;
use ed25519_dalek::SigningKey;

fn make_server_and_key() -> (SigningKey, String) {
    let sk = SigningKey::from_bytes(&[11u8; 32]);
    let pubkey_b64 = B64.encode(sk.verifying_key().as_bytes());
    (sk, pubkey_b64)
}

fn signed_get(sk: &SigningKey, addr: &str, path: &str) -> reqwest::RequestBuilder {
    let ts = Utc::now().to_rfc3339();
    let sig = fw_proto::auth::sign_request(sk, "GET", path, &ts, b"");
    reqwest::Client::new()
        .get(format!("http://{addr}{path}"))
        .header("x-fw-timestamp", &ts)
        .header("x-fw-signature", &sig)
}

#[tokio::test]
async fn unsigned_request_returns_401() {
    let (sk, pubkey_b64) = make_server_and_key();
    let _ = sk; // not used — server is paired but we send no headers
    let addr = fleetwatch_server::test_support::spawn_test_server(pubkey_b64).await;

    let status = reqwest::get(format!("http://{addr}/api/machines"))
        .await
        .unwrap()
        .status();
    assert_eq!(status.as_u16(), 401, "expected 401 for unsigned request");
}

#[tokio::test]
async fn bad_signature_returns_401() {
    let (sk, pubkey_b64) = make_server_and_key();
    let _ = sk;
    let addr = fleetwatch_server::test_support::spawn_test_server(pubkey_b64).await;

    let ts = Utc::now().to_rfc3339();
    let status = reqwest::Client::new()
        .get(format!("http://{addr}/api/machines"))
        .header("x-fw-timestamp", &ts)
        .header("x-fw-signature", "aGVsbG8=") // garbage sig
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status.as_u16(), 401, "expected 401 for bad signature");
}

#[tokio::test]
async fn correctly_signed_request_returns_200() {
    let (sk, pubkey_b64) = make_server_and_key();
    let addr = fleetwatch_server::test_support::spawn_test_server(pubkey_b64).await;

    let status = signed_get(&sk, &addr, "/api/machines")
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status.as_u16(), 200, "expected 200 for correctly signed request");
}

#[tokio::test]
async fn pair_and_agent_routes_are_unauthenticated() {
    // /api/pair and /agent must be reachable without signatures.
    let (sk, pubkey_b64) = make_server_and_key();
    let _ = sk;
    let addr = fleetwatch_server::test_support::spawn_test_server(pubkey_b64).await;

    // /agent upgrade without proper WS headers returns 400 (not 401)
    let status = reqwest::get(format!("http://{addr}/agent"))
        .await
        .unwrap()
        .status();
    assert_ne!(status.as_u16(), 401, "/agent must not return 401");

    // /api/pair with wrong content returns 422/400 (not 401) because it's not authed
    let status = reqwest::Client::new()
        .post(format!("http://{addr}/api/pair"))
        .json(&serde_json::json!({"pairing_token":"x","console_public_key_b64":"bad"}))
        .send()
        .await
        .unwrap()
        .status();
    // Already paired (server was seeded with a pubkey), so it returns 409 — still not 401
    assert_ne!(status.as_u16(), 401, "/api/pair must not return 401");
}
