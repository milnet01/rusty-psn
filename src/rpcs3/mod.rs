use std::cmp::Ordering;
use std::collections::HashSet;
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
    /// RPCS3 exited cleanly but the installed version is not the expected one.
    /// `found` is the APP_VER read afterwards, `None` where no PARAM.SFO could be read.
    NotInstalled {
        serial: String,
        found: Option<String>,
    },
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
            Rpcs3Error::NotInstalled {
                serial,
                found: Some(found),
            } => {
                write!(f, "RPCS3 finished, but {serial} still reports version {found}")
            }
            Rpcs3Error::NotInstalled { serial, found: None } => {
                write!(f, "RPCS3 finished, but no installed version of {serial} was found")
            }
            Rpcs3Error::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

/// Scan an RPCS3 installation for all games known to the emulator.
///
/// Lists games as RPCS3 does (Emu/game_enumeration.h, RPCS3 commit 6fe3c87): every game in
/// games.yml, which holds disc and external games, plus every folder in `dev_hdd0/game` with a
/// PARAM.SFO, which is where PSN games live. For a games.yml game, title and version come from:
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
        // Also read it when the dev_hdd0 SFO gives no version (a disc game with no update installed).
        let disc_sfo = if hdd0_sfo.as_ref().and_then(|sfo| sfo.app_ver.as_ref()).is_none() {
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

        // Installed version comes from dev_hdd0 SFO (the update data), else from the disc.
        let installed_version = hdd0_sfo
            .as_ref()
            .and_then(|sfo| sfo.app_ver.clone())
            .or_else(|| disc_sfo.as_ref().and_then(|sfo| sfo.app_ver.clone()));

        installed_games.push(InstalledGame {
            serial: serial.clone(),
            title,
            installed_version,
            game_path: game_path.clone(),
        });
    }

    let from_games_yml = installed_games.len();
    let listed: HashSet<String> = installed_games.iter().map(|game| game.serial.clone()).collect();
    installed_games.extend(
        hdd0_games(&game_dir)
            .into_iter()
            .filter(|game| !listed.contains(&game.serial)),
    );

    installed_games.sort_by(|a, b| a.serial.cmp(&b.serial));

    info!(
        "RPCS3 library scan found {} games ({from_games_yml} from games.yml)",
        installed_games.len()
    );
    Ok(installed_games)
}

/// Every folder in `dev_hdd0/game` whose PARAM.SFO names a serial, keyed by that serial as RPCS3
/// keys it, not by the folder name. A folder with no PARAM.SFO is skipped: RPCS3 keeps a lock
/// folder and TEST12345 there (RPSN-0136).
fn hdd0_games(game_dir: &Path) -> Vec<InstalledGame> {
    let entries = match std::fs::read_dir(game_dir) {
        Ok(entries) => entries,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!("Cannot list {}: {e}", game_dir.display());
            }
            return Vec::new();
        }
    };

    let mut games = Vec::new();
    for entry in entries.flatten() {
        let game_path = entry.path();
        let sfo_path = game_path.join("PARAM.SFO");
        if !game_path.is_dir() || !sfo_path.is_file() {
            continue;
        }
        let sfo = match sfo::parse_sfo(&sfo_path) {
            Ok(sfo) => sfo,
            Err(e) => {
                warn!("Failed to parse {}: {e}", sfo_path.display());
                continue;
            }
        };
        if !looks_like_serial(&sfo.title_id) {
            continue;
        }
        games.push(InstalledGame {
            serial: sfo.title_id,
            title: sfo.title,
            installed_version: sfo.app_ver,
            game_path,
        });
    }
    games
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

/// What an install should leave behind, read once RPCS3 has exited
/// (docs/specs/RPSN-0137-headless-confirmed-install.md § 4.2).
pub enum InstallCheck {
    /// An update package. `dev_hdd0/game/<serial>/PARAM.SFO` must carry APP_VER equal to
    /// `version`; with `None`, any APP_VER other than the one there before the install.
    AppVersion {
        dev_hdd0: PathBuf,
        serial: String,
        version: Option<String>,
    },
    /// Nothing rusty-psn can read back: a DLC, or no known dev_hdd0.
    Unconfirmed,
}

#[derive(Debug, PartialEq)]
pub enum InstallOutcome {
    Confirmed,
    /// RPCS3 exited cleanly; nothing was checked.
    Unconfirmed,
}

