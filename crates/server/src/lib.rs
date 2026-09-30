pub mod admin;
pub mod agent_ws;
pub mod api;
pub mod auth;
pub mod config;
pub mod conn;
pub mod db;
pub mod dispatch;
pub mod pair;
pub mod registry;
pub mod store;
pub mod sweeper;
pub mod tls;

use axum::{
    middleware,
    routing::{delete, get, patch, post},
    Router,
};
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
        .route("/api/machines/{id}", delete(api::delete_machine))
        .route("/api/machines/{id}/status", get(api::get_machine_status))
        .route(
            "/api/machines/{id}/snapshots",
            get(api::list_machine_snapshots),
        )
        .route(
            "/api/machines/{id}/shell",
            patch(admin::patch_shell_enabled),
        )
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

/// Load the paired public key from SQLite, or persist an explicitly
/// pre-provisioned key from server.toml on first boot.
async fn load_console_pubkey(
    pool: &SqlitePool,
    cfg: &config::ServerConfig,
) -> anyhow::Result<Option<Vec<u8>>> {
    if let Some(b64) = store::get_console_pubkey(pool).await? {
        use base64::{engine::general_purpose::STANDARD as B64, Engine};
        let bytes = B64.decode(b64)?;
        anyhow::ensure!(
            bytes.len() == 32,
            "stored console public key must be 32 bytes"
        );
        return Ok(Some(bytes));
    }
    if cfg.console_public_key_b64.trim().is_empty() {
        return Ok(None);
    }
    let bytes = cfg.console_public_key()?;
    store::set_console_pubkey(pool, cfg.console_public_key_b64.trim()).await?;
    Ok(Some(bytes))
}

pub async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    // Pin the rustls crypto provider so there's no ambiguity if more than one
    // provider is ever linked in. Harmless if already installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cfg = config::ServerConfig::from_env_or_file()?;

    // Ensure TLS cert/key exist before anything else.
    tls::ensure_cert(&cfg.tls_cert_path, &cfg.tls_key_path, &cfg.tls_san)?;

    let pool = db::init_pool(&cfg.db_path).await?;

    let pubkey_bytes = load_console_pubkey(&pool, &cfg).await?;
    // Startup guard: if the server is NOT yet paired, refuse to boot with a
    // weak/default pairing token — otherwise an attacker on the (publicly
    // reachable) /api/pair endpoint could pair their own key and take over the
    // control plane. Already-paired servers ignore the token entirely.
    if pubkey_bytes.is_none() {
        let t = cfg.pairing_token.trim();
        if t == "changeme" || t.len() < 16 {
            anyhow::bail!(
                "server is unpaired and `pairing_token` is weak/default; \
                 set a strong random pairing_token (>= 16 chars) before first pairing"
            );
        }
    }

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
                    let tower_svc = app.into_make_service().call(peer_addr).await.unwrap();
                    let hyper_svc = TowerToHyperService::new(tower_svc);
                    let builder = AutoBuilder::new(TokioExecutor::new());
                    if let Err(e) = builder.serve_connection_with_upgrades(io, hyper_svc).await {
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
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
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
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        addr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD as B64, Engine};

    fn config_with_key(key: String) -> config::ServerConfig {
        config::ServerConfig {
            bind: "127.0.0.1:0".into(),
            console_public_key_b64: key,
            db_path: ":memory:".into(),
            pairing_token: "test-pairing-token".into(),
            tls_cert_path: "cert.pem".into(),
            tls_key_path: "key.pem".into(),
            tls_san: vec![],
        }
    }

    #[tokio::test]
    async fn preprovisioned_key_is_persisted_on_first_boot() {
        let pool = db::init_pool_in_memory().await.unwrap();
        let cfg = config_with_key(B64.encode([7u8; 32]));
        assert_eq!(
            load_console_pubkey(&pool, &cfg).await.unwrap(),
            Some(vec![7u8; 32])
        );
        assert!(store::is_paired(&pool).await.unwrap());
    }

    #[tokio::test]
    async fn paired_database_key_takes_precedence_over_config() {
        let pool = db::init_pool_in_memory().await.unwrap();
        store::set_console_pubkey(&pool, &B64.encode([8u8; 32]))
            .await
            .unwrap();
        let cfg = config_with_key(B64.encode([7u8; 32]));
        assert_eq!(
            load_console_pubkey(&pool, &cfg).await.unwrap(),
            Some(vec![8u8; 32])
        );
    }
}
