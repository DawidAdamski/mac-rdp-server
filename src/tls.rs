//! Self-signed certificate management (F-001.6, F-001.7, ADR-0004).
//!
//! This module only produces PEM files on disk. Turning them into a TLS
//! acceptor is `ironrdp-server`'s `TlsIdentityCtx`, and therefore lives in
//! `session` — the only module allowed to touch IronRDP (F-001.12).

use std::path::Path;

use anyhow::{Context as _, Result};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use tracing::info;

/// What [`ensure_self_signed`] had to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertOutcome {
    Generated,
    Loaded,
}

/// Subject alternative names the generated certificate carries (F-001.7).
#[must_use]
pub fn subject_alt_names(hostname: &str) -> Vec<String> {
    let mut names = vec!["localhost".to_owned(), "127.0.0.1".to_owned(), "::1".to_owned()];
    if !hostname.is_empty() && !names.iter().any(|n| n == hostname) {
        names.push(hostname.to_owned());
        // Macs are reachable as `<name>.local` over Bonjour.
        if !hostname.contains('.') {
            names.push(format!("{hostname}.local"));
        }
    }
    names
}

/// Ensures both files exist, generating a self-signed pair if either is missing.
///
/// Existing files are never overwritten: a certificate the user deliberately
/// installed must survive a restart (AC-2).
pub fn ensure_self_signed(cert_path: &Path, key_path: &Path) -> Result<CertOutcome> {
    if cert_path.exists() && key_path.exists() {
        info!(cert = %cert_path.display(), key = %key_path.display(), "loaded existing certificate");
        return Ok(CertOutcome::Loaded);
    }

    let hostname = local_hostname();
    let names = subject_alt_names(&hostname);

    let mut params = CertificateParams::new(names.clone()).context("building certificate parameters")?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, hostname.clone());
    dn.push(DnType::OrganizationName, "mac-rdp-server");
    params.distinguished_name = dn;

    let signing_key = KeyPair::generate().context("generating key pair")?;
    let cert = params.self_signed(&signing_key).context("self-signing certificate")?;

    for path in [cert_path, key_path] {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating `{}`", dir.display()))?;
        }
    }

    std::fs::write(cert_path, cert.pem()).with_context(|| format!("writing `{}`", cert_path.display()))?;
    write_private_key(key_path, &signing_key.serialize_pem())
        .with_context(|| format!("writing `{}`", key_path.display()))?;

    info!(
        cert = %cert_path.display(),
        key = %key_path.display(),
        hostname = %hostname,
        sans = ?names,
        "generated self-signed certificate"
    );
    Ok(CertOutcome::Generated)
}

/// Writes the key with `0600` so it is not world-readable.
fn write_private_key(path: &Path, pem: &str) -> Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(pem.as_bytes())?;
    Ok(())
}

/// Best-effort local hostname; an empty string just means fewer SANs.
fn local_hostname() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mac-rdp-server-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn sans_always_cover_loopback() {
        let names = subject_alt_names("dawids-mac");
        assert!(names.contains(&"localhost".to_owned()));
        assert!(names.contains(&"127.0.0.1".to_owned()));
        assert!(names.contains(&"::1".to_owned()));
        assert!(names.contains(&"dawids-mac".to_owned()));
        assert!(names.contains(&"dawids-mac.local".to_owned()));
    }

    #[test]
    fn sans_do_not_duplicate_a_dotted_hostname() {
        let names = subject_alt_names("mac.lan");
        assert!(names.contains(&"mac.lan".to_owned()));
        assert!(!names.iter().any(|n| n == "mac.lan.local"));
    }

    #[test]
    fn empty_hostname_is_tolerated() {
        let names = subject_alt_names("");
        assert_eq!(names.len(), 3);
    }

    #[test]
    fn generates_once_then_loads() {
        let dir = temp_dir("cert");
        let cert = dir.join("c.pem");
        let key = dir.join("k.pem");

        assert_eq!(
            ensure_self_signed(&cert, &key).expect("first run"),
            CertOutcome::Generated
        );
        assert!(cert.exists() && key.exists());

        let cert_bytes = std::fs::read(&cert).expect("read cert");
        assert_eq!(
            ensure_self_signed(&cert, &key).expect("second run"),
            CertOutcome::Loaded
        );
        assert_eq!(
            std::fs::read(&cert).expect("re-read cert"),
            cert_bytes,
            "an existing certificate must not be overwritten"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn generated_files_are_pem() {
        let dir = temp_dir("pem");
        let cert = dir.join("c.pem");
        let key = dir.join("k.pem");
        ensure_self_signed(&cert, &key).expect("generate");

        let cert_text = std::fs::read_to_string(&cert).expect("read cert");
        let key_text = std::fs::read_to_string(&key).expect("read key");
        assert!(cert_text.starts_with("-----BEGIN CERTIFICATE-----"));
        // TlsIdentityCtx reads the key via PrivateKeyDer::from_pem_file, which
        // accepts PKCS#8 "PRIVATE KEY" blocks.
        assert!(key_text.contains("PRIVATE KEY-----"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn key_is_not_world_readable() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = temp_dir("perm");
        let cert = dir.join("c.pem");
        let key = dir.join("k.pem");
        ensure_self_signed(&cert, &key).expect("generate");

        let mode = std::fs::metadata(&key).expect("stat key").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "private key must be owner-only");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_key_regenerates_both() {
        let dir = temp_dir("half");
        let cert = dir.join("c.pem");
        let key = dir.join("k.pem");
        ensure_self_signed(&cert, &key).expect("generate");
        std::fs::remove_file(&key).expect("remove key");

        assert_eq!(
            ensure_self_signed(&cert, &key).expect("regenerate"),
            CertOutcome::Generated,
            "a certificate without its key is unusable and must be replaced"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
