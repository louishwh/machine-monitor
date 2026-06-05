use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};
use chrono::Utc;

use crate::AppState;

/// Axum middleware: verifies that the incoming request carries a valid
/// Ed25519 signature from the paired console.
///
/// Required headers:
///   x-fw-timestamp  — RFC3339 timestamp (must be within ±300 s of now)
///   x-fw-signature  — base64-encoded Ed25519 signature
///
/// Signed canonical form: `method\npath\ntimestamp\nSHA256(body)`
/// (path = `uri.path()`, i.e. without the query string)
pub async fn require_console_sig(
    State(st): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // Extract headers before consuming the request.
    let ts_str = req
        .headers()
        .get("x-fw-timestamp")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?
        .to_string();

    let sig_str = req
        .headers()
        .get("x-fw-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?
        .to_string();

    // Validate timestamp freshness (±300 s).
    let ts = chrono::DateTime::parse_from_rfc3339(&ts_str).map_err(|_| StatusCode::UNAUTHORIZED)?;
    let skew = (Utc::now() - ts.with_timezone(&Utc))
        .num_seconds()
        .unsigned_abs();
    if skew > 300 {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Read the shared pubkey — None means not yet paired.
    let pubkey: Vec<u8> = {
        let guard = st.console_pubkey.read().await;
        guard.clone().ok_or(StatusCode::UNAUTHORIZED)?
    };

    // Buffer the body so we can include it in the canonical string.
    let (parts, body) = req.into_parts();
    let bytes = axum::body::to_bytes(body, 1 << 20)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let method = parts.method.as_str();
    let path = parts.uri.path(); // does NOT include query string

    fw_proto::auth::verify_request(&pubkey, method, path, &ts_str, &bytes, &sig_str)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // Reconstruct the request with the buffered body and pass through.
    let req = Request::from_parts(parts, Body::from(bytes));
    Ok(next.run(req).await)
}
