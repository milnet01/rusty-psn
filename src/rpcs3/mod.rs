use std::cmp::Ordering;
use std::collections::HashSet;
use std::ffi::OsString;
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
    /// RPCS3 exited cleanly but its firmware is not the version installed. `found` is the version
    /// `dev_flash/vsh/etc/version.txt` names afterwards, `None` where it cannot be read.
    FirmwareNotInstalled {
        expected: String,
        found: Option<String>,
    },
    /// RPCS3 is already open, so an install would stop at its "already running" window.
    AlreadyRunning,
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
            Rpcs3Error::FirmwareNotInstalled {
                expected,
                found: Some(found),
            } => {
                write!(f, "RPCS3 finished, but its firmware is {found}, not {expected}")
            }
            Rpcs3Error::FirmwareNotInstalled { expected, found: None } => {
                write!(f, "RPCS3 finished, but no installed firmware was found (expected {expected})")
            }
            Rpcs3Error::AlreadyRunning => write!(f, "RPCS3 is open. Close RPCS3, then install again"),
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

        // dev_hdd0/game/{serial}/PARAM.SFO holds an installed update's APP_VER.
        let hdd0_sfo = read_sfo(&game_dir.join(serial).join("PARAM.SFO"));
        // The game's own PARAM.SFO; an ISO's is inside the image, which is not read.
        let game_sfo = read_sfo(&game_sfo_path(game_path, &installation.dev_hdd0_path));

        // Determine title: prefer hdd0 SFO, then the game's own SFO, then extract from filename.
        let title = if let Some(ref sfo) = hdd0_sfo {
            sfo.title.clone()
        } else if let Some(ref sfo) = game_sfo {
            sfo.title.clone()
        } else {
            title_from_path(game_path)
        };

        let installed_version = shown_version(game_sfo.as_ref(), hdd0_sfo.as_ref());

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
        hdd0_games(&game_dir, &installation.dev_hdd0_path)
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
fn hdd0_games(game_dir: &Path, dev_hdd0: &Path) -> Vec<InstalledGame> {
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
        if !game_path.is_dir() || !game_path.join("PARAM.SFO").is_file() {
            continue;
        }
        let sfo_path = game_sfo_path(&game_path, dev_hdd0);
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
        let installed_version = shown_version(Some(&sfo), None);
        games.push(InstalledGame {
            serial: sfo.title_id,
            title: sfo.title,
            installed_version,
            game_path,
        });
    }
    games
}

/// A parsed PARAM.SFO, or `None` where there is none or it cannot be read.
fn read_sfo(path: &Path) -> Option<sfo::SfoParams> {
    if !path.is_file() {
        return None;
    }
    sfo::parse_sfo(path)
        .inspect_err(|e| warn!("Failed to parse {}: {e}", path.display()))
        .ok()
}

/// Where a game keeps the PARAM.SFO RPCS3 reads, as `get_sfo_dir_from_game_path` in
/// rpcs3/Emu/system_utils.cpp (RPCS3 commit 6fe3c87) finds it: a disc game, marked by
/// PS3_DISC.SFB, in PS3_GAME; a trial (HG) game whose unlock licence is installed in C00,
/// where the full game is; any other game at its root. A folder with neither file falls
/// back to PS3_GAME, which RPCS3 would not list at all.
fn game_sfo_path(game_path: &Path, dev_hdd0: &Path) -> PathBuf {
    let root_sfo = game_path.join("PARAM.SFO");
    if game_path.join("PS3_DISC.SFB").is_file() || !root_sfo.is_file() {
        return game_path.join("PS3_GAME").join("PARAM.SFO");
    }
    let c00_sfo = game_path.join("C00").join("PARAM.SFO");
    let unlocked = c00_sfo.is_file()
        && read_sfo(&root_sfo).is_some_and(|sfo| sfo.category == "HG" && c00_unlock_edat_exists(dev_hdd0, &sfo.content_id));
    if unlocked {
        c00_sfo
    } else {
        root_sfo
    }
}

