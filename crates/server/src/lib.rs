pub mod config;
pub mod db;
pub mod store;
pub mod registry;
pub mod agent_ws;
pub mod api;
pub mod admin;
pub mod pair;
pub mod auth;
pub mod sweeper;
pub mod conn;
pub mod dispatch;
pub mod tls;

use axum::{middleware, routing::{get, patch, post}, Router};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Health check handler — not behind auth.
async fn health() -> &'static str {
    "ok"
}

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub registry: registry::Registry,
    pub console_pubkey: Arc<RwLock<Option<Vec<u8>>>>,
    pub conns: conn::Conns,
    pub pairing_token: Arc<String>,
}

pub fn build_router(state: AppState) -> Router {
    // Build the console-signature auth middleware layer.
    let sig_layer = middleware::from_fn_with_state(state.clone(), auth::require_console_sig);

    // Control-plane routes — layered with signature auth before merging.
    // The sub-router gets its own with_state so that route-scoped layers
    // (added via .layer before .with_state) stay bound to these routes only
    // when the two routers are merged in axum 0.8.
    let protected = Router::new()
        .route("/api/machines", get(api::list_machines))
        .route("/api/machines/{id}/status", get(api::get_machine_status))
        .route("/api/machines/{id}/snapshots", get(api::list_machine_snapshots))
        .route("/api/machines/{id}/shell", patch(admin::patch_shell_enabled))
        .route("/api/machines/{id}/run-shell", post(admin::post_run_shell))
        .route("/api/machines/{id}/revoke", post(admin::post_revoke))
        .route("/api/audit", get(admin::get_audit))
        .layer(sig_layer)
        .with_state(state.clone());

    // Unauthenticated routes — agent WebSocket, one-time pairing, and health.
    Router::new()
        .route("/agent", get(agent_ws::handler))
        .route("/api/pair", post(pair::pair))
        .route("/health", get(health))
        .with_state(state)
        .merge(protected)
}

pub async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cfg = config::ServerConfig::from_env_or_file()?;

    // Ensure TLS cert/key exist before anything else.
    tls::ensure_cert(&cfg.tls_cert_path, &cfg.tls_key_path, &cfg.tls_san)?;

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

    serve_tls(app, &cfg.bind, &cfg.tls_cert_path, &cfg.tls_key_path).await
}

/// Serve the given `app` over TLS on `bind_addr` using the cert/key PEM files.
///
/// Uses tokio-rustls with a manual hyper accept loop because axum-server 0.7
/// requires hyper 0.14 which conflicts with axum 0.8 / hyper 1.
pub async fn serve_tls(
    app: Router,
    bind_addr: &str,
    cert_path: &str,
    key_path: &str,
) -> anyhow::Result<()> {
    use hyper_util::rt::{TokioExecutor, TokioIo};
    use hyper_util::server::conn::auto::Builder as AutoBuilder;
    use hyper_util::service::TowerToHyperService;
    use tokio_rustls::TlsAcceptor;
    use tower::Service;

    let tls_cfg = tls::server_config(cert_path, key_path)?;
    let acceptor = TlsAcceptor::from(tls_cfg);

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!(bind = %bind_addr, "fleetwatch-server listening (TLS)");

    loop {
        let (tcp_stream, peer_addr) = listener.accept().await?;
        let acceptor = acceptor.clone();
        let app = app.clone();

        tokio::spawn(async move {
            match acceptor.accept(tcp_stream).await {
                Ok(tls_stream) => {
                    let io = TokioIo::new(tls_stream);
                    let tower_svc = app
                        .into_make_service()
                        .call(peer_addr)
                        .await
                        .unwrap();
                    let hyper_svc = TowerToHyperService::new(tower_svc);
                    let builder = AutoBuilder::new(TokioExecutor::new());
                    if let Err(e) = builder
                        .serve_connection_with_upgrades(io, hyper_svc)
                        .await
                    {
                        tracing::debug!(peer = %peer_addr, err = %e, "connection error");
                    }
                }
                Err(e) => {
                    tracing::debug!(peer = %peer_addr, err = %e, "TLS handshake failed");
                }
            }
        });
    }
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