/// Install a PKG file, or a folder of PKG, RAP and EDAT files, via RPCS3's headless mode.
///
/// Headless RPCS3 installs with no window and exits 0 whatever happened (`main` in
/// rpcs3/rpcs3.cpp, RPCS3 commit 6fe3c87), so success is decided by `check`, not the exit.
/// `pkg_path` may also be a folder: RPCS3 then places the RAP and EDAT licences at its top
/// level and installs every PKG there.
/// If `rpcs3_executable` is the Flatpak sentinel `flatpak:net.rpcs3.RPCS3`, runs via `flatpak run`.
pub async fn install_pkg(rpcs3_executable: &Path, pkg_path: &Path, check: InstallCheck) -> Result<InstallOutcome, Rpcs3Error> {
    if !pkg_path.exists() {
        return Err(Rpcs3Error::InstallFailed(format!(
            "PKG path not found: {}",
            pkg_path.display()
        )));
    }

    // The serial is joined onto dev_hdd0/game, so it must be a serial before anything runs.
    if let InstallCheck::AppVersion { serial, .. } = &check {
        if !looks_like_serial(serial) {
            return Err(Rpcs3Error::InstallFailed(format!("not a game serial: {serial:?}")));
        }
    }
    let before = match &check {
        InstallCheck::AppVersion {
            dev_hdd0,
            serial,
            version: None,
        } => installed_app_ver(dev_hdd0, serial),
        _ => None,
    };

    let exe_str = rpcs3_executable.to_string_lossy();
    let is_flatpak = exe_str.starts_with("flatpak:");

    if !is_flatpak && !rpcs3_executable.exists() {
        return Err(Rpcs3Error::InstallFailed(format!(
            "RPCS3 executable not found: {}",
            rpcs3_executable.display()
        )));
    }

    info!(
        "Installing PKG via RPCS3: {} --headless --installpkg {}",
        rpcs3_executable.display(),
        pkg_path.display()
    );

    let output = if is_flatpak {
        let app_id = exe_str.strip_prefix("flatpak:").unwrap_or("net.rpcs3.RPCS3");
        tokio::process::Command::new("flatpak")
            .args(["run", app_id, "--headless", "--installpkg"])
            .arg(pkg_path)
            .output()
            .await
    } else {
        tokio::process::Command::new(rpcs3_executable)
            .args(["--headless", "--installpkg"])
            .arg(pkg_path)
            .output()
            .await
    }
    .map_err(|e| Rpcs3Error::InstallFailed(format!("failed to launch RPCS3: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Rpcs3Error::InstallFailed(format!(
            "RPCS3 exited with {}: {}",
            output.status,
            stderr.trim()
        )));
    }

    let InstallCheck::AppVersion {
        dev_hdd0,
        serial,
        version,
    } = check
    else {
        info!("RPCS3 finished; the install was not checked");
        return Ok(InstallOutcome::Unconfirmed);
    };

    // Compared as text: compare_versions reads an unparsable version as 0.0, so two
    // unreadable versions would compare equal and confirm an install that did not happen.
    let found = installed_app_ver(&dev_hdd0, &serial);
    let confirmed = match &version {
        Some(expected) => found.as_deref() == Some(expected.trim()),
        None => found.is_some() && found != before,
    };
    if confirmed {
        info!("RPCS3 installed {serial}, now at version {found:?}");
        Ok(InstallOutcome::Confirmed)
    } else {
        warn!("RPCS3 finished but {serial} reports version {found:?}, expected {version:?}");
        Err(Rpcs3Error::NotInstalled { serial, found })
    }
}

