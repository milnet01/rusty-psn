use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};

use log::warn;

/// Allowed Sony PSN domain suffixes for download and manifest URLs.
/// Package URLs from Sony's XML responses are validated against these before any request is made.
pub const ALLOWED_DOMAINS: &[&str] = &[".dl.playstation.net", ".playstation.net"];

/// Maximum size for metadata responses (XML update info, JSON manifests): 1 MB.
pub const MAX_METADATA_SIZE: u64 = 1_048_576;

/// Known valid serial prefixes for PlayStation platforms.
const VALID_PREFIXES: &[&str] = &[
    // PS3
    "NPUB", "NPUA", "NPEB", "NPEA", "NPJB", "NPJA", "NPHA", "NPHB", "NPHG", "NPHJ", "BCES", "BCUS", "BCAS", "BCJS", "BCKS",
    "BLES", "BLUS", "BLAS", "BLJS", "BLKS", // PS4
    "CUSA", // PSVita (digital)
    "PCSA", "PCSB", "PCSC", "PCSD", "PCSE", "PCSF", "PCSG", "PCSH", // PSVita (physical card)
    "VCAS", "VCJS", "VCJX", "VCKS", "VLAS", "VLJM", "VLJS", "VLKS",
];

#[derive(Debug)]
pub enum SecurityError {
    DisallowedDomain(String),
    PathEscape { attempted: PathBuf, boundary: PathBuf },
    InvalidSerial(String),
}

impl fmt::Display for SecurityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecurityError::DisallowedDomain(url) => {
                write!(f, "URL points to a non-PlayStation domain: {url}")
            }
            SecurityError::PathEscape { attempted, boundary } => {
                write!(f, "Path {:?} escapes the download directory {:?}", attempted, boundary)
            }
            SecurityError::InvalidSerial(reason) => {
                write!(f, "Invalid serial: {reason}")
            }
        }
    }
}

/// Whether a host name falls under one of the allowed Sony domains.
fn is_allowed_sony_host(host: &str) -> bool {
    ALLOWED_DOMAINS.iter().any(|d| host.ends_with(d))
}

/// Validate that a URL points to an allowed Sony PlayStation domain.
pub fn validate_download_url(url: &str) -> Result<(), SecurityError> {
    let parsed = match reqwest::Url::parse(url) {
        Ok(u) => u,
        Err(_) => return Err(SecurityError::DisallowedDomain(url.to_string())),
    };

    // Only allow http and https schemes.
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return Err(SecurityError::DisallowedDomain(url.to_string())),
    }

    let host = match parsed.host_str() {
        Some(h) => h,
        None => return Err(SecurityError::DisallowedDomain(url.to_string())),
    };

    if is_allowed_sony_host(host) {
        Ok(())
    } else {
        Err(SecurityError::DisallowedDomain(url.to_string()))
    }
}

/// Normalize a path lexically by resolving `.` and `..` components without filesystem access.
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            _ => normalized.push(component),
        }
    }
    normalized
}

/// Validate that a resolved path stays within the specified base directory.
/// The base directory must already exist on disk (for canonicalization).
/// The target path does not need to exist — only its relationship to the base is checked.
pub fn validate_path_containment(target: &Path, base_dir: &Path) -> Result<(), SecurityError> {
    // Canonicalize the base directory (must exist).
    let canonical_base = match std::fs::canonicalize(base_dir) {
        Ok(p) => p,
        Err(_) => {
            // If the base doesn't exist yet, normalize both paths lexically and use
            // Path::starts_with (component-aware) instead of string comparison.
            // String-based starts_with is UNSAFE: "/base-evil/" passes against "/base".
            // Both are made absolute first, so a leading ".." on a relative path is resolved
            // against the current folder instead of being dropped.
            let cwd = std::env::current_dir().unwrap_or_default();
            let base_normalized = normalize_lexical(&cwd.join(base_dir));
            let target_normalized = normalize_lexical(&cwd.join(target));
            if target_normalized.starts_with(&base_normalized) {
                return Ok(());
            }
            return Err(SecurityError::PathEscape {
                attempted: target.to_path_buf(),
                boundary: base_dir.to_path_buf(),
            });
        }
    };

    // Canonicalize the deepest part of the target that exists, so a relative path is made
    // absolute and a linked folder is followed the way the OS will follow it. The part that
    // does not exist yet holds no links, so it is normalized lexically and appended.
    let mut existing = target;
    let mut missing = Vec::new();
    let resolved = loop {
        if let Ok(canonical) = std::fs::canonicalize(existing) {
            break canonical;
        }
        // Something is there but cannot be resolved, such as a link to a missing file.
        // Opening it would follow the link, so it is not a "missing" part: refuse.
        if std::fs::symlink_metadata(existing).is_ok() {
            return Err(SecurityError::PathEscape {
                attempted: target.to_path_buf(),
                boundary: base_dir.to_path_buf(),
            });
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name);
                existing = parent;
            }
            // A relative path whose first component does not exist: resolve from the current folder.
            _ if existing.as_os_str().is_empty() => match std::env::current_dir() {
                Ok(cwd) => break cwd,
                Err(_) => break PathBuf::new(),
            },
            _ => break PathBuf::new(),
        }
    };
    let normalized = normalize_lexical(&missing.iter().rev().fold(resolved, |path, name| path.join(name)));

    if normalized.starts_with(&canonical_base) {
        Ok(())
    } else {
        Err(SecurityError::PathEscape {
            attempted: target.to_path_buf(),
            boundary: base_dir.to_path_buf(),
        })
    }
}

