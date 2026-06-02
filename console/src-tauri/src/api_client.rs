//! Signed HTTP client for the FleetWatch control plane.
//!
//! All control-plane GETs include:
//!   `x-fw-timestamp`  — RFC 3339 timestamp
//!   `x-fw-signature`  — base64 ed25519 signature over canonical(method|path|ts|body_hash)
//!
//! `signed_get` always signs the **path without the query string** so the
//! server middleware can verify using `uri.path()`.
//!
//! `signed_post` / `signed_patch` serialise the body JSON **once**, sign over
//! those exact bytes, and send those exact bytes as the request body — so that
//! signature input matches wire bytes.

use chrono::Utc;
use ed25519_dalek::SigningKey;
use serde_json::Value;

use crate::error::{AppError, AppResult};

/// Build a reqwest client.
///
/// When `ca_pem` is `Some`, the PEM-encoded certificate is added as a trusted
/// root so that HTTPS connections to a self-signed server succeed.  When
/// `None`, the system trust store is used (plain http:// or system-trusted
/// https:// both work without a CA).
pub fn build_client(ca_pem: Option<&str>) -> AppResult<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(40));
    if let Some(pem) = ca_pem {
        let cert = reqwest::Certificate::from_pem(pem.as_bytes())
            .map_err(|e| AppError::Other(format!("无效的 CA PEM: {e}")))?;
        builder = builder.add_root_certificate(cert);
    }
    builder
        .build()
        .map_err(|e| AppError::Other(format!("无法构建 HTTP 客户端: {e}")))
}

/// `POST {server}/api/pair` with `{pairing_token, console_public_key_b64}`.
pub async fn pair(
    server: &str,
    pairing_token: &str,
    console_pubkey_b64: &str,
    ca_pem: Option<&str>,
) -> AppResult<()> {
    let url = format!("{server}/api/pair");
    let body = serde_json::json!({
        "pairing_token": pairing_token,
        "console_public_key_b64": console_pubkey_b64,
    });

    let client = build_client(ca_pem)?;
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
pub async fn signed_get(
    server: &str,
    path: &str,
    sk: &SigningKey,
    ca_pem: Option<&str>,
) -> AppResult<Value> {
    // Split path from query string; sign only the path component.
    let (sign_path, _query) = path.split_once('?').unwrap_or((path, ""));

    let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let sig = fw_proto::auth::sign_request(sk, "GET", sign_path, &ts, b"");

    let url = format!("{server}{path}");
    let client = build_client(ca_pem)?;
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

/// Perform a signed POST request with a JSON body.
///
/// The body is serialized to a `String` exactly once; the signature is
/// computed over those bytes, and those same bytes are sent as the wire body.
/// This guarantees the signature input always matches what the server reads.
pub async fn signed_post(
    server: &str,
    path: &str,
    body: &Value,
    sk: &SigningKey,
    ca_pem: Option<&str>,
) -> AppResult<Value> {
    let body_str = serde_json::to_string(body)
        .map_err(|e| AppError::Other(e.to_string()))?;
    let body_bytes = body_str.as_bytes();

    let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let sig = fw_proto::auth::sign_request(sk, "POST", path, &ts, body_bytes);

    let url = format!("{server}{path}");
    let client = build_client(ca_pem)?;
    let resp = client
        .post(&url)
        .header("x-fw-timestamp", &ts)
        .header("x-fw-signature", &sig)
        .header("content-type", "application/json")
        .body(body_str)
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

/// Perform a signed PATCH request with a JSON body.
///
/// Same serialization-once guarantee as `signed_post`.
pub async fn signed_patch(
    server: &str,
    path: &str,
    body: &Value,
    sk: &SigningKey,
    ca_pem: Option<&str>,
) -> AppResult<Value> {
    let body_str = serde_json::to_string(body)
        .map_err(|e| AppError::Other(e.to_string()))?;
    let body_bytes = body_str.as_bytes();

    let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let sig = fw_proto::auth::sign_request(sk, "PATCH", path, &ts, body_bytes);

    let url = format!("{server}{path}");
    let client = build_client(ca_pem)?;
    let resp = client
        .patch(&url)
        .header("x-fw-timestamp", &ts)
        .header("x-fw-signature", &sig)
        .header("content-type", "application/json")
        .body(body_str)
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
