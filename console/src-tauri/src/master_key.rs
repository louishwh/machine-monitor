//! OS keychain-backed ed25519 master key for the FleetWatch console.
//!
//! Service: `com.fleetwatch.console`
//! User:    `master_seed`
//! Stored as base64-encoded 32-byte seed.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::SigningKey;
use keyring::Entry;
use rand_core::{OsRng, RngCore};
use std::sync::Mutex;

use crate::error::{AppError, AppResult};

const SERVICE: &str = "com.fleetwatch.console";
const SEED_USER: &str = "master_seed";
static GENERATION_LOCK: Mutex<()> = Mutex::new(());

fn entry() -> AppResult<Entry> {
    Ok(Entry::new(SERVICE, SEED_USER)?)
}

/// Returns `true` if a master key seed is present in the OS keychain.
pub fn has_key() -> bool {
    entry().and_then(|e| Ok(e.get_password()?)).is_ok()
}

/// Generate a fresh random 32-byte seed and store it in the OS keychain.
/// Returns an error if a seed already exists (caller should check `has_key` first).
pub fn generate() -> AppResult<()> {
    let _guard = GENERATION_LOCK
        .lock()
        .map_err(|_| AppError::Key("主密钥生成锁不可用".into()))?;
    match entry()?.get_password() {
        Ok(_) => return Err(AppError::Key("主密钥已存在，拒绝覆盖".into())),
        Err(keyring::Error::NoEntry) => {}
        Err(e) => return Err(AppError::Keyring(e.to_string())),
    }
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    let b64 = B64.encode(seed);
    entry()?.set_password(&b64)?;
    Ok(())
}

/// Load the signing key from the OS keychain.
pub fn load() -> AppResult<SigningKey> {
    let b64 = entry()?.get_password().map_err(|e| match e {
        keyring::Error::NoEntry => AppError::NoMasterKey,
        other => AppError::Keyring(other.to_string()),
    })?;
    let bytes = B64.decode(b64.trim()).map_err(|e| AppError::Key(e.to_string()))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| AppError::Key("seed must be 32 bytes".into()))?;
    Ok(SigningKey::from_bytes(&arr))
}

/// Return the base64-encoded public key, or `None` if no key has been generated.
pub fn public_key_b64() -> AppResult<Option<String>> {
    if !has_key() {
        return Ok(None);
    }
    let sk = load()?;
    Ok(Some(B64.encode(sk.verifying_key().as_bytes())))
}

// ──────────────────────────── Tauri commands ────────────────────────────

/// Response type for `master_status`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MasterStatus {
    pub has_key: bool,
    pub public_key_b64: Option<String>,
}

#[tauri::command]
pub fn master_status() -> Result<MasterStatus, crate::error::AppError> {
    let has = has_key();
    let pk = if has { public_key_b64()? } else { None };
    Ok(MasterStatus {
        has_key: has,
        public_key_b64: pk,
    })
}

/// Response type for `generate_master_key`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedKey {
    pub public_key_b64: String,
}

#[tauri::command]
pub fn generate_master_key() -> Result<GeneratedKey, crate::error::AppError> {
    generate()?;
    let sk = load()?;
    Ok(GeneratedKey {
        public_key_b64: B64.encode(sk.verifying_key().as_bytes()),
    })
}

// ──────────────────────────── Tests ────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use fw_proto::token::{sign_identity, verify_identity, IdentityPayload};

    /// Pure unit test: derive a signing key from a fixed seed, sign an identity
    /// payload, and verify it using fw_proto — proves issuance is compatible with
    /// the server's verifier without touching the OS keychain.
    #[test]
    fn issued_token_verifies_with_fw_proto() {
        let seed = [42u8; 32];
        let sk = SigningKey::from_bytes(&seed);
        let vk = sk.verifying_key();

        let payload = IdentityPayload {
            machine_id: "m-test-001".into(),
            name: "test-machine".into(),
            issued_at: "2026-06-02T00:00:00Z".into(),
        };

        let token = sign_identity(&sk, &payload);
        let got = verify_identity(vk.as_bytes(), &token)
            .expect("token should verify with the matching public key");

        assert_eq!(got.machine_id, "m-test-001");
        assert_eq!(got.name, "test-machine");
    }

    #[test]
    fn wrong_key_rejects_token() {
        let sk = SigningKey::from_bytes(&[1u8; 32]);
        let wrong_vk = SigningKey::from_bytes(&[2u8; 32]).verifying_key();

        let payload = IdentityPayload {
            machine_id: "m-x".into(),
            name: "x".into(),
            issued_at: "2026-06-02T00:00:00Z".into(),
        };
        let token = sign_identity(&sk, &payload);
        assert!(verify_identity(wrong_vk.as_bytes(), &token).is_err());
    }
}