/// Validate a normalized serial/title_id string.
/// Must be 9 alphanumeric characters starting with a known prefix.
pub fn validate_serial(serial: &str) -> Result<(), SecurityError> {
    if serial.len() > 20 {
        return Err(SecurityError::InvalidSerial("input too long".to_string()));
    }

    if serial.len() != 9 {
        return Err(SecurityError::InvalidSerial(format!(
            "expected 9 characters, got {}",
            serial.len()
        )));
    }

    if !serial.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(SecurityError::InvalidSerial(
            "must contain only alphanumeric characters".to_string(),
        ));
    }

    if !VALID_PREFIXES.iter().any(|prefix| serial.starts_with(prefix)) {
        warn!("Serial {serial} has an unrecognized prefix — allowing but flagging");
    }

    Ok(())
}

/// Custom redirect error for reqwest redirect policy.
#[derive(Debug)]
pub struct RedirectError(pub String);

impl fmt::Display for RedirectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Redirect to disallowed domain: {}", self.0)
    }
}

impl std::error::Error for RedirectError {}

/// Build a custom reqwest redirect policy that validates each redirect URL
/// stays within the allowed Sony domains.
pub fn sony_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        let url_string = attempt.url().to_string();
        if let Some(host) = attempt.url().host_str() {
            if is_allowed_sony_host(host) {
                return attempt.follow();
            }
        }
        attempt.error(RedirectError(url_string))
    })
}

/// Certificate check that skips verification for Sony hosts only.
///
/// Sony's update hosts chain to Sony's private "SCEI DNAS Root 05", signed with SHA-1, so no
/// system trusts it and rustls refuses to; gs2.ww.prod.dl.playstation.net serves Akamai's
/// certificate under another name. Every other host gets the system's normal verification.
/// The handshake signature is still checked on every host.
#[derive(Debug)]
struct SonyOnlyCertBypass {
    inner: rustls_platform_verifier::Verifier,
}

impl ServerCertVerifier for SonyOnlyCertBypass {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if let ServerName::DnsName(name) = server_name {
            if is_allowed_sony_host(name.as_ref()) {
                return Ok(ServerCertVerified::assertion());
            }
        }
        self.inner
            .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// Build the TLS configuration for the PSN client: Sony hosts skip certificate verification,
/// every other host is verified normally.
pub fn sony_tls_config() -> Result<rustls::ClientConfig, rustls::Error> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let verifier = SonyOnlyCertBypass {
        inner: rustls_platform_verifier::Verifier::new(provider.clone())?,
    };
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_psn_download_urls() {
        assert!(validate_download_url("https://a0.ww.np.dl.playstation.net/tpl/np/NPUB30826/NPUB30826-ver.xml").is_ok());
        assert!(validate_download_url("https://gs-sec.ww.np.dl.playstation.net/plo/np/CUSA00001/abc/CUSA00001-ver.xml").is_ok());
        assert!(validate_download_url("http://gs.ww.np.dl.playstation.net/ppkg/np/PCSA00007/pkg.pkg").is_ok());
    }

