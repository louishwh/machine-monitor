use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityPayload {
    pub machine_id: String,
    pub name: String,
    pub issued_at: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("格式错误")]
    Format,
    #[error("base64 解码失败")]
    B64,
    #[error("签名验证失败")]
    Sig,
    #[error("payload 解析失败")]
    Payload,
    #[error("公钥无效")]
    Key,
}

/// token = base64(payload_json).base64(sig)
pub fn sign_identity(sk: &SigningKey, payload: &IdentityPayload) -> String {
    let bytes = serde_json::to_vec(payload).expect("serialize payload");
    let sig = sk.sign(&bytes);
    format!("{}.{}", B64.encode(&bytes), B64.encode(sig.to_bytes()))
}

pub fn verify_identity(pubkey: &[u8], token: &str) -> Result<IdentityPayload, TokenError> {
    let (p_b64, s_b64) = token.split_once('.').ok_or(TokenError::Format)?;
    let p = B64.decode(p_b64.trim()).map_err(|_| TokenError::B64)?;
    let s = B64.decode(s_b64.trim()).map_err(|_| TokenError::B64)?;
    let key_arr: [u8; 32] = pubkey.try_into().map_err(|_| TokenError::Key)?;
    let vk = VerifyingKey::from_bytes(&key_arr).map_err(|_| TokenError::Key)?;
    let sig = Signature::from_slice(&s).map_err(|_| TokenError::Sig)?;
    vk.verify_strict(&p, &sig).map_err(|_| TokenError::Sig)?;
    serde_json::from_slice(&p).map_err(|_| TokenError::Payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    #[test]
    fn sign_then_verify_roundtrip() {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let vk = sk.verifying_key();
        let payload = IdentityPayload {
            machine_id: "m-1".into(),
            name: "web-01".into(),
            issued_at: "2026-05-30T00:00:00Z".into(),
        };
        let token = sign_identity(&sk, &payload);
        let got = verify_identity(vk.as_bytes(), &token).expect("verify ok");
        assert_eq!(got.machine_id, "m-1");
    }

    #[test]
    fn tampered_token_is_rejected() {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let vk = sk.verifying_key();
        let payload = IdentityPayload {
            machine_id: "m-1".into(),
            name: "x".into(),
            issued_at: "t".into(),
        };
        let mut token = sign_identity(&sk, &payload);
        token.push('A'); // corrupt signature
        assert!(verify_identity(vk.as_bytes(), &token).is_err());
    }
}
