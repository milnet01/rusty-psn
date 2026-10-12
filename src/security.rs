use std::fmt;
use std::path::{Path, PathBuf};

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

    if ALLOWED_DOMAINS.iter().any(|d| host.ends_with(d)) {
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
            let base_normalized = normalize_lexical(base_dir);
            let target_normalized = normalize_lexical(target);
            if target_normalized.starts_with(&base_normalized) {
                return Ok(());
            }
            return Err(SecurityError::PathEscape {
                attempted: target.to_path_buf(),
                boundary: base_dir.to_path_buf(),
            });
        }
    };

    // If target exists, canonicalize it for an exact check.
    if target.exists() {
        if let Ok(canonical_target) = std::fs::canonicalize(target) {
            if canonical_target.starts_with(&canonical_base) {
                return Ok(());
            }
            return Err(SecurityError::PathEscape {
                attempted: target.to_path_buf(),
                boundary: base_dir.to_path_buf(),
            });
        }
    }

    // Target doesn't exist yet — normalize lexically and check containment.
    let normalized = normalize_lexical(target);

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
            if ALLOWED_DOMAINS.iter().any(|d| host.ends_with(d)) {
                return attempt.follow();
            }
        }
        attempt.error(RedirectError(url_string))
    })
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
}
