//! TLS certificate generation and management.
//!
//! Generates a self-signed certificate using `rcgen` and persists it so that
//! clients can trust a stable cert across server restarts.

use std::path::Path;

use anyhow::Context;
use rcgen::{
    CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
    PKCS_ECDSA_P256_SHA256,
};

/// Ensure that a self-signed cert/key pair exists at the given paths.
///
/// - If both files already exist, this is a no-op (stable cert — clients keep trusting it).
/// - Otherwise, generates a new self-signed certificate that includes:
///   - DNS SAN: `localhost`
///   - IP SAN: `127.0.0.1`
///   - Any entries in `extra_sans` — parsed as `IpAddr` → IP SAN, otherwise → DNS SAN.
///
/// The key file is written with mode 0600 on Unix.
pub fn ensure_cert(
    cert_path: impl AsRef<Path>,
    key_path: impl AsRef<Path>,
    extra_sans: &[String],
) -> anyhow::Result<()> {
    let cert_path = cert_path.as_ref();
    let key_path = key_path.as_ref();

    if cert_path.exists() && key_path.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(key_path, std::fs::Permissions::from_mode(0o600))?;
        }
        return Ok(());
    }

    // Collect SANs: always include localhost + 127.0.0.1, then extras.
    // Skip extras that duplicate the defaults.
    let mut san_strings: Vec<String> = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    for entry in extra_sans {
        let entry = entry.trim().to_string();
        if !entry.is_empty() && entry != "localhost" && entry != "127.0.0.1" {
            san_strings.push(entry);
        }
    }

    let key_pair = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).context("generate key pair")?;

    let mut params = CertificateParams::new(san_strings).context("build cert params")?;
    params
        .distinguished_name
        .push(DnType::CommonName, "fleetwatch-server");
    // This is an end-entity TLS server cert, NOT a CA cert.
    // Clients using webpki (reqwest, rustls) reject CA certs as end-entity certs.
    params.is_ca = IsCa::NoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];

    let cert = params
        .self_signed(&key_pair)
        .context("self-sign certificate")?;

    let cert_pem_data = cert.pem();
    let key_pem_data = key_pair.serialize_pem();

    // Create parent directories if needed.
    if let Some(parent) = cert_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create cert dir {}", parent.display()))?;
        }
    }
    if let Some(parent) = key_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create key dir {}", parent.display()))?;
        }
    }

    std::fs::write(cert_path, cert_pem_data.as_bytes())
        .with_context(|| format!("write cert to {}", cert_path.display()))?;

    write_key_file(key_path, key_pem_data.as_bytes())
        .with_context(|| format!("write key to {}", key_path.display()))?;

    tracing::info!(
        cert = %cert_path.display(),
        key = %key_path.display(),
        "generated new self-signed TLS certificate"
    );

    Ok(())
}

/// Read the certificate PEM from disk.
pub fn cert_pem(cert_path: impl AsRef<Path>) -> anyhow::Result<String> {
    let path = cert_path.as_ref();
    std::fs::read_to_string(path).with_context(|| format!("read cert PEM from {}", path.display()))
}

/// Write key file, setting mode 0600 on Unix platforms.
fn write_key_file(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;

    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        file.write_all(data)?;
    }

    #[cfg(not(unix))]
    {
        std::fs::write(path, data)?;
    }

    Ok(())
}

/// Build a rustls `ServerConfig` from PEM cert and key files on disk.
pub fn server_config(
    cert_path: impl AsRef<Path>,
    key_path: impl AsRef<Path>,
) -> anyhow::Result<std::sync::Arc<rustls::ServerConfig>> {
    use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
    use rustls::ServerConfig;

    let cert_pem_data = std::fs::read(cert_path.as_ref())
        .with_context(|| format!("read cert {}", cert_path.as_ref().display()))?;
    let key_pem_data = std::fs::read(key_path.as_ref())
        .with_context(|| format!("read key {}", key_path.as_ref().display()))?;

    let cert_chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(&cert_pem_data)
        .collect::<Result<Vec<_>, _>>()
        .context("parse cert PEM")?;

    let key = PrivateKeyDer::from_pem_slice(&key_pem_data).context("parse key PEM")?;

    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .context("build rustls ServerConfig")?;

    Ok(std::sync::Arc::new(cfg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn ensure_cert_creates_and_is_stable() {
        let dir = TempDir::new().unwrap();
        let cert = dir.path().join("cert.pem");
        let key = dir.path().join("key.pem");

        // First call: should create the files.
        ensure_cert(
            &cert,
            &key,
            &["192.168.1.1".to_string(), "myhost.local".to_string()],
        )
        .expect("first ensure_cert call failed");

        assert!(cert.exists(), "cert file should exist");
        assert!(key.exists(), "key file should exist");

        let cert_pem_1 = std::fs::read_to_string(&cert).unwrap();
        let key_pem_1 = std::fs::read_to_string(&key).unwrap();

        // Sanity: it really is a PEM certificate.
        assert!(
            cert_pem_1.contains("BEGIN CERTIFICATE"),
            "cert PEM should contain BEGIN CERTIFICATE"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
        }

        // Second call: preserve the cert/key contents and tighten key mode.
        ensure_cert(&cert, &key, &[]).expect("second ensure_cert call failed");

        let cert_pem_2 = std::fs::read_to_string(&cert).unwrap();
        let key_pem_2 = std::fs::read_to_string(&key).unwrap();

        assert_eq!(
            cert_pem_1, cert_pem_2,
            "cert should not change on second call"
        );
        assert_eq!(key_pem_1, key_pem_2, "key should not change on second call");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&key).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
