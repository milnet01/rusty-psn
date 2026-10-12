use std::cmp::Ordering;
use std::fmt;
use std::path::{Path, PathBuf};

use log::{info, warn};

pub mod auto_detect;
pub mod config;
pub mod sfo;

/// A detected RPCS3 installation.
pub struct Rpcs3Installation {
    pub config_dir: PathBuf,
    pub dev_hdd0_path: PathBuf,
    /// Path to the RPCS3 executable (AppImage, binary, or "flatpak" sentinel).
    /// `None` if the executable couldn't be found automatically.
    pub executable: Option<PathBuf>,
}

/// A game found in the RPCS3 library with its installed version info.
#[derive(Clone)]
#[allow(dead_code)]
pub struct InstalledGame {
    pub serial: String,
    pub title: String,
    pub installed_version: Option<String>,
    pub game_path: PathBuf,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum Rpcs3Error {
    NotFound,
    ConfigNotFound(PathBuf),
    SfoParseFailed(PathBuf, String),
    YamlParseFailed(String),
    InstallFailed(String),
    Io(std::io::Error),
}

impl fmt::Display for Rpcs3Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rpcs3Error::NotFound => write!(f, "RPCS3 installation not found"),
            Rpcs3Error::ConfigNotFound(p) => write!(f, "RPCS3 config file not found: {}", p.display()),
            Rpcs3Error::SfoParseFailed(p, reason) => write!(f, "Failed to parse {}: {reason}", p.display()),
            Rpcs3Error::YamlParseFailed(reason) => write!(f, "Failed to parse RPCS3 config: {reason}"),
            Rpcs3Error::InstallFailed(reason) => write!(f, "RPCS3 installation failed: {reason}"),
            Rpcs3Error::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

/// Scan an RPCS3 installation for all games known to the emulator.
///
/// Uses games.yml as the primary source (contains ALL registered games, including ISOs).
/// For each game, tries to read PARAM.SFO from multiple locations to get title and version:
/// 1. `dev_hdd0/game/{serial}/PARAM.SFO` — installed game data (has update version)
/// 2. `{game_path}/PS3_GAME/PARAM.SFO` — for .PS3/ folder-format games
/// 3. Falls back to extracting the title from the filename for ISO games without game data.
pub fn scan_library(installation: &Rpcs3Installation) -> Result<Vec<InstalledGame>, Rpcs3Error> {
    let games_yml_path = installation.config_dir.join("games.yml");
    if !games_yml_path.exists() {
        return Err(Rpcs3Error::ConfigNotFound(games_yml_path));
    }

    let games_map = config::parse_games_yml(&games_yml_path).map_err(Rpcs3Error::YamlParseFailed)?;
    let game_dir = installation.dev_hdd0_path.join("game");

    let mut installed_games = Vec::new();

    for (serial, game_path) in &games_map {
        if !looks_like_serial(serial) {
            continue;
        }

        // Try SFO sources in priority order.
        // 1. dev_hdd0/game/{serial}/PARAM.SFO — has installed update version (APP_VER).
        let hdd0_sfo_path = game_dir.join(serial).join("PARAM.SFO");
        let hdd0_sfo = if hdd0_sfo_path.exists() {
            match sfo::parse_sfo(&hdd0_sfo_path) {
                Ok(params) => Some(params),
                Err(e) => {
                    warn!("Failed to parse dev_hdd0 PARAM.SFO for {serial}: {e}");
                    None
                }
            }
        } else {
            None
        };

        // 2. {game_path}/PS3_GAME/PARAM.SFO — for .PS3/ folder-format games on disc.
        let disc_sfo = if hdd0_sfo.is_none() {
            let disc_sfo_path = game_path.join("PS3_GAME").join("PARAM.SFO");
            if disc_sfo_path.exists() {
                match sfo::parse_sfo(&disc_sfo_path) {
                    Ok(params) => Some(params),
                    Err(e) => {
                        warn!("Failed to parse disc PARAM.SFO for {serial}: {e}");
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };

        // Determine title: prefer hdd0 SFO, then disc SFO, then extract from filename.
        let title = if let Some(ref sfo) = hdd0_sfo {
            sfo.title.clone()
        } else if let Some(ref sfo) = disc_sfo {
            sfo.title.clone()
        } else {
            title_from_path(game_path)
        };

        // Installed version comes from dev_hdd0 SFO (the update data).
        let installed_version = hdd0_sfo.as_ref().and_then(|sfo| sfo.app_ver.clone());

        installed_games.push(InstalledGame {
            serial: serial.clone(),
            title,
            installed_version,
            game_path: game_path.clone(),
        });
    }

    installed_games.sort_by(|a, b| a.serial.cmp(&b.serial));

    info!(
        "RPCS3 library scan found {} games ({} from games.yml)",
        installed_games.len(),
        games_map.len()
    );
    Ok(installed_games)
}

/// Extract a human-readable game title from a file path.
///
/// Handles common ROM naming conventions:
/// - `/path/to/God of War III (USA).iso` → `God of War III`
/// - `/path/to/Rock Band (USA).PS3/` → `Rock Band`
/// - Strips region tags like `(USA)`, `(Europe)`, `(World)`, `(Japan)`.
/// - Preserves meaningful tags like `(Game of the Year Edition)`.
fn title_from_path(path: &Path) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown");

    // Strip common extensions.
    let name = name
        .trim_end_matches('/')
        .trim_end_matches(".iso")
        .trim_end_matches(".ISO")
        .trim_end_matches(".PS3")
        .trim_end_matches(".ps3");

    // Strip region tags: (USA), (Europe), (World), (Japan), (Asia), (Korea).
    static REGION_TAGS: &[&str] = &[
        "(USA)",
        "(Europe)",
        "(World)",
        "(Japan)",
        "(Asia)",
        "(Korea)",
        "(En)",
        "(Ja)",
        "(Complete Edition)",
    ];

    let mut result = name.to_string();
    for tag in REGION_TAGS {
        result = result.replace(tag, "");
    }

    // Clean up extra whitespace from removed tags.
    let result = result.split_whitespace().collect::<Vec<_>>().join(" ");

    if result.is_empty() {
        name.to_string()
    } else {
        result
    }
}

/// Install a PKG file via RPCS3's CLI.
///
/// Spawns `rpcs3 --installpkg {pkg_path}`. Note: RPCS3 opens its GUI for installation progress.
/// If `rpcs3_executable` is the Flatpak sentinel `flatpak:net.rpcs3.RPCS3`, runs via `flatpak run`.
pub async fn install_pkg(rpcs3_executable: &Path, pkg_path: &Path) -> Result<(), Rpcs3Error> {
    if !pkg_path.exists() {
        return Err(Rpcs3Error::InstallFailed(format!(
            "PKG file not found: {}",
            pkg_path.display()
        )));
    }

    let exe_str = rpcs3_executable.to_string_lossy();
    let is_flatpak = exe_str.starts_with("flatpak:");

    if !is_flatpak && !rpcs3_executable.exists() {
        return Err(Rpcs3Error::InstallFailed(format!(
            "RPCS3 executable not found: {}",
            rpcs3_executable.display()
        )));
    }

    info!(
        "Installing PKG via RPCS3: {} --installpkg {}",
        rpcs3_executable.display(),
        pkg_path.display()
    );

    let output = if is_flatpak {
        let app_id = exe_str.strip_prefix("flatpak:").unwrap_or("net.rpcs3.RPCS3");
        tokio::process::Command::new("flatpak")
            .args(["run", app_id, "--installpkg"])
            .arg(pkg_path)
            .output()
            .await
    } else {
        tokio::process::Command::new(rpcs3_executable)
            .arg("--installpkg")
            .arg(pkg_path)
            .output()
            .await
    }
    .map_err(|e| Rpcs3Error::InstallFailed(format!("failed to launch RPCS3: {e}")))?;

    if output.status.success() {
        info!("RPCS3 PKG installation completed successfully");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(Rpcs3Error::InstallFailed(format!(
            "RPCS3 exited with {}: {}",
            output.status,
            stderr.trim()
        )))
    }
}

/// Compare two Sony version strings (format "XX.YY", e.g., "01.03", "01.10").
///
/// Returns `Ordering::Less` if `installed` is older than `available`.
#[cfg_attr(not(feature = "egui"), allow(dead_code))]
pub fn compare_versions(installed: &str, available: &str) -> Ordering {
    let parse = |v: &str| -> (u32, u32) {
        let parts: Vec<&str> = v.split('.').collect();
        let major = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
        let minor = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        (major, minor)
    };

    let installed_ver = parse(installed);
    let available_ver = parse(available);
    installed_ver.cmp(&available_ver)
}

/// Check if a directory name looks like a PS3 serial (4 uppercase letters + 5 digits).
fn looks_like_serial(name: &str) -> bool {
    crate::utils::looks_like_serial(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison() {
        assert_eq!(compare_versions("01.03", "01.10"), Ordering::Less);
        assert_eq!(compare_versions("01.10", "01.10"), Ordering::Equal);
        assert_eq!(compare_versions("02.00", "01.99"), Ordering::Greater);
        assert_eq!(compare_versions("01.00", "02.00"), Ordering::Less);
    }

    #[test]
    fn version_comparison_edge_cases() {
        assert_eq!(compare_versions("", "01.00"), Ordering::Less);
        assert_eq!(compare_versions("01.00", ""), Ordering::Greater);
        assert_eq!(compare_versions("", ""), Ordering::Equal);
    }

    #[test]
    fn serial_detection() {
        assert!(looks_like_serial("BCUS98111"));
        assert!(looks_like_serial("NPUB30826"));
        assert!(!looks_like_serial("$locks"));
        assert!(!looks_like_serial("BCUS98111-DATA"));
        assert!(!looks_like_serial("short"));
        assert!(!looks_like_serial("toolongstring"));
    }

    #[test]
    fn title_from_iso_path() {
        assert_eq!(
            title_from_path(Path::new("/mnt/Games/ROMS/ps3/God of War III (USA).iso")),
            "God of War III"
        );
    }

    #[test]
    fn title_from_ps3_folder_path() {
        assert_eq!(
            title_from_path(Path::new("/mnt/Games/ROMS/ps3/Rock Band (USA).PS3/")),
            "Rock Band"
        );
    }

    #[test]
    fn title_from_path_with_edition() {
        assert_eq!(
            title_from_path(Path::new(
                "/games/Batman - Arkham Asylum (USA) (Game of the Year Edition).iso"
            )),
            "Batman - Arkham Asylum (Game of the Year Edition)"
        );
    }

    #[test]
    fn title_from_path_europe() {
        assert_eq!(title_from_path(Path::new("/games/MotorStorm (Europe).iso")), "MotorStorm");
    }

    #[test]
    fn title_from_path_no_region() {
        assert_eq!(
            title_from_path(Path::new("/games/Battlefield 3 (World).iso")),
            "Battlefield 3"
        );
    }
}
