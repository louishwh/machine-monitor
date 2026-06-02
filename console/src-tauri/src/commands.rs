//! Tauri IPC commands for pairing, identity issuance, and the signed control plane.

use chrono::Utc;
use fw_proto::token::{sign_identity, IdentityPayload};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    api_client,
    error::AppError,
    master_key, settings,
};

// ─────────────────────────── pair_server ────────────────────────────────

/// Pair this console with the server using a one-time pairing token.
///
/// Requires a master key to exist; returns an error if none has been generated.
/// Uses the server URL stored via `set_server_url`.
#[tauri::command]
pub async fn pair_server(pairing_token: String) -> Result<(), AppError> {
    let server = settings::require_server_url()?;
    let pk_b64 = master_key::public_key_b64()?.ok_or(AppError::NoMasterKey)?;
    api_client::pair(&server, &pairing_token, &pk_b64).await
}

// ─────────────────────────── issue_machine ──────────────────────────────

/// Response returned by `issue_machine`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuedMachine {
    pub machine_id: String,
    pub name: String,
    pub token: String,
}

/// Sign a machine identity token.  The token can be handed to the machine
/// operator who passes it to `fleetwatch-agent enroll --identity <token>`.
#[tauri::command]
pub fn issue_machine(name: String) -> Result<IssuedMachine, AppError> {
    let sk = master_key::load()?;
    let machine_id = Uuid::new_v4().to_string();
    let issued_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let payload = IdentityPayload {
        machine_id: machine_id.clone(),
        name: name.clone(),
        issued_at,
    };
    let token = sign_identity(&sk, &payload);

    Ok(IssuedMachine {
        machine_id,
        name,
        token,
    })
}

// ─────────────────────────── list_machines ──────────────────────────────

/// `GET /api/machines` — signed with the master key.
#[tauri::command]
pub async fn list_machines() -> Result<Value, AppError> {
    let server = settings::require_server_url()?;
    let sk = master_key::load()?;
    api_client::signed_get(&server, "/api/machines", &sk).await
}

// ─────────────────────────── machine_status ─────────────────────────────

/// `GET /api/machines/{id}/status?kind={kind}` — signed over the **path without query**.
#[tauri::command]
pub async fn machine_status(id: String, kind: String) -> Result<Value, AppError> {
    let server = settings::require_server_url()?;
    let sk = master_key::load()?;
    // Path with query for the actual request; signed_get strips the query before signing.
    let path = format!("/api/machines/{id}/status?kind={kind}");
    api_client::signed_get(&server, &path, &sk).await
}

// ─────────────────────────── machine_snapshots ──────────────────────────

/// `GET /api/machines/{id}/snapshots` — signed with the master key.
#[tauri::command]
pub async fn machine_snapshots(id: String) -> Result<Value, AppError> {
    let server = settings::require_server_url()?;
    let sk = master_key::load()?;
    let path = format!("/api/machines/{id}/snapshots");
    api_client::signed_get(&server, &path, &sk).await
}

// ─────────────────────────── Tests ──────────────────────────────────────

#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use ed25519_dalek::SigningKey;
    use fw_proto::token::{sign_identity, verify_identity, IdentityPayload};
    use uuid::Uuid;

    /// Confirm that `issue_machine` (logic only, without keyring) produces a
    /// token that the server-side verifier accepts.
    #[test]
    fn issue_machine_token_verifies() {
        let seed = [99u8; 32];
        let sk = SigningKey::from_bytes(&seed);
        let vk = sk.verifying_key();

        let machine_id = Uuid::new_v4().to_string();
        let issued_at = "2026-06-02T00:00:00Z";
        let payload = IdentityPayload {
            machine_id: machine_id.clone(),
            name: "prod-server-01".into(),
            issued_at: issued_at.into(),
        };
        let token = sign_identity(&sk, &payload);

        let got = verify_identity(vk.as_bytes(), &token)
            .expect("server verifier should accept a console-issued token");
        assert_eq!(got.machine_id, machine_id);
        assert_eq!(got.name, "prod-server-01");
    }

    /// Verify that the public key round-trips through base64 correctly.
    #[test]
    fn public_key_b64_roundtrip() {
        let seed = [7u8; 32];
        let sk = SigningKey::from_bytes(&seed);
        let pk_bytes = sk.verifying_key().to_bytes();
        let b64 = B64.encode(pk_bytes);
        let decoded = B64.decode(&b64).expect("decode");
        assert_eq!(decoded, pk_bytes);
    }
}
