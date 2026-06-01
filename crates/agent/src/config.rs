use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    pub server_url: String,
    pub identity_token: String,
}

impl AgentConfig {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(&std::fs::read_to_string(path)?)?)
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
}
