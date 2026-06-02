//! Signed HTTP client for the FleetWatch control plane.
//!
//! All control-plane GETs include:
//!   `x-fw-timestamp`  — RFC 3339 timestamp
//!   `x-fw-signature`  — base64 ed25519 signature over canonical(method|path|ts|body_hash)
//!
//! `signed_get` always signs the **path without the query string** so the
//! server middleware can verify using `uri.path()`.

use chrono::Utc;
use ed25519_dalek::SigningKey;
use serde_json::Value;

use crate::error::{AppError, AppResult};

/// `POST {server}/api/pair` with `{pairing_token, console_public_key_b64}`.
pub async fn pair(server: &str, pairing_token: &str, console_pubkey_b64: &str) -> AppResult<()> {
    let url = format!("{server}/api/pair");
    let body = serde_json::json!({
        "pairing_token": pairing_token,
        "console_public_key_b64": console_pubkey_b64,
    });

    let client = reqwest::Client::new();
    let resp = client.post(&url).json(&body).send().await?;

    if resp.status().is_success() {
        Ok(())
    } else {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        Err(AppError::ServerError { status, body })
    }
}

/// Perform a signed GET request.
///
/// The signature is computed over the **path** only (no query string),
/// matching what the server middleware verifies via `uri.path()`.
///
/// `path` should be the full path including any query string, e.g.
/// `/api/machines/abc/status?kind=host`.  The function splits the query off
/// before signing so the header carries only the signed path component.
pub async fn signed_get(server: &str, path: &str, sk: &SigningKey) -> AppResult<Value> {
    // Split path from query string; sign only the path component.
    let (sign_path, _query) = path.split_once('?').unwrap_or((path, ""));

    let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let sig = fw_proto::auth::sign_request(sk, "GET", sign_path, &ts, b"");

    let url = format!("{server}{path}");
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("x-fw-timestamp", &ts)
        .header("x-fw-signature", &sig)
        .send()
        .await?;

    if resp.status().is_success() {
        Ok(resp.json::<Value>().await?)
    } else {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        Err(AppError::ServerError { status, body })
    }
}
