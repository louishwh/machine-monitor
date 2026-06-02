//! Integration test: TLS serving and cert generation.
//!
//! Verifies that:
//!   - `ensure_cert` generates a self-signed certificate with correct SANs
//!   - The server can be bound with that cert using rustls
//!   - A reqwest client that trusts only that cert can reach GET /health → 200 "ok"

use std::net::SocketAddr;

use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as AutoBuilder;
use hyper_util::service::TowerToHyperService;
use tempfile::TempDir;
use tokio_rustls::TlsAcceptor;
use tower::Service;

/// Spawn a minimal TLS server on an ephemeral port; return the bound address.
async fn spawn_tls_server(router: axum::Router, cert_path: &str, key_path: &str) -> SocketAddr {
    let tls_cfg = fleetwatch_server::tls::server_config(cert_path, key_path).unwrap();
    let acceptor = TlsAcceptor::from(tls_cfg);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let Ok((tcp_stream, peer_addr)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            let router = router.clone();
            tokio::spawn(async move {
                if let Ok(tls_stream) = acceptor.accept(tcp_stream).await {
                    let io = TokioIo::new(tls_stream);
                    let tower_svc = router
                        .into_make_service()
                        .call(peer_addr)
                        .await
                        .unwrap();
                    let hyper_svc = TowerToHyperService::new(tower_svc);
                    let _ = AutoBuilder::new(TokioExecutor::new())
                        .serve_connection_with_upgrades(io, hyper_svc)
                        .await;
                }
            });
        }
    });

    addr
}

/// Install ring as the default rustls CryptoProvider for this test process.
/// This is required when multiple providers (ring + aws-lc-rs) are compiled in
/// via transitive dependencies (e.g. reqwest pulls both).
fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[tokio::test]
async fn tls_health_endpoint_works() {
    install_crypto_provider();

    let dir = TempDir::new().unwrap();
    let cert_path = dir.path().join("cert.pem");
    let key_path = dir.path().join("key.pem");

    // Generate cert with localhost + 127.0.0.1 SANs (defaults).
    fleetwatch_server::tls::ensure_cert(&cert_path, &key_path, &[])
        .expect("ensure_cert failed");

    let cert_pem_str = fleetwatch_server::tls::cert_pem(&cert_path).unwrap();
    let cert_pem_bytes = cert_pem_str.as_bytes();

    // Build a minimal router with just /health.
    let router = axum::Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }));

    let addr = spawn_tls_server(
        router,
        cert_path.to_str().unwrap(),
        key_path.to_str().unwrap(),
    )
    .await;

    // Build a reqwest client that trusts ONLY our self-signed cert.
    let root_cert = reqwest::Certificate::from_pem(cert_pem_bytes)
        .expect("parse cert PEM for reqwest");
    let client = reqwest::Client::builder()
        .add_root_certificate(root_cert)
        .build()
        .unwrap();

    let url = format!("https://127.0.0.1:{}/health", addr.port());
    let resp = client.get(&url).send().await.expect("GET /health failed");
    assert_eq!(resp.status().as_u16(), 200, "expected 200 from /health");

    let body = resp.text().await.unwrap();
    assert_eq!(body, "ok", "expected body 'ok'");
}

#[tokio::test]
async fn tls_rejects_untrusted_client() {
    install_crypto_provider();

    let dir = TempDir::new().unwrap();
    let cert_path = dir.path().join("cert.pem");
    let key_path = dir.path().join("key.pem");

    fleetwatch_server::tls::ensure_cert(&cert_path, &key_path, &[])
        .expect("ensure_cert failed");

    let router = axum::Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }));

    let addr = spawn_tls_server(
        router,
        cert_path.to_str().unwrap(),
        key_path.to_str().unwrap(),
    )
    .await;

    // A client that does NOT trust our cert should fail.
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(false)
        .build()
        .unwrap();

    let url = format!("https://127.0.0.1:{}/health", addr.port());
    let result = client.get(&url).send().await;
    assert!(result.is_err(), "expected TLS verification failure");
}
