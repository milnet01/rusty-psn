use std::path::PathBuf;

use log::info;

use super::Rpcs3Installation;

/// Attempt to auto-detect an RPCS3 installation by checking common paths.
///
/// Returns `None` if no installation is found. The user can always configure the path manually.
pub fn detect_rpcs3() -> Option<Rpcs3Installation> {
    let candidates = get_candidate_paths();

    for config_dir in candidates {
        if config_dir.join("games.yml").exists() {
            info!("Auto-detected RPCS3 config at: {}", config_dir.display());

            let dev_hdd0_path = super::config::resolve_dev_hdd0(&config_dir);

            let executable = detect_executable(&config_dir);
            if let Some(ref exe) = executable {
                info!("Auto-detected RPCS3 executable: {}", exe.display());
            }

            return Some(Rpcs3Installation {
                config_dir,
                dev_hdd0_path,
                executable,
            });
        }
    }

    None
}

/// Attempt to find the RPCS3 executable given a detected config directory.
fn detect_executable(config_dir: &std::path::Path) -> Option<PathBuf> {
    // A portable config sits in the folder holding RPCS3 (fs::get_config_dir in
    // Utilities/File.cpp, RPCS3 commit 6fe3c87).
    if config_dir.file_name().is_some_and(|name| name == "portable") {
        if let Some(exe) = config_dir.parent().and_then(executable_in) {
            return Some(exe);
        }
    }

    // 1. AppImage: config is inside *.AppImage.home/.config/rpcs3/
    //    The executable is the sibling *.AppImage file next to the .home directory.
    if let Some(exe) = detect_appimage_executable(config_dir) {
        return Some(exe);
    }

    // 2. Portable/installed: rpcs3 binary alongside or near the config directory.
    #[cfg(target_os = "linux")]
    {
        // Check common locations relative to config
        let candidates = [config_dir.join("rpcs3"), config_dir.join("../../rpcs3").to_path_buf()];
        for candidate in &candidates {
            if candidate.exists() && candidate.is_file() {
                return Some(candidate.canonicalize().unwrap_or_else(|_| candidate.clone()));
            }
        }
        if let Some(path) = rpcs3_on_path() {
            return Some(path);
        }
        // Check if Flatpak is available
        if let Ok(output) = crate::sandbox::host_command("flatpak")
            .args(["info", "net.rpcs3.RPCS3"])
            .output()
        {
            if output.status.success() {
                // Return a sentinel path — install_pkg will need to handle this specially
                return Some(PathBuf::from("flatpak:net.rpcs3.RPCS3"));
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // rpcs3.exe sits in RPCS3's own folder: the config folder itself (older versions),
        // its parent (<RPCS3>\\config), or two up (<RPCS3>\\portable\\config).
        let candidates = [
            config_dir.join("rpcs3.exe"),
            config_dir.join("../rpcs3.exe"),
            config_dir.join("../../rpcs3.exe"),
        ];
        for candidate in &candidates {
            if candidate.exists() {
                return Some(candidate.canonicalize().unwrap_or_else(|_| candidate.clone()));
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(path) = rpcs3_on_path() {
            return Some(path);
        }
        // RPCS3 ships as RPCS3.app, which is not on PATH.
        if let Some(exe) = find_app_bundle_executable(&mac_app_dirs()) {
            return Some(exe);
        }
    }

    None
}

/// `which rpcs3` on the host, or `None` where it finds nothing.
#[cfg(unix)]
fn rpcs3_on_path() -> Option<PathBuf> {
    let output = crate::sandbox::host_command("which").arg("rpcs3").output().ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !path.is_empty()).then(|| PathBuf::from(path))
}

/// The folders macOS keeps applications in, where RPCS3.app lives.
#[cfg(target_os = "macos")]
fn mac_app_dirs() -> Vec<PathBuf> {
    let mut app_dirs = vec![PathBuf::from("/Applications")];
    if let Some(home) = dirs::home_dir() {
        app_dirs.push(home.join("Applications"));
    }
    app_dirs
}

/// RPCS3's executable inside `dir`: an `rpcs3` binary or an RPCS3 AppImage.
#[cfg(not(any(target_os = "macos", windows)))]
fn executable_in(dir: &std::path::Path) -> Option<PathBuf> {
    let binary = dir.join("rpcs3");
    if binary.is_file() {
        return Some(binary);
    }
    rpcs3_appimage_in(dir)
}

/// RPCS3's executable inside `dir`: the binary inside an RPCS3.app bundle.
#[cfg(target_os = "macos")]
fn executable_in(dir: &std::path::Path) -> Option<PathBuf> {
    find_app_bundle_executable(&[dir.to_path_buf()])
}

/// Windows finds rpcs3.exe from its own config candidates in `detect_executable`.
#[cfg(windows)]
fn executable_in(_dir: &std::path::Path) -> Option<PathBuf> {
    None
}

/// An AppImage in `dir` whose name says rpcs3. The name must say so, because install_pkg
/// runs whatever detection returns with --installpkg.
fn rpcs3_appimage_in(dir: &std::path::Path) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().map(|entry| entry.path()).find(|path| {
        path.file_name().is_some_and(|name| {
            let name = name.to_string_lossy();
            name.ends_with(".AppImage") && name.to_lowercase().contains("rpcs3")
        }) && path.is_file()
    })
}

/// The binary inside the first `RPCS3.app` found in `app_dirs`.
///
/// RPCS3's macOS release names the bundle RPCS3.app and its binary rpcs3
/// (.ci/deploy-mac.sh and rpcs3/rpcs3.plist.in, RPCS3 commit 6fe3c87).
#[cfg(any(target_os = "macos", test))]
fn find_app_bundle_executable(app_dirs: &[PathBuf]) -> Option<PathBuf> {
    app_dirs
        .iter()
        .map(|dir| dir.join("RPCS3.app/Contents/MacOS/rpcs3"))
        .find(|exe| exe.is_file())
}

/// Detect RPCS3 AppImage executable from a config directory inside `.AppImage.home/`.
fn detect_appimage_executable(config_dir: &std::path::Path) -> Option<PathBuf> {
    // Walk up from config_dir looking for a directory named *.AppImage.home
    let mut current = config_dir;
    loop {
        let name = current.file_name()?.to_string_lossy();
        if name.ends_with(".AppImage.home") {
            // The AppImage is a sibling file: strip ".home" from the directory name
            let appimage_name = name.trim_end_matches(".home");
            let parent = current.parent()?;
            let appimage_path = parent.join(appimage_name);
            if appimage_path.exists() && appimage_path.is_file() {
                return Some(appimage_path);
            }
            // Fall back to a renamed RPCS3 AppImage beside it.
            return rpcs3_appimage_in(parent);
        }
        current = current.parent()?;
    }
}

/// Build a list of candidate RPCS3 config directories to check, in priority order.
fn get_candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "linux")]
    {
        let home = dirs::home_dir();
        let appimage_search_dirs = home
            .as_ref()
            .map(|home| {
                vec![
                    home.join("Applications"),
                    home.join("Games"),
                    PathBuf::from("/opt"),
                    PathBuf::from("/mnt"),
                ]
            })
            .unwrap_or_default();

        // RPCS3 resolves its own path through /proc/self/exe, so a symlink on PATH counts
        // as the folder it points into.
        let mut exe_dirs: Vec<PathBuf> = rpcs3_on_path()
            .and_then(|exe| exe.canonicalize().ok())
            .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
            .into_iter()
            .collect();
        for search_dir in &appimage_search_dirs {
            exe_dirs.extend(appimage_dirs(search_dir));
        }
        let xdg_config = crate::sandbox::host_env("XDG_CONFIG_HOME")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from);
        paths.extend(linux_config_candidates(&exe_dirs, xdg_config, home));

        // AppImage pattern: look for *.AppImage.home/.config/rpcs3/ in common emulator dirs.
        paths.extend(appimage_search_dirs.iter().filter_map(|dir| scan_for_appimage_config(dir)));
    }

    #[cfg(target_os = "windows")]
    {
        let env_dir = std::env::var_os("RPCS3_CONFIG_DIR").map(PathBuf::from);
        let exe_dir = std::env::var_os("PATH")
            .and_then(|path| find_on_path("rpcs3.exe", &path))
            .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf));
        paths.extend(windows_config_candidates(env_dir, exe_dir));
    }

    #[cfg(target_os = "macos")]
    {
        // A portable folder beside RPCS3.app comes first, as RPCS3 chooses it.
        let app_dirs = mac_app_dirs();
        paths.extend(
            app_dirs
                .iter()
                .filter(|dir| dir.join("RPCS3.app").is_dir())
                .map(|dir| dir.join("portable")),
        );
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join("Library/Application Support/rpcs3"));
        }
    }

    paths
}