/// Whether the licence that unlocks a trial's full game is installed, as RPCS3's game list
/// checks it (`verify_c00_unlock_edat` called with `fast`, and `get_c00_unlock_edat_path`, in
/// rpcs3/Emu/system_utils.cpp, RPCS3 commit 6fe3c87): `{content_id}.edat` in the exdata folder
/// of any user in dev_hdd0/home. RPCS3's list checks only that the file is there, and so does this.
fn c00_unlock_edat_exists(dev_hdd0: &Path, content_id: &str) -> bool {
    // RPCS3's `is_valid_content_id`. It also keeps the ID from naming any other path.
    let valid = !content_id.is_empty()
        && content_id.len() <= 0x30
        && content_id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_');
    let Ok(users) = std::fs::read_dir(dev_hdd0.join("home")) else {
        return false;
    };
    valid
        && users.flatten().any(|user| {
            is_user_folder(&user.file_name())
                && user.path().is_dir()
                && user.path().join("exdata").join(format!("{content_id}.edat")).is_file()
        })
}

/// `check_user` in rpcs3/Emu/system_utils.cpp: a name eight bytes long whose leading digits,
/// read as `std::from_chars` reads them, make a number other than zero.
fn is_user_folder(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let digits = name.bytes().take_while(u8::is_ascii_digit).count();
    name.len() == 8 && name[..digits].parse::<u32>().is_ok_and(|id| id != 0)
}

/// The version RPCS3 shows for a game (`apply_patches` in rpcs3/Emu/game_enumeration.h and
/// `GetGameVersion` in rpcs3/rpcs3qt/gui_game_info.cpp, RPCS3 commit 6fe3c87). A disc game,
/// or one whose PARAM.SFO cannot be read, takes an installed patch's APP_VER where it is
/// higher than its own or it has none. With no APP_VER, the game's VERSION is shown.
fn shown_version(game: Option<&sfo::SfoParams>, patch: Option<&sfo::SfoParams>) -> Option<String> {
    let mut app_ver = game.and_then(|game| game.app_ver.clone());
    let takes_patches = game.is_none_or(|game| game.category == "DG");
    let patch_ver = patch
        .filter(|patch| patch.category != "DG")
        .and_then(|patch| patch.app_ver.clone());
    if let Some(patch_ver) = patch_ver.filter(|_| takes_patches) {
        if app_ver.as_deref().is_none_or(|own| version_is_bigger(&patch_ver, own)) {
            app_ver = Some(patch_ver);
        }
    }
    app_ver.or_else(|| game.map(|game| game.version.clone()).filter(|version| !version.is_empty()))
}

