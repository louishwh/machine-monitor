#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("cert") {
        // `cert` subcommand: ensure cert exists and print it to stdout.
        let cfg = fleetwatch_server::config::ServerConfig::from_env_or_file()?;
        fleetwatch_server::tls::ensure_cert(&cfg.tls_cert_path, &cfg.tls_key_path, &cfg.tls_san)?;
        let pem = fleetwatch_server::tls::cert_pem(&cfg.tls_cert_path)?;
        print!("{pem}");
        return Ok(());
    }
    fleetwatch_server::run().await
}
