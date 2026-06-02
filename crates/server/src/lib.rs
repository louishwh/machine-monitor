pub mod config;
pub mod db;
pub mod store;
pub mod registry;
pub mod agent_ws;
pub mod api;
pub mod sweeper;
pub mod conn;
pub mod dispatch;

use axum::{routing::get, Router};
use sqlx::SqlitePool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub registry: registry::Registry,
    pub console_pubkey: Arc<Vec<u8>>,
    pub conns: conn::Conns,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/agent", get(agent_ws::handler))
        .route("/api/machines", get(api::list_machines))
        .route("/api/machines/{id}/status", get(api::get_machine_status))
        .route("/api/machines/{id}/snapshots", get(api::list_machine_snapshots))
        .with_state(state)
}

pub async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cfg = config::ServerConfig::from_env_or_file()?;
    let pool = db::init_pool(&cfg.db_path).await?;
    let console_pubkey = Arc::new(cfg.console_public_key()?);
    let state = AppState { pool, registry: registry::Registry::new(), console_pubkey, conns: conn::Conns::new() };
    sweeper::spawn(state.clone());
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    tracing::info!(bind = %cfg.bind, "fleetwatch-server listening");
    axum::serve(listener, app).await?;
    Ok(())
}

pub mod test_support {
    use super::*;
    use base64::{engine::general_purpose::STANDARD as B64, Engine};

    pub async fn spawn_test_server(pubkey_b64: String) -> String {
        let pool = db::init_pool_in_memory().await.unwrap();
        let console_pubkey = Arc::new(B64.decode(pubkey_b64).unwrap());
        let state = AppState { pool, registry: registry::Registry::new(), console_pubkey, conns: conn::Conns::new() };
        let app = build_router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        addr
    }
}
