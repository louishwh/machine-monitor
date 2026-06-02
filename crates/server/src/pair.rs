use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};

use crate::{store, AppState};

#[derive(Deserialize)]
pub struct PairRequest {
    pub pairing_token: String,
    pub console_public_key_b64: String,
}

#[derive(Serialize)]
pub struct PairResponse {
    pub ok: bool,
}

/// POST /api/pair
///
/// One-time console pairing. Accepts a pairing token and the console's
/// Ed25519 public key (base64). Rejected if already paired or token wrong.
pub async fn pair(
    State(st): State<AppState>,
    Json(body): Json<PairRequest>,
) -> Result<Json<PairResponse>, (StatusCode, String)> {
    // Already paired?
    let paired = store::is_paired(&st.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if paired {
        return Err((StatusCode::CONFLICT, "already paired".into()));
    }

    // Correct token?
    if body.pairing_token != *st.pairing_token {
        return Err((StatusCode::UNAUTHORIZED, "invalid pairing token".into()));
    }

    // Decode and validate pubkey length.
    let decoded = B64
        .decode(&body.console_public_key_b64)
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid base64".into()))?;
    if decoded.len() != 32 {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("public key must be 32 bytes, got {}", decoded.len()),
        ));
    }

    // Persist to DB.
    store::set_console_pubkey(&st.pool, &body.console_public_key_b64)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Update the in-memory shared RwLock so auth middleware picks it up immediately.
    *st.console_pubkey.write().await = Some(decoded);

    Ok(Json(PairResponse { ok: true }))
}
