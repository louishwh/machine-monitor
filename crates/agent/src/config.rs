use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use std::io::Write as _;

#[derive(Clone, Deserialize, Serialize)]
pub struct AgentConfig {
    pub server_url: String,
    pub identity_token: String,
    /// Inline PEM of the server's self-signed TLS certificate.
    /// When present together with a `wss://` URL the agent pins this cert as
    /// the only trusted root (no system roots consulted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_ca_pem: Option<String>,
}

impl AgentConfig {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(&std::fs::read_to_string(path)?)?)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        let url = url::Url::parse(&self.server_url).context("invalid server URL")?;
        anyhow::ensure!(
            matches!(url.scheme(), "ws" | "wss"),
            "server URL must use ws:// or wss://"
        );
        anyhow::ensure!(url.host_str().is_some(), "server URL needs a hostname");
        anyhow::ensure!(
            url.username().is_empty() && url.password().is_none(),
            "server URL must not contain credentials"
        );
        anyhow::ensure!(
            url.fragment().is_none(),
            "server URL must not contain a fragment"
        );
        anyhow::ensure!(
            !self.identity_token.trim().is_empty(),
            "identity token must not be empty"
        );
        if let Some(pem) = &self.server_ca_pem {
            crate::client::build_tls_connector(pem)?;
        }
        Ok(())
    }

    /// Serialize rather than interpolating untrusted input into TOML. Persist
    /// atomically so failed enrollment never truncates a working configuration.
    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        self.validate()?;
        let path = std::path::Path::new(path);
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        std::fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(toml::to_string(self)?.as_bytes())?;
        file.as_file().sync_all()?;
        file.persist(path)
            .with_context(|| format!("failed to save {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_toml() {
        let toml = r#"
            server_url = "wss://mon.example.com/agent"
            identity_token = "abc.def"
        "#;
        let c: AgentConfig = toml::from_str(toml).unwrap();
        assert_eq!(c.server_url, "wss://mon.example.com/agent");
        assert_eq!(c.identity_token, "abc.def");
    }

    #[test]
    fn enrollment_serializes_and_replaces_private_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent.toml");
        std::fs::write(&path, "old config").unwrap();
        let cfg = AgentConfig {
            server_url: "wss://mon.example.com/agent".into(),
            identity_token: "token\"with\\escapes\nand newline".into(),
            server_ca_pem: None,
        };
        cfg.save(path.to_str().unwrap()).unwrap();
        let loaded = AgentConfig::load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.identity_token, cfg.identity_token);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let invalid = AgentConfig {
            server_url: "https://mon.example.com".into(),
            ..cfg
        };
        assert!(invalid.save(path.to_str().unwrap()).is_err());
        assert_eq!(
            AgentConfig::load(path.to_str().unwrap())
                .unwrap()
                .identity_token,
            loaded.identity_token
        );
    }
}