/// Linux config folders to try, in the order RPCS3 itself chooses its folder: a `portable`
/// folder beside the executable, then `$XDG_CONFIG_HOME/rpcs3`, then `~/.config/rpcs3`
/// (fs::get_config_dir in Utilities/File.cpp, RPCS3 commit 6fe3c87); then the Flatpak's.
/// `exe_dirs` are the folders holding an RPCS3 binary or AppImage.
#[cfg(any(target_os = "linux", test))]
fn linux_config_candidates(exe_dirs: &[PathBuf], xdg_config: Option<PathBuf>, home: Option<PathBuf>) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = exe_dirs.iter().map(|dir| dir.join("portable")).collect();
    paths.extend(xdg_config.map(|dir| dir.join("rpcs3")));
    if let Some(home) = home {
        paths.push(home.join(".config/rpcs3"));
        paths.push(home.join(".var/app/net.rpcs3.RPCS3/config/rpcs3"));
    }
    paths
}

/// `search_dir` and each folder directly inside it that holds an RPCS3 AppImage.
#[cfg(target_os = "linux")]
fn appimage_dirs(search_dir: &std::path::Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(search_dir) else {
        return Vec::new();
    };
    std::iter::once(search_dir.to_path_buf())
        .chain(entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()))
        .filter(|dir| rpcs3_appimage_in(dir).is_some())
        .collect()
}