/// `version_is_bigger` in rpcs3/Emu/system_utils.cpp: both versions read as decimal numbers,
/// and a version that does not read as one is never bigger.
fn version_is_bigger(v0: &str, v1: &str) -> bool {
    matches!((v0.parse::<f64>(), v1.parse::<f64>()), (Ok(v0), Ok(v1)) if v0 > v1)
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

/// Installs wait here for each other: two RPCS3 processes cannot run at once (RPSN-0138).
static INSTALLS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The file RPCS3 locks while it runs, `<cache dir>/RPCS3.buf`, or `None` where it cannot be
/// named. The cache dir follows `fs::get_cache_dir` in Utilities/File.cpp, RPCS3 commit 6fe3c87.
fn instance_lock_path(rpcs3_executable: &Path) -> Option<PathBuf> {
    lock_path_from(rpcs3_executable, |key| std::env::var_os(key))
}

fn lock_path_from(rpcs3_executable: &Path, env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let home = || env("HOME").map(PathBuf::from);
    let exe_str = rpcs3_executable.to_string_lossy();
    let cache_dir = if let Some(app_id) = exe_str.strip_prefix("flatpak:") {
        // Flatpak points XDG_CACHE_HOME inside the sandbox at ~/.var/app/<app id>/cache.
        if app_id.is_empty() || app_id.starts_with('.') || app_id.contains(['/', '\\']) {
            return None;
        }
        home()?.join(".var/app").join(app_id).join("cache/rpcs3")
    } else if cfg!(windows) {
        // On Windows the cache dir is the config dir: a portable/ folder beside the
        // executable, else RPCS3_CONFIG_DIR, else the executable's folder.
        let exe_dir = rpcs3_executable.parent()?;
        let portable = exe_dir.join("portable");
        if portable.is_dir() {
            portable
        } else {
            env("RPCS3_CONFIG_DIR").map_or_else(|| exe_dir.to_path_buf(), PathBuf::from)
        }
    } else if cfg!(target_os = "macos") {
        home()?.join("Library/Caches/rpcs3")
    } else {
        // RPCS3 really does fall back to XDG_CONFIG_HOME here, not to a config folder.
        let base = env("XDG_CACHE_HOME")
            .or_else(|| env("XDG_CONFIG_HOME"))
            .map(PathBuf::from)
            .or_else(|| home().map(|home| home.join(".cache")))?;
        base.join("rpcs3")
    };
    Some(cache_dir.join("RPCS3.buf"))
}

/// Whether a running RPCS3 holds `lock_file`. RPCS3 opens it for writing and takes
/// `flock(LOCK_EX | LOCK_NB)`; on Windows it shares the file for reading only. So a test
/// open for writing, then a test lock, finds it. RPCS3 retries its own lock for about two
/// seconds at start, so this brief lock cannot stop it starting.
fn rpcs3_is_running(lock_file: &Path) -> bool {
    let file = match std::fs::OpenOptions::new().write(true).open(lock_file) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return false,
        // ERROR_SHARING_VIOLATION: another process holds the file without sharing writes.
        Err(e) if cfg!(windows) && e.raw_os_error() == Some(32) => return true,
        Err(e) => {
            warn!("Could not check whether RPCS3 is running ({}): {e}", lock_file.display());
            return false;
        }
    };
    match file.try_lock() {
        // Dropping `file` releases the test lock.
        Ok(()) => false,
        Err(std::fs::TryLockError::WouldBlock) => true,
        Err(std::fs::TryLockError::Error(e)) => {
            warn!("Could not check whether RPCS3 is running ({}): {e}", lock_file.display());
            false
        }
    }
}

/// Install a PKG file, or a folder of PKG, RAP and EDAT files, via RPCS3's headless mode.
///
/// Headless RPCS3 installs with no window and exits 0 whatever happened (`main` in
/// rpcs3/rpcs3.cpp, RPCS3 commit 6fe3c87), so success is decided by `check`, not the exit.
/// `pkg_path` may also be a folder: RPCS3 then places the RAP and EDAT licences at its top
/// level and installs every PKG there.
/// If `rpcs3_executable` is the Flatpak sentinel `flatpak:net.rpcs3.RPCS3`, runs via `flatpak run`.
///
/// Installs run one at a time, and none starts while RPCS3 is already open: a second RPCS3
/// shows a fatal-error window before it reads `--headless` (RPSN-0138).
pub async fn install_pkg(rpcs3_executable: &Path, pkg_path: &Path, check: InstallCheck) -> Result<InstallOutcome, Rpcs3Error> {
    let lock_file = instance_lock_path(rpcs3_executable);
    install_pkg_guarded(rpcs3_executable, pkg_path, check, lock_file.as_deref()).await
}

