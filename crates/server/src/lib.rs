pub mod config;
pub mod db;
pub mod store;
pub mod registry;
pub mod agent_ws;
pub mod api;
pub mod pair;
pub mod sweeper;
pub mod conn;
pub mod dispatch;

use axum::{routing::{get, post}, Router};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub registry: registry::Registry,
    pub console_pubkey: Arc<RwLock<Option<Vec<u8>>>>,
    pub conns: conn::Conns,
    pub pairing_token: Arc<String>,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/agent", get(agent_ws::handler))
        .route("/api/pair", post(pair::pair))
        .route("/api/machines", get(api::list_machines))
        .route("/api/machines/{id}/status", get(api::get_machine_status))
        .route("/api/machines/{id}/snapshots", get(api::list_machine_snapshots))
        .with_state(state)
}

pub async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cfg = config::ServerConfig::from_env_or_file()?;
    let pool = db::init_pool(&cfg.db_path).await?;

    // Load console public key from DB (None if not yet paired).
    let pubkey_bytes = if let Some(b64) = store::get_console_pubkey(&pool).await? {
        use base64::{engine::general_purpose::STANDARD as B64, Engine};
        Some(B64.decode(b64)?)
    } else {
        None
    };
    let console_pubkey = Arc::new(RwLock::new(pubkey_bytes));
    let pairing_token = Arc::new(cfg.pairing_token.clone());

    let state = AppState {
        pool,
        registry: registry::Registry::new(),
        console_pubkey,
        conns: conn::Conns::new(),
        pairing_token,
    };
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

    /// Spin up a test server pre-seeded with the given console public key
    /// (base64-encoded). Existing M1/M2 integration tests pass this so that
    /// the server starts in "paired" state and agent identity verification works.
    /// Both the in-memory RwLock and the DB are seeded so `is_paired()` returns true.
    pub async fn spawn_test_server(pubkey_b64: String) -> String {
        let pool = db::init_pool_in_memory().await.unwrap();
        // Seed the DB so store::is_paired() returns true.
        store::set_console_pubkey(&pool, &pubkey_b64).await.unwrap();
        let decoded = B64.decode(&pubkey_b64).unwrap();
        let console_pubkey = Arc::new(RwLock::new(Some(decoded)));
        let pairing_token = Arc::new("unused-in-paired-mode".to_string());
        let state = AppState {
            pool,
            registry: registry::Registry::new(),
            console_pubkey,
            conns: conn::Conns::new(),
            pairing_token,
        };
        let app = build_router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        addr
    }

    /// Spin up an unpaired test server (no console pubkey seeded).
    /// The given `pairing_token` is the secret that must be presented to `/api/pair`.
    pub async fn spawn_unpaired(pairing_token: String) -> String {
        let pool = db::init_pool_in_memory().await.unwrap();
        let console_pubkey = Arc::new(RwLock::new(None));
        let pairing_token = Arc::new(pairing_token);
        let state = AppState {
            pool,
            registry: registry::Registry::new(),
            console_pubkey,
            conns: conn::Conns::new(),
            pairing_token,
        };
        let app = build_router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        addr
    }
}