/// Windows config folders to try, in the order RPCS3 itself chooses its folder: a
/// `portable` folder beside rpcs3.exe, then RPCS3_CONFIG_DIR, then the exe's own folder
/// (fs::get_config_dir in Utilities/File.cpp, RPCS3 commit 6fe3c87). Each folder's
/// `config` subfolder comes first; older RPCS3 versions kept games.yml in the folder itself.
#[cfg(any(target_os = "windows", test))]
fn windows_config_candidates(env_dir: Option<PathBuf>, exe_dir: Option<PathBuf>) -> Vec<PathBuf> {
    let bases = [
        exe_dir.as_ref().map(|dir| dir.join("portable")),
        env_dir,
        exe_dir,
        Some(PathBuf::from("C:\\Program Files\\RPCS3")),
    ];
    bases
        .into_iter()
        .flatten()
        .flat_map(|base| [base.join("config"), base])
        .collect()
}

/// The first `name` file in a PATH-style list of folders.
#[cfg(any(target_os = "windows", test))]
fn find_on_path(name: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Scan a directory (non-recursively) for RPCS3 AppImage config patterns.
/// Looks for directories matching `*.AppImage.home/.config/rpcs3/`.
#[cfg(target_os = "linux")]
fn scan_for_appimage_config(search_dir: &std::path::Path) -> Option<PathBuf> {
    // Only scan one level deep to avoid long searches.
    let entries = match std::fs::read_dir(search_dir) {
        Ok(e) => e,
        Err(_) => return None,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        // Check for AppImage home directory pattern.
        let name = path.file_name()?.to_string_lossy();
        if name.ends_with(".AppImage.home") || name.contains("RPCS3") || name.contains("rpcs3") {
            let config_candidate = path.join(".config/rpcs3");
            if config_candidate.join("games.yml").exists() {
                return Some(config_candidate);
            }
        }

        // Also check one level deeper (e.g., /opt/emulators/ps3/RPCS3/)
        if let Ok(sub_entries) = std::fs::read_dir(&path) {
            for sub in sub_entries.flatten() {
                let sub_name = sub.file_name().to_string_lossy().to_string();
                if sub_name.ends_with(".AppImage.home") {
                    let config_candidate = sub.path().join(".config/rpcs3");
                    if config_candidate.join("games.yml").exists() {
                        return Some(config_candidate);
                    }
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder holding `rpcs3-old.AppImage.home/.config/rpcs3` but not `rpcs3-old.AppImage`, plus `others`.
    fn appimage_folder(others: &[&str]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let config_dir = dir.path().join("rpcs3-old.AppImage.home/.config/rpcs3");
        std::fs::create_dir_all(&config_dir).expect("config dir");
        for name in others {
            std::fs::write(dir.path().join(name), b"").expect("appimage file");
        }
        (dir, config_dir)
    }

    #[test]
    fn windows_candidates_follow_rpcs3s_own_order() {
        // RPCS3 on Windows keeps everything beside rpcs3.exe, never in AppData (RPSN-0115).
        let exe = PathBuf::from("/rpcs3");
        let env = PathBuf::from("/custom");

        let paths = windows_config_candidates(Some(env.clone()), Some(exe.clone()));

        assert_eq!(
            paths[..6],
            [
                exe.join("portable/config"),
                exe.join("portable"),
                env.join("config"),
                env,
                exe.join("config"),
                exe,
            ]
        );
    }

    #[test]
    fn find_on_path_takes_the_first_folder_holding_the_file() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        std::fs::write(second.path().join("rpcs3.exe"), b"").unwrap();
        let path = std::env::join_paths([first.path(), second.path()]).unwrap();

        assert_eq!(find_on_path("rpcs3.exe", &path), Some(second.path().join("rpcs3.exe")));
    }

    #[test]
    fn app_bundle_is_found_in_the_second_applications_folder() {
        // On macOS RPCS3 is an app bundle, not a command on PATH (RPSN-0114).
        let system = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        let exe = user.path().join("RPCS3.app/Contents/MacOS/rpcs3");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();

        let found = find_app_bundle_executable(&[system.path().to_path_buf(), user.path().to_path_buf()]);

        assert_eq!(found, Some(exe));
    }

    #[test]
    fn app_bundle_is_not_found_where_there_is_none() {
        let empty = tempfile::tempdir().unwrap();

        assert_eq!(find_app_bundle_executable(&[empty.path().to_path_buf()]), None);
    }

    #[test]
    fn appimage_fallback_ignores_an_unrelated_appimage() {
        // Another program's AppImage must never be run with --installpkg (RPSN-0110).
        let (_dir, config_dir) = appimage_folder(&["Krita.AppImage"]);
        assert_eq!(detect_appimage_executable(&config_dir), None);
    }

    #[test]
    fn appimage_fallback_takes_a_renamed_rpcs3() {
        let (dir, config_dir) = appimage_folder(&["Krita.AppImage", "RPCS3-v0.0.38.AppImage"]);
        assert_eq!(
            detect_appimage_executable(&config_dir),
            Some(dir.path().join("RPCS3-v0.0.38.AppImage"))
        );
    }

    #[test]
    fn linux_candidates_follow_rpcs3s_own_order() {
        // RPCS3 takes a portable folder first, then XDG_CONFIG_HOME, then ~/.config (RPSN-0139).
        let home = PathBuf::from("/h");
        assert_eq!(
            linux_config_candidates(&[PathBuf::from("/a")], Some(PathBuf::from("/x")), Some(home.clone())),
            [
                PathBuf::from("/a/portable"),
                PathBuf::from("/x/rpcs3"),
                PathBuf::from("/h/.config/rpcs3"),
                PathBuf::from("/h/.var/app/net.rpcs3.RPCS3/config/rpcs3"),
            ]
        );
        assert_eq!(
            linux_config_candidates(&[], None, Some(home))[0],
            PathBuf::from("/h/.config/rpcs3")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn portable_config_finds_the_rpcs3_beside_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let portable = dir.path().join("portable");
        std::fs::create_dir(&portable).expect("portable dir");
        std::fs::write(dir.path().join("RPCS3-v0.0.38.AppImage"), b"").expect("appimage file");
        assert_eq!(detect_executable(&portable), Some(dir.path().join("RPCS3-v0.0.38.AppImage")));

        std::fs::write(dir.path().join("rpcs3"), b"").expect("binary");
        assert_eq!(detect_executable(&portable), Some(dir.path().join("rpcs3")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn appimage_dirs_lists_folders_holding_an_rpcs3_appimage() {
        let dir = tempfile::tempdir().expect("temp dir");
        for (folder, file) in [("ps3", "rpcs3.AppImage"), ("art", "Krita.AppImage")] {
            std::fs::create_dir(dir.path().join(folder)).expect("folder");
            std::fs::write(dir.path().join(folder).join(file), b"").expect("appimage file");
        }
        assert_eq!(appimage_dirs(dir.path()), [dir.path().join("ps3")]);
    }
}