async fn install_pkg_guarded(
    rpcs3_executable: &Path,
    pkg_path: &Path,
    check: InstallCheck,
    lock_file: Option<&Path>,
) -> Result<InstallOutcome, Rpcs3Error> {
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

    // Held until RPCS3 exits. Taken before the lock check, so the RPCS3 an earlier
    // install started is never mistaken for one the user has open.
    let _one_at_a_time = INSTALLS.lock().await;
    if lock_file.is_some_and(rpcs3_is_running) {
        return Err(Rpcs3Error::AlreadyRunning);
    }

    // Read under the queue, after any earlier install of the same game has finished.
    let before = match &check {
        InstallCheck::AppVersion {
            dev_hdd0,
            serial,
            version: None,
        } => installed_app_ver(dev_hdd0, serial),
        _ => None,
    };

    run_headless(rpcs3_executable, "--installpkg", pkg_path).await?;

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

/// Run RPCS3 headless with `flag` and `path` (`--installpkg` or `--installfw`), waiting for it
/// to exit. If `rpcs3_executable` is the Flatpak sentinel `flatpak:net.rpcs3.RPCS3`, runs via
/// `flatpak run`. The caller holds `INSTALLS` and has checked RPCS3 is not open.
async fn run_headless(rpcs3_executable: &Path, flag: &str, path: &Path) -> Result<(), Rpcs3Error> {
    let exe_str = rpcs3_executable.to_string_lossy();
    let is_flatpak = exe_str.starts_with("flatpak:");

    if !is_flatpak && !rpcs3_executable.exists() {
        return Err(Rpcs3Error::InstallFailed(format!(
            "RPCS3 executable not found: {}",
            rpcs3_executable.display()
        )));
    }

    info!(
        "Running RPCS3: {} --headless {flag} {}",
        rpcs3_executable.display(),
        path.display()
    );

    let output = if is_flatpak {
        let app_id = exe_str.strip_prefix("flatpak:").unwrap_or("net.rpcs3.RPCS3");
        tokio::process::Command::new("flatpak")
            .args(["run", app_id, "--headless", flag])
            .arg(path)
            .output()
            .await
    } else {
        tokio::process::Command::new(rpcs3_executable)
            .args(["--headless", flag])
            .arg(path)
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
    Ok(())
}

/// What a firmware install should leave behind: `dev_flash` holding firmware `version`, as Sony
/// lists it ("4.9300").
pub struct FirmwareCheck {
    pub dev_flash: PathBuf,
    pub version: String,
}

/// Install a PS3 firmware file (PS3UPDAT.PUP) via RPCS3's headless mode (RPSN-0141).
///
/// Like `install_pkg`, headless RPCS3 exits 0 whatever happened (`run_rpcs3` in rpcs3/rpcs3.cpp,
/// RPCS3 commit 6fe3c87), so `check`, when given, decides: the firmware version.txt names
/// afterwards must be the expected one. Runs one at a time with package installs, and not
/// while RPCS3 is open.
pub async fn install_firmware(
    rpcs3_executable: &Path,
    pup_path: &Path,
    check: Option<FirmwareCheck>,
) -> Result<InstallOutcome, Rpcs3Error> {
    let lock_file = instance_lock_path(rpcs3_executable);
    install_firmware_guarded(rpcs3_executable, pup_path, check, lock_file.as_deref()).await
}

async fn install_firmware_guarded(
    rpcs3_executable: &Path,
    pup_path: &Path,
    check: Option<FirmwareCheck>,
    lock_file: Option<&Path>,
) -> Result<InstallOutcome, Rpcs3Error> {
    if !pup_path.is_file() {
        return Err(Rpcs3Error::InstallFailed(format!(
            "firmware file not found: {}",
            pup_path.display()
        )));
    }

    let _one_at_a_time = INSTALLS.lock().await;
    if lock_file.is_some_and(rpcs3_is_running) {
        return Err(Rpcs3Error::AlreadyRunning);
    }

    run_headless(rpcs3_executable, "--installfw", pup_path).await?;

    let Some(FirmwareCheck { dev_flash, version }) = check else {
        info!("RPCS3 finished; the firmware install was not checked");
        return Ok(InstallOutcome::Unconfirmed);
    };
    let found = installed_firmware_version(&dev_flash);
    if found.is_some() && found == normalize_firmware_version(&version) {
        info!("RPCS3 firmware is now {found:?}");
        Ok(InstallOutcome::Confirmed)
    } else {
        warn!("RPCS3 finished but its firmware reads {found:?}, expected {version}");
        Err(Rpcs3Error::FirmwareNotInstalled {
            expected: version,
            found,
        })
    }
}

/// The firmware version RPCS3 has installed, as RPCS3 shows it ("4.93"), or `None` where
/// `dev_flash/vsh/etc/version.txt` is missing or unreadable. The version is the text between
/// the first two colons ("release:04.9300:"), as `utils::get_firmware_version` in
/// util/sysinfo.cpp reads it (RPCS3 commit 6fe3c87).
pub fn installed_firmware_version(dev_flash: &Path) -> Option<String> {
    let path = dev_flash.join("vsh/etc/version.txt");
    let text = String::from_utf8(read_capped(&path, 4096).ok()?).ok()?;
    let field = text.split(':').nth(1)?;
    normalize_firmware_version(field)
}

/// "04.9300" and "4.9300" both become "4.93": leading zeros off the major number, trailing zeros
/// off the minor, as RPCS3 trims them.
fn normalize_firmware_version(version: &str) -> Option<String> {
    let (major, minor) = version.trim().split_once('.').unwrap_or((version.trim(), ""));
    if major.is_empty() || !major.bytes().all(|b| b.is_ascii_digit()) || !minor.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let major = major.trim_start_matches('0');
    let major = if major.is_empty() { "0" } else { major };
    let minor = minor.trim_end_matches('0');
    Some(if minor.is_empty() {
        major.to_string()
    } else {
        format!("{major}.{minor}")
    })
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

    fn sfo_with(category: &str, app_ver: Option<&str>, version: &str) -> sfo::SfoParams {
        sfo::SfoParams {
            title: String::new(),
            title_id: "BCUS99999".to_string(),
            app_ver: app_ver.map(str::to_string),
            version: version.to_string(),
            category: category.to_string(),
            content_id: String::new(),
        }
    }

    #[test]
    fn shown_version_follows_rpcs3() {
        // RPCS3 keeps the higher of a disc's and its patch's APP_VER (RPSN-0140).
        let disc = |app_ver| sfo_with("DG", app_ver, "01.00");
        let patch = sfo_with("GD", Some("01.05"), "01.00");
        assert_eq!(
            shown_version(Some(&disc(Some("01.02"))), Some(&patch)).as_deref(),
            Some("01.05")
        );
        assert_eq!(
            shown_version(Some(&disc(Some("01.10"))), Some(&patch)).as_deref(),
            Some("01.10")
        );
        assert_eq!(shown_version(Some(&disc(None)), Some(&patch)).as_deref(), Some("01.05"));
        // An unreadable disc, such as an ISO, takes the patch.
        assert_eq!(shown_version(None, Some(&patch)).as_deref(), Some("01.05"));
        // A version that does not read as a number is never higher.
        let odd_patch = sfo_with("GD", Some("v2"), "01.00");
        assert_eq!(
            shown_version(Some(&disc(Some("01.00"))), Some(&odd_patch)).as_deref(),
            Some("01.00")
        );
        // Only a disc game takes patches.
        let psn_game = sfo_with("HG", Some("01.00"), "01.00");
        assert_eq!(shown_version(Some(&psn_game), Some(&patch)).as_deref(), Some("01.00"));
        // With no APP_VER, RPCS3 shows VERSION.
        assert_eq!(
            shown_version(Some(&sfo_with("HG", None, "01.03")), None).as_deref(),
            Some("01.03")
        );
        assert_eq!(shown_version(Some(&sfo_with("HG", None, "")), None), None);
    }

    #[test]
    fn scan_library_reads_an_external_game_at_its_root() {
        // An external HG game keeps PARAM.SFO at its root, not in PS3_GAME (RPSN-0140).
        let root = tempfile::tempdir().unwrap();
        let game_path = root.path().join("external");
        std::fs::create_dir_all(&game_path).unwrap();
        std::fs::write(game_path.join("PARAM.SFO"), sfo::tests::make_test_sfo()).unwrap();
        let installation = installation_with(root.path(), &format!("BCUS99999: {}/\n", game_path.display()));

        let games = scan_library(&installation).unwrap();

        assert_eq!(games[0].title, "Test Game");
        assert_eq!(games[0].installed_version.as_deref(), Some("01.05"));
    }

    #[test]
    fn a_disc_game_reads_ps3_game_even_with_a_root_param_sfo() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("PS3_DISC.SFB"), b"").unwrap();
        std::fs::write(root.path().join("PARAM.SFO"), b"").unwrap();
        assert_eq!(
            game_sfo_path(root.path(), &root.path().join("dev_hdd0")),
            root.path().join("PS3_GAME/PARAM.SFO")
        );
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
    fn scan_library_reads_an_unlocked_trial_from_c00() {
        // RPCS3 shows a trial's full game, from C00, once its unlock licence is installed (RPSN-0154).
        const CONTENT_ID: &str = "UP0001-NPUB12345_00-TRIALGAME0000001";
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("dev_hdd0/game/NPUB12345");
        std::fs::create_dir_all(game.join("C00")).unwrap();
        let sfo = |title, app_ver| {
            sfo::tests::sfo_bytes(&[
                ("CATEGORY", "HG"),
                ("CONTENT_ID", CONTENT_ID),
                ("TITLE", title),
                ("TITLE_ID", "NPUB12345"),
                ("APP_VER", app_ver),
            ])
        };
        std::fs::write(game.join("PARAM.SFO"), sfo("Trial", "01.00")).unwrap();
        std::fs::write(game.join("C00/PARAM.SFO"), sfo("Full Game", "01.02")).unwrap();
        let installation = installation_with(root.path(), "");
        let shown = || {
            let games = scan_library(&installation).unwrap();
            (games[0].title.clone(), games[0].installed_version.clone().unwrap_or_default())
        };

        // Still locked: no licence, or one in a folder that is not a user's.
        assert_eq!(shown(), ("Trial".to_string(), "01.00".to_string()));
        let not_a_user = root.path().join("dev_hdd0/home/00000000/exdata");
        std::fs::create_dir_all(&not_a_user).unwrap();
        std::fs::write(not_a_user.join(format!("{CONTENT_ID}.edat")), b"").unwrap();
        assert_eq!(shown(), ("Trial".to_string(), "01.00".to_string()));

        let exdata = root.path().join("dev_hdd0/home/00000001/exdata");
        std::fs::create_dir_all(&exdata).unwrap();
        std::fs::write(exdata.join(format!("{CONTENT_ID}.edat")), b"").unwrap();
        assert_eq!(shown(), ("Full Game".to_string(), "01.02".to_string()));
    }

    #[test]
    fn user_folders_follow_rpcs3s_check_user() {
        for (name, is_user) in [
            ("00000001", true),
            ("12345678", true),
            ("0001abcd", true),
            ("00000000", false),
            ("abcdefgh", false),
            ("1234567", false),
            ("123456789", false),
        ] {
            assert_eq!(is_user_folder(std::ffi::OsStr::new(name)), is_user, "{name}");
        }
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

    /// `install_pkg` without the running-RPCS3 check, so a test never reads the real cache folder.
    #[cfg(unix)]
    async fn install(exe: &Path, pkg: &Path, check: InstallCheck) -> Result<InstallOutcome, Rpcs3Error> {
        install_pkg_guarded(exe, pkg, check, None).await
    }

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

        install(&exe, &pkg, InstallCheck::Unconfirmed).await.unwrap();

        let recorded = std::fs::read_to_string(args).unwrap();
        assert_eq!(recorded, format!("--headless\n--installpkg\n{}\n", pkg.display()));
    }

    #[test]
    fn firmware_versions_read_as_rpcs3_shows_them() {
        let dev_flash = tempfile::tempdir().unwrap();
        assert_eq!(installed_firmware_version(dev_flash.path()), None);
        std::fs::create_dir_all(dev_flash.path().join("vsh/etc")).unwrap();
        std::fs::write(dev_flash.path().join("vsh/etc/version.txt"), "release:04.9300:\nbuild:...").unwrap();
        assert_eq!(installed_firmware_version(dev_flash.path()).as_deref(), Some("4.93"));

        assert_eq!(normalize_firmware_version("4.9300").as_deref(), Some("4.93"));
        assert_eq!(normalize_firmware_version("04.8900").as_deref(), Some("4.89"));
        assert_eq!(normalize_firmware_version("4.90").as_deref(), Some("4.9"));
        assert_eq!(normalize_firmware_version("x.10"), None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn firmware_install_runs_installfw_and_confirms_the_version() {
        // RPSN-0141: headless --installfw, then version.txt decides.
        let root = tempfile::tempdir().unwrap();
        let dev_flash = root.path().join("dev_flash");
        let writes_4_93 = format!(
            "mkdir -p '{0}/vsh/etc' && printf 'release:04.9300:' > '{0}/vsh/etc/version.txt'",
            dev_flash.display()
        );
        let (exe, args, _, _) = stand_in_rpcs3(root.path(), &writes_4_93, 0);
        let pup = root.path().join("PS3UPDAT.PUP");
        std::fs::write(&pup, b"pup").unwrap();
        let check = |version: &str| {
            Some(FirmwareCheck {
                dev_flash: dev_flash.clone(),
                version: version.to_string(),
            })
        };

        let outcome = install_firmware_guarded(&exe, &pup, check("4.9300"), None).await.unwrap();
        assert_eq!(outcome, InstallOutcome::Confirmed);
        let recorded = std::fs::read_to_string(&args).unwrap();
        assert_eq!(recorded, format!("--headless\n--installfw\n{}\n", pup.display()));

        assert!(matches!(
            install_firmware_guarded(&exe, &pup, check("4.9400"), None).await,
            Err(Rpcs3Error::FirmwareNotInstalled { found: Some(ref v), .. }) if v == "4.93"
        ));
        assert_eq!(
            install_firmware_guarded(&exe, &pup, None, None).await.unwrap(),
            InstallOutcome::Unconfirmed
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn install_confirms_the_expected_version() {
        // Headless RPCS3 exits 0 whatever happened, so the version read back decides (RPSN-0137).
        let root = tempfile::tempdir().unwrap();
        let (exe, _, dev_hdd0, pkg) = stand_in_rpcs3(root.path(), WRITES_01_05, 0);
        assert_eq!(
            install(&exe, &pkg, expect(&dev_hdd0, Some("01.05"))).await.unwrap(),
            InstallOutcome::Confirmed
        );
        assert!(matches!(
            install(&exe, &pkg, expect(&dev_hdd0, Some("01.06"))).await,
            Err(Rpcs3Error::NotInstalled { found: Some(ref v), .. }) if v == "01.05"
        ));

        let root = tempfile::tempdir().unwrap();
        let (exe, _, dev_hdd0, pkg) = stand_in_rpcs3(root.path(), "", 0);
        assert!(matches!(
            install(&exe, &pkg, expect(&dev_hdd0, Some("01.05"))).await,
            Err(Rpcs3Error::NotInstalled { found: None, .. })
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn install_confirms_a_changed_version() {
        let root = tempfile::tempdir().unwrap();
        let (exe, _, dev_hdd0, pkg) = stand_in_rpcs3(root.path(), WRITES_01_05, 0);
        assert_eq!(
            install(&exe, &pkg, expect(&dev_hdd0, None)).await.unwrap(),
            InstallOutcome::Confirmed
        );

        // Now 01.05 is there before the install, and a stand-in that writes nothing changes nothing.
        let (exe, _, _, _) = stand_in_rpcs3(root.path(), "", 0);
        assert!(matches!(
            install(&exe, &pkg, expect(&dev_hdd0, None)).await,
            Err(Rpcs3Error::NotInstalled { .. })
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn install_failure_beats_a_matching_version() {
        let root = tempfile::tempdir().unwrap();
        let (exe, _, dev_hdd0, pkg) = stand_in_rpcs3(root.path(), WRITES_01_05, 1);
        assert!(matches!(
            install(&exe, &pkg, expect(&dev_hdd0, Some("01.05"))).await,
            Err(Rpcs3Error::InstallFailed(_))
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn install_without_a_check_is_unconfirmed() {
        let root = tempfile::tempdir().unwrap();
        let (exe, _, _, pkg) = stand_in_rpcs3(root.path(), WRITES_01_05, 0);
        assert_eq!(
            install(&exe, &pkg, InstallCheck::Unconfirmed).await.unwrap(),
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

        assert!(matches!(install(&exe, &pkg, check).await, Err(Rpcs3Error::InstallFailed(_))));
        assert!(!args.exists(), "RPCS3 was launched");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn install_refuses_while_rpcs3_is_open() {
        // A second RPCS3 shows a fatal-error window before it reads --headless (RPSN-0138).
        let root = tempfile::tempdir().unwrap();
        let (exe, args, _, pkg) = stand_in_rpcs3(root.path(), "", 0);
        let lock_file = root.path().join("RPCS3.buf");
        let open_rpcs3 = std::fs::File::create(&lock_file).unwrap();
        open_rpcs3.lock().unwrap();

        let result = install_pkg_guarded(&exe, &pkg, InstallCheck::Unconfirmed, Some(&lock_file)).await;

        assert!(matches!(result, Err(Rpcs3Error::AlreadyRunning)));
        assert!(!args.exists(), "RPCS3 was launched");

        // Once RPCS3 has closed, the install runs and leaves the file unlocked.
        drop(open_rpcs3);
        let result = install_pkg_guarded(&exe, &pkg, InstallCheck::Unconfirmed, Some(&lock_file)).await;
        assert_eq!(result.unwrap(), InstallOutcome::Unconfirmed);
        assert!(std::fs::File::open(&lock_file).unwrap().try_lock().is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn installs_run_one_at_a_time() {
        let root = tempfile::tempdir().unwrap();
        let log = root.path().join("log");
        let action = format!("echo start >> '{0}'; sleep 0.3; echo end >> '{0}'", log.display());
        let (exe, _, _, pkg) = stand_in_rpcs3(root.path(), &action, 0);
        let lock_file = root.path().join("RPCS3.buf");
        std::fs::File::create(&lock_file).unwrap();

        let (first, second) = tokio::join!(
            install_pkg_guarded(&exe, &pkg, InstallCheck::Unconfirmed, Some(&lock_file)),
            install_pkg_guarded(&exe, &pkg, InstallCheck::Unconfirmed, Some(&lock_file)),
        );

        first.unwrap();
        second.unwrap();
        assert_eq!(std::fs::read_to_string(log).unwrap(), "start\nend\nstart\nend\n");
    }

    #[test]
    fn no_lock_file_means_rpcs3_is_not_running() {
        let root = tempfile::tempdir().unwrap();
        assert!(!rpcs3_is_running(&root.path().join("RPCS3.buf")));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn lock_path_follows_rpcs3s_cache_dir() {
        fn env<'a>(vars: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
            move |key| vars.iter().find(|(k, _)| *k == key).map(|(_, v)| OsString::from(v))
        }
        let exe = Path::new("/opt/rpcs3/rpcs3");
        let all = [("XDG_CACHE_HOME", "/c"), ("XDG_CONFIG_HOME", "/f"), ("HOME", "/h")];

        assert_eq!(lock_path_from(exe, env(&all)), Some(PathBuf::from("/c/rpcs3/RPCS3.buf")));
        assert_eq!(lock_path_from(exe, env(&all[1..])), Some(PathBuf::from("/f/rpcs3/RPCS3.buf")));
        assert_eq!(
            lock_path_from(exe, env(&all[2..])),
            Some(PathBuf::from("/h/.cache/rpcs3/RPCS3.buf"))
        );
        assert_eq!(lock_path_from(exe, env(&[])), None);
    }

    #[test]
    fn flatpak_lock_path_is_inside_its_sandbox() {
        let env = |key: &str| (key == "HOME").then(|| OsString::from("/h"));
        assert_eq!(
            lock_path_from(Path::new("flatpak:net.rpcs3.RPCS3"), env),
            Some(PathBuf::from("/h/.var/app/net.rpcs3.RPCS3/cache/rpcs3/RPCS3.buf"))
        );
        assert_eq!(lock_path_from(Path::new("flatpak:../x"), env), None);
        assert_eq!(lock_path_from(Path::new("flatpak:a/b"), env), None);
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
