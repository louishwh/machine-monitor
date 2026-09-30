mod client;
mod collectors;
mod config;

use anyhow::Context as _;
use clap::{Parser, Subcommand};
use std::io::Write as _;

#[derive(Parser)]
#[command(name = "fleetwatch-agent")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run with config file (default /etc/fleetwatch/agent.toml)
    Run {
        #[arg(long, default_value = "/etc/fleetwatch/agent.toml")]
        config: String,
    },
    /// Write identity token to config
    Enroll {
        #[arg(long)]
        server: String,
        /// Legacy non-interactive path; visible in process arguments and shell history.
        #[arg(long)]
        identity: Option<String>,
        /// Read the identity token from a hidden terminal prompt.
        #[arg(long, conflicts_with = "identity")]
        identity_prompt: bool,
        #[arg(long, default_value = "/etc/fleetwatch/agent.toml")]
        config: String,
        /// Path to server CA PEM file (for wss:// with self-signed cert).
        /// When provided, the PEM is embedded inline in agent.toml.
        #[arg(long)]
        server_ca: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    match Cli::parse().cmd {
        Cmd::Run { config } => {
            let cfg = config::AgentConfig::load(&config)?;
            client::run_loop(cfg).await;
        }
        Cmd::Enroll {
            server,
            identity,
            identity_prompt,
            config,
            server_ca,
        } => {
            let identity = if identity_prompt {
                rpassword::prompt_password("Machine identity token: ")?
                    .trim()
                    .to_string()
            } else {
                identity.ok_or_else(|| {
                    anyhow::anyhow!("provide --identity-prompt (recommended) or --identity")
                })?
            };
            anyhow::ensure!(!identity.is_empty(), "identity token must not be empty");
            let mut body = format!("server_url = \"{server}\"\nidentity_token = \"{identity}\"\n");
            if let Some(ca_path) = server_ca {
                let pem = std::fs::read_to_string(&ca_path)
                    .with_context(|| format!("failed to read --server-ca file: {ca_path}"))?;
                // TOML literal multi-line string ('''…''') needs no escaping.
                // PEM contains only base64, newlines, and "-----…-----" headers — no '''.
                body.push_str(&format!("server_ca_pem = '''\n{pem}'''\n"));
            }
            let path = std::path::Path::new(&config);
            if let Some(p) = path.parent() {
                if !p.as_os_str().is_empty() {
                    std::fs::create_dir_all(p)?;
                }
            }
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
                options.mode(0o600);
                let mut file = options.open(path)?;
                // Existing enrollment files may have been created with mode 0644.
                // Tighten the opened file before writing the new identity token.
                file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
                file.write_all(body.as_bytes())?;
            }
            #[cfg(not(unix))]
            {
                options.open(path)?.write_all(body.as_bytes())?;
            }
            println!("Written to {config}");
        }
    }
    Ok(())
}