/// APP_VER of `dev_hdd0/game/<serial>/PARAM.SFO`, trimmed, or `None` where it cannot be read.
fn installed_app_ver(dev_hdd0: &Path, serial: &str) -> Option<String> {
    let sfo_path = dev_hdd0.join("game").join(serial).join("PARAM.SFO");
    if !sfo_path.is_file() {
        return None;
    }
    match sfo::parse_sfo(&sfo_path) {
        Ok(sfo) => sfo.app_ver.map(|ver| ver.trim().to_string()),
        Err(e) => {
            warn!("Failed to parse {}: {e}", sfo_path.display());
            None
        }
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

/// Read a whole file, refusing one larger than `max` bytes.
///
/// The limit applies to the bytes read, not to the size the file reports, so a file
/// that grows after it is opened, or a pipe that reports no size, is still capped.
fn read_capped(path: &Path, max: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read;

    let mut data = Vec::new();
    std::fs::File::open(path)?.take(max + 1).read_to_end(&mut data)?;
    if data.len() as u64 > max {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("larger than {max} bytes"),
        ));
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_capped_refuses_a_file_over_the_limit() {
        // The size check used to come before an unbounded read (RPSN-0120).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        std::fs::write(&path, b"12345").unwrap();

        assert_eq!(read_capped(&path, 5).unwrap(), b"12345");
        assert!(read_capped(&path, 4).is_err());
    }

    #[test]
    fn scan_library_reads_disc_app_ver_when_no_update_installed() {
        // A disc game with no update installed has no dev_hdd0/game/{serial} folder.
        // Its installed version must come from the disc's own PARAM.SFO.
        let root = tempfile::tempdir().unwrap();
        let config_dir = root.path().join("config");
        let game_path = root.path().join("disc");
        std::fs::create_dir_all(game_path.join("PS3_GAME")).unwrap();
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(game_path.join("PS3_GAME/PARAM.SFO"), sfo::tests::make_test_sfo()).unwrap();
        std::fs::write(config_dir.join("games.yml"), format!("BCUS99999: {}/\n", game_path.display())).unwrap();
        let installation = Rpcs3Installation {
            config_dir,
            dev_hdd0_path: root.path().join("dev_hdd0"),
            executable: None,
        };

        let games = scan_library(&installation).unwrap();

        assert_eq!(games.len(), 1);
        assert_eq!(games[0].installed_version.as_deref(), Some("01.05"));
    }

    fn installation_with(root: &Path, games_yml: &str) -> Rpcs3Installation {
        let config_dir = root.join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("games.yml"), games_yml).unwrap();
        Rpcs3Installation {
            config_dir,
            dev_hdd0_path: root.join("dev_hdd0"),
            executable: None,
        }
    }

    #[test]
    fn scan_library_finds_psn_games_in_dev_hdd0() {
        // RPCS3 writes PSN games to dev_hdd0/game only, never to games.yml (RPSN-0136).
        let root = tempfile::tempdir().unwrap();
        let game_dir = root.path().join("dev_hdd0/game");
        std::fs::create_dir_all(game_dir.join("NPUB30826")).unwrap();
        std::fs::write(game_dir.join("NPUB30826/PARAM.SFO"), sfo::tests::make_test_sfo()).unwrap();
        std::fs::create_dir_all(game_dir.join("TEST12345")).unwrap();
        let installation = installation_with(root.path(), "");

        let games = scan_library(&installation).unwrap();

        assert_eq!(games.len(), 1);
        // The serial is the one PARAM.SFO names (make_test_sfo's), as RPCS3 keys it.
        assert_eq!(games[0].serial, "BCUS99999");
        assert_eq!(games[0].title, "Test Game");
        assert_eq!(games[0].installed_version.as_deref(), Some("01.05"));
    }

    #[test]
    fn scan_library_lists_a_game_in_both_places_once() {
        let root = tempfile::tempdir().unwrap();
        let game_dir = root.path().join("dev_hdd0/game");
        std::fs::create_dir_all(game_dir.join("BCUS99999")).unwrap();
        std::fs::write(game_dir.join("BCUS99999/PARAM.SFO"), sfo::tests::make_test_sfo()).unwrap();
        let disc = root.path().join("disc");
        let installation = installation_with(root.path(), &format!("BCUS99999: {}/\n", disc.display()));

        let games = scan_library(&installation).unwrap();

        assert_eq!(games.len(), 1);
        assert_eq!(games[0].game_path, disc);
    }

    /// A stand-in RPCS3 for the install tests: a shell script that records its arguments to
    /// `args`, runs `action`, and exits with `status`. Returns (executable, args file, dev_hdd0, pkg).
    #[cfg(unix)]
    fn stand_in_rpcs3(root: &Path, action: &str, status: i32) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;

        let fixture = root.join("fixture.sfo");
        std::fs::write(&fixture, sfo::tests::make_test_sfo()).unwrap();
        let dev_hdd0 = root.join("dev_hdd0");
        let args = root.join("args");
        let pkg = root.join("update.pkg");
        std::fs::write(&pkg, b"pkg").unwrap();
        let exe = root.join("rpcs3");
        let action = action
            .replace("{fixture}", &fixture.display().to_string())
            .replace("{game}", &dev_hdd0.join("game/BCUS99999").display().to_string());
        std::fs::write(
            &exe,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n{action}\nexit {status}\n",
                args.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        (exe, args, dev_hdd0, pkg)
    }

    #[cfg(unix)]
    const WRITES_01_05: &str = "mkdir -p '{game}' && cp '{fixture}' '{game}/PARAM.SFO'";

    #[cfg(unix)]
    fn expect(dev_hdd0: &Path, version: Option<&str>) -> InstallCheck {
        InstallCheck::AppVersion {
            dev_hdd0: dev_hdd0.to_path_buf(),
            serial: "BCUS99999".to_string(),
            version: version.map(str::to_string),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn install_runs_rpcs3_headless() {
        // Without --headless RPCS3 opens its window and the install waits for it to close (RPSN-0137).
        let root = tempfile::tempdir().unwrap();
        let (exe, args, _, pkg) = stand_in_rpcs3(root.path(), "", 0);

        install_pkg(&exe, &pkg, InstallCheck::Unconfirmed).await.unwrap();

        let recorded = std::fs::read_to_string(args).unwrap();
        assert_eq!(recorded, format!("--headless\n--installpkg\n{}\n", pkg.display()));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn install_confirms_the_expected_version() {
        // Headless RPCS3 exits 0 whatever happened, so the version read back decides (RPSN-0137).
        let root = tempfile::tempdir().unwrap();
        let (exe, _, dev_hdd0, pkg) = stand_in_rpcs3(root.path(), WRITES_01_05, 0);
        assert_eq!(
            install_pkg(&exe, &pkg, expect(&dev_hdd0, Some("01.05"))).await.unwrap(),
            InstallOutcome::Confirmed
        );
        assert!(matches!(
            install_pkg(&exe, &pkg, expect(&dev_hdd0, Some("01.06"))).await,
            Err(Rpcs3Error::NotInstalled { found: Some(ref v), .. }) if v == "01.05"
        ));

        let root = tempfile::tempdir().unwrap();
        let (exe, _, dev_hdd0, pkg) = stand_in_rpcs3(root.path(), "", 0);
        assert!(matches!(
            install_pkg(&exe, &pkg, expect(&dev_hdd0, Some("01.05"))).await,
            Err(Rpcs3Error::NotInstalled { found: None, .. })
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn install_confirms_a_changed_version() {
        let root = tempfile::tempdir().unwrap();
        let (exe, _, dev_hdd0, pkg) = stand_in_rpcs3(root.path(), WRITES_01_05, 0);
        assert_eq!(
            install_pkg(&exe, &pkg, expect(&dev_hdd0, None)).await.unwrap(),
            InstallOutcome::Confirmed
        );

        // Now 01.05 is there before the install, and a stand-in that writes nothing changes nothing.
        let (exe, _, _, _) = stand_in_rpcs3(root.path(), "", 0);
        assert!(matches!(
            install_pkg(&exe, &pkg, expect(&dev_hdd0, None)).await,
            Err(Rpcs3Error::NotInstalled { .. })
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn install_failure_beats_a_matching_version() {
        let root = tempfile::tempdir().unwrap();
        let (exe, _, dev_hdd0, pkg) = stand_in_rpcs3(root.path(), WRITES_01_05, 1);
        assert!(matches!(
            install_pkg(&exe, &pkg, expect(&dev_hdd0, Some("01.05"))).await,
            Err(Rpcs3Error::InstallFailed(_))
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn install_without_a_check_is_unconfirmed() {
        let root = tempfile::tempdir().unwrap();
        let (exe, _, _, pkg) = stand_in_rpcs3(root.path(), WRITES_01_05, 0);
        assert_eq!(
            install_pkg(&exe, &pkg, InstallCheck::Unconfirmed).await.unwrap(),
            InstallOutcome::Unconfirmed
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn install_refuses_an_unsafe_serial() {
        let root = tempfile::tempdir().unwrap();
        let (exe, args, dev_hdd0, pkg) = stand_in_rpcs3(root.path(), "", 0);
        let check = InstallCheck::AppVersion {
            dev_hdd0,
            serial: "../x".to_string(),
            version: None,
        };

        assert!(matches!(
            install_pkg(&exe, &pkg, check).await,
            Err(Rpcs3Error::InstallFailed(_))
        ));
        assert!(!args.exists(), "RPCS3 was launched");
    }

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
            title_from_path(Path::new("/home/user/roms/ps3/God of War III (USA).iso")),
            "God of War III"
        );
    }

    #[test]
    fn title_from_ps3_folder_path() {
        assert_eq!(
            title_from_path(Path::new("/home/user/roms/ps3/Rock Band (USA).PS3/")),
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
