//! fwctl — headless FleetWatch console.
//!
//! The trust root. Holds an ed25519 master key in a local file, pairs with the
//! server (registers the public key), and issues per-machine identity tokens.
//! Mirrors the Tauri console's logic without GUI/keychain so deployment and CI
//! can issue identities and query the control plane.

use anyhow::{Context as _, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use clap::{Parser, Subcommand};
use ed25519_dalek::SigningKey;
use fw_proto::token::{sign_identity, IdentityPayload};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "fwctl", about = "FleetWatch headless console (master key / pair / issue)")]
struct Cli {
    /// Path to the master key file (default: ~/.fleetwatch/master.key)
    #[arg(long, global = true, default_value = "")]
    key: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Generate a new master key (refuses to overwrite an existing one)
    Keygen,
    /// Print the master public key (base64)
    Pubkey,
    /// Register the master public key with the server (one-time)
    Pair {
        #[arg(long)]
        server: String,
        #[arg(long)]
        pairing_token: String,
        /// Path to server CA PEM (for https:// with self-signed cert)
        #[arg(long)]
        server_ca: Option<String>,
    },
    /// Issue a machine identity token (prints token + the enroll command)
    Issue {
        #[arg(long)]
        name: String,
        /// The server URL agents should connect to (for the printed enroll cmd)
        #[arg(long, default_value = "")]
        server: String,
    },
    /// List machines (signed control-plane call)
    List {
        #[arg(long)]
        server: String,
        /// Path to server CA PEM (for https:// with self-signed cert)
        #[arg(long)]
        server_ca: Option<String>,
    },
    /// Pull a machine's status (kind: host|cpu|mem|disk|net|proc|service)
    Status {
        #[arg(long)]
        server: String,
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "host")]
        kind: String,
        /// Path to server CA PEM (for https:// with self-signed cert)
        #[arg(long)]
        server_ca: Option<String>,
    },
}

fn key_path(s: &str) -> PathBuf {
    if !s.is_empty() {
        return PathBuf::from(s);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".fleetwatch").join("master.key")
}

fn load_key(p: &PathBuf) -> Result<SigningKey> {
    let b64 = std::fs::read_to_string(p)
        .with_context(|| format!("读取主密钥失败 {} —— 先运行 `fwctl keygen`", p.display()))?;
    let seed = B64.decode(b64.trim())?;
    let arr: [u8; 32] = seed
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("master key seed 非 32 字节"))?;
    Ok(SigningKey::from_bytes(&arr))
}

fn pubkey_b64(sk: &SigningKey) -> String {
    B64.encode(sk.verifying_key().as_bytes())
}

/// Build a reqwest client.  When `ca_path` is Some, the PEM at that path is
/// added as the sole trusted root (for self-signed server certs).  When None,
/// system roots are used (plain http:// or system-trusted https://).
fn http(ca_path: Option<&str>) -> anyhow::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(40));
    if let Some(path) = ca_path {
        let pem_bytes = std::fs::read(path)
            .with_context(|| format!("failed to read --server-ca file: {path}"))?;
        let cert = reqwest::Certificate::from_pem(&pem_bytes)?;
        builder = builder.add_root_certificate(cert);
    }
    Ok(builder.build()?)
}

/// Signed control-plane GET. `sign_path` is the path WITHOUT query string
/// (the server verifies over `uri.path()`); `query` is appended to the URL only.
async fn signed_get(
    client: &reqwest::Client,
    server: &str,
    sign_path: &str,
    query: &str,
    sk: &SigningKey,
) -> Result<serde_json::Value> {
    let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let sig = fw_proto::auth::sign_request(sk, "GET", sign_path, &ts, b"");
    let url = format!("{}{}{}", server.trim_end_matches('/'), sign_path, query);
    let resp = client
        .get(&url)
        .header("x-fw-timestamp", ts)
        .header("x-fw-signature", sig)
        .send()
        .await?;
    let status = resp.status();
    let body = resp.text().await?;
    anyhow::ensure!(status.is_success(), "GET {sign_path} -> {status}: {body}");
    Ok(serde_json::from_str(&body).unwrap_or(serde_json::Value::String(body)))
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let kp = key_path(&cli.key);

    match cli.cmd {
        Cmd::Keygen => {
            if kp.exists() {
                anyhow::bail!("主密钥已存在：{}（如确需重置，请手动删除该文件）", kp.display());
            }
            if let Some(dir) = kp.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let mut seed = [0u8; 32];
            rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut seed);
            let sk = SigningKey::from_bytes(&seed);
            std::fs::write(&kp, B64.encode(seed))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&kp, std::fs::Permissions::from_mode(0o600)).ok();
            }
            println!("已生成主密钥：{}", kp.display());
            println!("公钥(base64)：{}", pubkey_b64(&sk));
        }
        Cmd::Pubkey => {
            let sk = load_key(&kp)?;
            println!("{}", pubkey_b64(&sk));
        }
        Cmd::Pair {
            server,
            pairing_token,
            server_ca,
        } => {
            let sk = load_key(&kp)?;
            let client = http(server_ca.as_deref())?;
            let body = serde_json::json!({
                "pairing_token": pairing_token,
                "console_public_key_b64": pubkey_b64(&sk),
            });
            let url = format!("{}/api/pair", server.trim_end_matches('/'));
            let resp = client.post(&url).json(&body).send().await?;
            let status = resp.status();
            let text = resp.text().await?;
            anyhow::ensure!(status.is_success(), "配对失败 {status}: {text}");
            println!("配对成功：公钥已注册到 {server}");
        }
        Cmd::Issue { name, server } => {
            let sk = load_key(&kp)?;
            let machine_id = uuid::Uuid::new_v4().to_string();
            let payload = IdentityPayload {
                machine_id: machine_id.clone(),
                name: name.clone(),
                issued_at: chrono::Utc::now().to_rfc3339(),
            };
            let token = sign_identity(&sk, &payload);
            println!("machine_id: {machine_id}");
            println!("name:       {name}");
            println!("token:      {token}");
            let srv = if server.is_empty() { "wss://<server>/agent" } else { &server };
            println!("\n# 在目标机执行：");
            println!("sudo fleetwatch-agent enroll --server {srv} --identity '{token}'");
        }
        Cmd::List { server, server_ca } => {
            let sk = load_key(&kp)?;
            let client = http(server_ca.as_deref())?;
            let v = signed_get(&client, &server, "/api/machines", "", &sk).await?;
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        Cmd::Status { server, id, kind, server_ca } => {
            let sk = load_key(&kp)?;
            let client = http(server_ca.as_deref())?;
            let path = format!("/api/machines/{id}/status");
            let query = format!("?kind={kind}");
            let v = signed_get(&client, &server, &path, &query, &sk).await?;
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
    }
    Ok(())
}
