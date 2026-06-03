//! Minimal signed control-plane client (read-only). Mirrors fwctl's signing so
//! the ops agent authenticates to the FleetWatch server the same way.

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::SigningKey;
use std::path::Path;

use crate::health::Machine;

pub struct Client {
    http: reqwest::Client,
    server: String,
    key: SigningKey,
}

impl Client {
    pub fn new(server: &str, key_path: &Path, ca_path: Option<&str>) -> Result<Self> {
        let b64 = std::fs::read_to_string(key_path).with_context(|| {
            format!("read operator key {} (run `fwctl keygen` / `fwctl pair` first)", key_path.display())
        })?;
        let seed = B64.decode(b64.trim())?;
        let arr: [u8; 32] = seed
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("operator key seed is not 32 bytes"))?;
        let key = SigningKey::from_bytes(&arr);

        let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(40));
        if let Some(path) = ca_path {
            let pem = std::fs::read(path).with_context(|| format!("read --server-ca {path}"))?;
            builder = builder.add_root_certificate(reqwest::Certificate::from_pem(&pem)?);
        }
        Ok(Self {
            http: builder.build()?,
            server: server.trim_end_matches('/').to_string(),
            key,
        })
    }

    async fn signed_get(&self, sign_path: &str, query: &str) -> Result<serde_json::Value> {
        let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let sig = fw_proto::auth::sign_request(&self.key, "GET", sign_path, &ts, b"");
        let url = format!("{}{}{}", self.server, sign_path, query);
        let resp = self
            .http
            .get(&url)
            .header("x-fw-timestamp", ts)
            .header("x-fw-signature", sig)
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await?;
        anyhow::ensure!(status.is_success(), "GET {sign_path} -> {status}: {body}");
        Ok(serde_json::from_str(&body)?)
    }

    pub async fn list_machines(&self) -> Result<Vec<Machine>> {
        let v = self.signed_get("/api/machines", "").await?;
        Ok(serde_json::from_value(v)?)
    }
}