    #[test]
    fn invalid_urls_rejected() {
        assert!(validate_download_url("https://evil.com/payload.pkg").is_err());
        assert!(validate_download_url("https://playstation.net.evil.com/pkg").is_err());
        assert!(validate_download_url("ftp://a0.ww.np.dl.playstation.net/file").is_err());
        assert!(validate_download_url("not-a-url").is_err());
        assert!(validate_download_url("").is_err());
    }

    #[test]
    fn valid_serials() {
        assert!(validate_serial("NPUB30826").is_ok());
        assert!(validate_serial("BCUS98148").is_ok());
        assert!(validate_serial("CUSA00001").is_ok());
        assert!(validate_serial("PCSA00007").is_ok());
        assert!(validate_serial("BLUS31147").is_ok());
    }

    #[test]
    fn invalid_serials() {
        assert!(validate_serial("").is_err());
        assert!(validate_serial("ABC").is_err());
        assert!(validate_serial("TOOLONG12345").is_err());
        assert!(validate_serial("NPUB-3082").is_err()); // dash not alphanumeric
        assert!(validate_serial("NPUB 3082").is_err()); // space not alphanumeric
    }

    #[test]
    fn path_containment_basic() {
        // Lexical check: target under base.
        let base = Path::new("/tmp/downloads");
        let target = Path::new("/tmp/downloads/game/serial");
        assert!(validate_path_containment(target, base).is_ok());
    }

    #[test]
    fn path_escape_detected() {
        let base = Path::new("/tmp/downloads");
        let target = Path::new("/etc/passwd");
        assert!(validate_path_containment(target, base).is_err());
    }

    #[test]
    fn path_escape_via_similar_prefix() {
        // Regression test: string-based starts_with would pass this incorrectly.
        // "/tmp/downloads-evil/" starts with string "/tmp/downloads" but is NOT a child path.
        let base = Path::new("/tmp/downloads");
        let target = Path::new("/tmp/downloads-evil/payload");
        assert!(validate_path_containment(target, base).is_err());
    }

    #[test]
    fn path_escape_via_parent_traversal() {
        let base = Path::new("/tmp/downloads");
        let target = Path::new("/tmp/downloads/../../etc/passwd");
        assert!(validate_path_containment(target, base).is_err());
    }

    #[test]
    fn path_containment_relative_existing_base() {
        // The command line's default download folder is the relative "pkgs/". Once it
        // exists, a new file inside it must still pass.
        // Cargo runs tests from the crate root, so a folder under target/ has a relative form.
        let dir = tempfile::tempdir_in("target").unwrap();
        let cwd = std::env::current_dir().unwrap();
        let relative_base = dir.path().strip_prefix(&cwd).unwrap_or(dir.path());
        assert!(relative_base.is_relative());
        assert!(validate_path_containment(&relative_base.join("BLUS31147/x.pkg"), relative_base).is_ok());
        assert!(validate_path_containment(&relative_base.join("../escape.pkg"), relative_base).is_err());
    }

    #[test]
    fn path_escape_via_leading_parent_on_missing_relative_base() {
        // The base does not exist yet, so the check is lexical. A leading ".." must not be dropped.
        let base = Path::new("rusty-psn-missing-base");
        assert!(!base.exists());
        let target = base.join("../../rusty-psn-missing-base/x.pkg");
        assert!(validate_path_containment(&target, base).is_err());
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn path_escape_via_dangling_symlink_detected() {
        // A link inside the base pointing at a path that does not exist yet must not count as "missing".
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("downloads");
        std::fs::create_dir(&base).unwrap();
        std::os::unix::fs::symlink(root.path().join("outside.pkg"), base.join("link.pkg")).unwrap();
        assert!(validate_path_containment(&base.join("link.pkg"), &base).is_err());
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn path_escape_via_symlinked_folder_detected() {
        // A folder inside the base that links elsewhere must not let a new file escape.
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("downloads");
        let outside = root.path().join("outside");
        std::fs::create_dir(&base).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, base.join("link")).unwrap();
        assert!(validate_path_containment(&base.join("link/new.pkg"), &base).is_err());
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn path_containment_symlinked_base() {
        // A download folder that is itself a link (a common home-folder setup) must still work.
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real");
        let link = root.path().join("link");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(validate_path_containment(&link.join("BLUS31147/new.pkg"), &link).is_ok());
    }
}
