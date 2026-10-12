use std::collections::HashMap;
use std::path::{Path, PathBuf};

use log::warn;

/// Maximum file size for YAML config files (10 MB).
const MAX_YAML_SIZE: u64 = 10_485_760;

/// Resolved RPCS3 virtual filesystem paths.
pub struct Rpcs3VfsConfig {
    pub dev_hdd0_path: PathBuf,
}

/// Parse RPCS3's games.yml file.
/// Returns a mapping of serial → game path.
pub fn parse_games_yml(path: &Path) -> Result<HashMap<String, PathBuf>, String> {
    let metadata = std::fs::metadata(path).map_err(|e| format!("cannot read games.yml: {e}"))?;
    if metadata.len() > MAX_YAML_SIZE {
        return Err(format!("games.yml too large: {} bytes", metadata.len()));
    }

    let content = std::fs::read_to_string(path).map_err(|e| format!("cannot read games.yml: {e}"))?;

    let map: HashMap<String, String> =
        serde_yaml_ng::from_str(&content).map_err(|e| format!("failed to parse games.yml: {e}"))?;

    Ok(map.into_iter().map(|(k, v)| (k, PathBuf::from(v))).collect())
}

/// Parse RPCS3's vfs.yml file and resolve the dev_hdd0 path.
///
/// `config_dir` is the directory containing vfs.yml — used to resolve `$(EmulatorDir)`
/// when it's set to an empty string (the default).
pub fn parse_vfs_yml(path: &Path, config_dir: &Path) -> Result<Rpcs3VfsConfig, String> {
    let metadata = std::fs::metadata(path).map_err(|e| format!("cannot read vfs.yml: {e}"))?;
    if metadata.len() > MAX_YAML_SIZE {
        return Err(format!("vfs.yml too large: {} bytes", metadata.len()));
    }

    let content = std::fs::read_to_string(path).map_err(|e| format!("cannot read vfs.yml: {e}"))?;

    let map: HashMap<String, serde_yaml_ng::Value> =
        serde_yaml_ng::from_str(&content).map_err(|e| format!("failed to parse vfs.yml: {e}"))?;

    // Resolve $(EmulatorDir). When the value is empty, it defaults to the config directory.
    let emulator_dir = match map.get("$(EmulatorDir)") {
        Some(serde_yaml_ng::Value::String(s)) if !s.is_empty() => PathBuf::from(s),
        _ => config_dir.to_path_buf(),
    };

    // Resolve /dev_hdd0/. The value may reference $(EmulatorDir) or be a direct path.
    // Ensure emulator_dir string ends with '/' for correct substitution
    // (e.g., "$(EmulatorDir)dev_hdd0/" should become "/path/to/rpcs3/dev_hdd0/").
    let emulator_dir_str = {
        let s = emulator_dir.to_string_lossy().to_string();
        if s.ends_with('/') {
            s
        } else {
            format!("{s}/")
        }
    };

    let dev_hdd0_path = match map.get("/dev_hdd0/") {
        Some(serde_yaml_ng::Value::String(s)) if !s.is_empty() => {
            let resolved = s.replace("$(EmulatorDir)", &emulator_dir_str);
            PathBuf::from(resolved)
        }
        _ => emulator_dir.join("dev_hdd0"),
    };

    if !dev_hdd0_path.exists() {
        warn!("Resolved dev_hdd0 path does not exist: {}", dev_hdd0_path.display());
    }

    Ok(Rpcs3VfsConfig { dev_hdd0_path })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parse_simple_games_yml() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "BCUS98111: /games/God of War III.iso").unwrap();
        writeln!(f, "NPUB30826: /games/AC3.iso").unwrap();

        let result = parse_games_yml(f.path()).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result["BCUS98111"], PathBuf::from("/games/God of War III.iso"));
        assert_eq!(result["NPUB30826"], PathBuf::from("/games/AC3.iso"));
    }

    #[test]
    fn parse_vfs_yml_empty_emulator_dir() {
        let dir = tempfile::tempdir().unwrap();
        let vfs_path = dir.path().join("vfs.yml");
        std::fs::write(
            &vfs_path,
            "\"$(EmulatorDir)\": \"\"\n\"/dev_hdd0/\": \"$(EmulatorDir)dev_hdd0/\"\n",
        )
        .unwrap();

        let config = parse_vfs_yml(&vfs_path, dir.path()).unwrap();
        // Resolved path should be under the config directory.
        assert!(
            config
                .dev_hdd0_path
                .to_string_lossy()
                .starts_with(&dir.path().to_string_lossy().to_string()),
            "Expected path starting with {:?}, got {:?}",
            dir.path(),
            config.dev_hdd0_path
        );
    }

    #[test]
    fn parse_vfs_yml_explicit_emulator_dir() {
        let dir = tempfile::tempdir().unwrap();
        let vfs_path = dir.path().join("vfs.yml");
        std::fs::write(
            &vfs_path,
            "\"$(EmulatorDir)\": \"/opt/rpcs3/\"\n\"/dev_hdd0/\": \"$(EmulatorDir)dev_hdd0/\"\n",
        )
        .unwrap();

        let config = parse_vfs_yml(&vfs_path, dir.path()).unwrap();
        assert_eq!(config.dev_hdd0_path, PathBuf::from("/opt/rpcs3/dev_hdd0/"));
    }

    #[test]
    fn parse_nonexistent_games_yml() {
        assert!(parse_games_yml(Path::new("/nonexistent/games.yml")).is_err());
    }
}
