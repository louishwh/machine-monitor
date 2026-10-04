mod client;
mod collectors;
mod config;

use anyhow::Context as _;
use clap::{Parser, Subcommand};
use std::io::Read as _;

#[derive(Parser)]
#[command(name = "fleetwatch-agent", version)]
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
        #[arg(long, conflicts_with_all = ["identity", "identity_file"])]
        identity_prompt: bool,
        /// Read the identity token from a file for unattended installation.
        #[arg(long, conflicts_with_all = ["identity", "identity_prompt"])]
        identity_file: Option<String>,
        /// Verify the server accepts this identity before saving configuration.
        #[arg(long)]
        check: bool,
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
    let _ = rustls::crypto::ring::default_provider().install_default();
    match Cli::parse().cmd {
        Cmd::Run { config } => {
            let cfg = config::AgentConfig::load(&config)?;
            client::run_loop(cfg).await;
        }
        Cmd::Enroll {
            server,
            identity,
            identity_prompt,
            identity_file,
            check,
            config,
            server_ca,
        } => {
            let identity = if identity_prompt {
                rpassword::prompt_password("Machine identity token: ")?
                    .trim()
                    .to_string()
            } else if let Some(path) = identity_file {
                let mut value = String::new();
                std::fs::File::open(&path)
                    .with_context(|| format!("failed to read identity file: {path}"))?
                    .take(65_537)
                    .read_to_string(&mut value)?;
                anyhow::ensure!(value.len() <= 65_536, "identity file exceeds 64 KiB");
                value.trim().to_string()
            } else {
                identity.ok_or_else(|| {
                    anyhow::anyhow!("provide --identity-prompt (recommended) or --identity-file")
                })?
            };
            let server_ca_pem = server_ca
                .map(|path| {
                    std::fs::read_to_string(&path)
                        .with_context(|| format!("failed to read --server-ca file: {path}"))
                })
                .transpose()?;
            let cfg = config::AgentConfig {
                server_url: server,
                identity_token: identity,
                server_ca_pem,
            };
            cfg.validate()?;
            if check {
                client::verify_connection(&cfg).await?;
                println!("Server accepted this machine's identity.");
            }
            cfg.save(&config)?;
            println!("Written to {config}");
        }
    }
    Ok(())
}
