use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub bind: String,
    /// 管理端公钥（信任锚），首次配对后由 console 注册；M1 先从配置读
    pub console_public_key_b64: String,
    pub db_path: String,
    /// One-time secret token the console must present to complete pairing.
    #[serde(default = "default_pairing_token")]
    pub pairing_token: String,
    /// Path to the TLS certificate PEM file.
    #[serde(default = "default_tls_cert_path")]
    pub tls_cert_path: String,
    /// Path to the TLS private key PEM file.
    #[serde(default = "default_tls_key_path")]
    pub tls_key_path: String,
    /// Additional Subject Alternative Names (DNS names or IP addresses) for the self-signed cert.
    #[serde(default)]
    pub tls_san: Vec<String>,
}

fn default_pairing_token() -> String {
    // Intentionally a placeholder — production deployments must set this explicitly.
    "changeme".into()
}

fn default_tls_cert_path() -> String {
    "/etc/fleetwatch/tls/cert.pem".into()
}

fn default_tls_key_path() -> String {
    "/etc/fleetwatch/tls/key.pem".into()
}

impl ServerConfig {
    pub fn console_public_key(&self) -> anyhow::Result<Vec<u8>> {
        let v = B64.decode(self.console_public_key_b64.trim())?;
        anyhow::ensure!(v.len() == 32, "公钥必须为 32 字节");
        Ok(v)
    }
    pub fn from_env_or_file() -> anyhow::Result<Self> {
        let path = std::env::var("FW_SERVER_CONFIG").unwrap_or_else(|_| "server.toml".into());
        let text = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&text)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_pubkey_from_base64() {
        let cfg = ServerConfig {
            bind: "127.0.0.1:8080".into(),
            console_public_key_b64: base64::engine::general_purpose::STANDARD.encode([9u8; 32]),
            db_path: "/tmp/fw.db".into(),
            pairing_token: "test-token".into(),
            tls_cert_path: default_tls_cert_path(),
            tls_key_path: default_tls_key_path(),
            tls_san: vec![],
        };
        let key = cfg.console_public_key().unwrap();
        assert_eq!(key.len(), 32);
    }
}
