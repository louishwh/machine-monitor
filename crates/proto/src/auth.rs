use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

fn canonical(method: &str, path: &str, ts: &str, body: &[u8]) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(body);
    let body_hex = format!("{:x}", h.finalize());
    format!("{method}\n{path}\n{ts}\n{body_hex}").into_bytes()
}

pub fn sign_request(sk: &SigningKey, method: &str, path: &str, ts: &str, body: &[u8]) -> String {
    B64.encode(sk.sign(&canonical(method, path, ts, body)).to_bytes())
}

pub fn verify_request(
    pubkey: &[u8],
    method: &str,
    path: &str,
    ts: &str,
    body: &[u8],
    sig_b64: &str,
) -> Result<(), String> {
    let key: [u8; 32] = pubkey.try_into().map_err(|_| "bad key".to_string())?;
    let vk = VerifyingKey::from_bytes(&key).map_err(|e| e.to_string())?;
    let sig_bytes = B64.decode(sig_b64).map_err(|e| e.to_string())?;
    let sig = Signature::from_slice(&sig_bytes).map_err(|e| e.to_string())?;
    vk.verify_strict(&canonical(method, path, ts, body), &sig)
        .map_err(|_| "sig".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    #[test]
    fn sign_verify_request() {
        let sk = SigningKey::from_bytes(&[5u8; 32]);
        let vk = sk.verifying_key();
        let ts = "2026-06-02T00:00:00Z";
        let sig = sign_request(&sk, "GET", "/api/machines", ts, b"");
        assert!(verify_request(vk.as_bytes(), "GET", "/api/machines", ts, b"", &sig).is_ok());
        assert!(verify_request(vk.as_bytes(), "POST", "/api/machines", ts, b"", &sig).is_err());
    }
}
