mod config;
mod client;
mod collectors;

use clap::{Parser, Subcommand};

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
        #[arg(long)]
        identity: String,
        #[arg(long, default_value = "/etc/fleetwatch/agent.toml")]
        config: String,
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
        Cmd::Enroll { server, identity, config } => {
            let body = format!("server_url = \"{server}\"\nidentity_token = \"{identity}\"\n");
            if let Some(p) = std::path::Path::new(&config).parent() {
                std::fs::create_dir_all(p).ok();
            }
            std::fs::write(&config, body)?;
            println!("Written to {config}");
        }
    }
    Ok(())
}
