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
        // Check PATH via `which`
        if let Ok(output) = std::process::Command::new("which").arg("rpcs3").output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Some(PathBuf::from(path));
                }
            }
        }
        // Check if Flatpak is available
        if let Ok(output) = std::process::Command::new("flatpak")
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
        // Look for rpcs3.exe near the config directory or up two levels
        let candidates = [config_dir.join("rpcs3.exe"), config_dir.join("../../rpcs3.exe").to_path_buf()];
        for candidate in &candidates {
            if candidate.exists() {
                return Some(candidate.canonicalize().unwrap_or_else(|_| candidate.clone()));
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("which").arg("rpcs3").output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Some(PathBuf::from(path));
                }
            }
        }
    }

    None
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
            // Also scan parent for any *.AppImage files
            if let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let entry_name = entry.file_name().to_string_lossy().to_string();
                    if entry_name.ends_with(".AppImage") && entry.path().is_file() {
                        return Some(entry.path());
                    }
                }
            }
            return None;
        }
        current = current.parent()?;
    }
}

/// Build a list of candidate RPCS3 config directories to check, in priority order.
fn get_candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "linux")]
    {
        // Standard Linux config location.
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".config/rpcs3"));
        }

        // Flatpak installation.
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".var/app/net.rpcs3.RPCS3/config/rpcs3"));
        }

        // AppImage pattern: look for *.AppImage.home/.config/rpcs3/ in common emulator dirs.
        if let Some(home) = dirs::home_dir() {
            let appimage_search_dirs = [
                home.join("Applications"),
                home.join("Games"),
                PathBuf::from("/opt"),
                PathBuf::from("/mnt"),
            ];
            for search_dir in &appimage_search_dirs {
                if let Some(found) = scan_for_appimage_config(search_dir) {
                    paths.push(found);
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = dirs::config_dir() {
            paths.push(appdata.join("rpcs3"));
        }
        if let Some(local) = dirs::data_local_dir() {
            paths.push(local.join("rpcs3"));
        }
        // Alongside rpcs3.exe in Program Files.
        paths.push(PathBuf::from("C:\\Program Files\\RPCS3"));
        paths.push(PathBuf::from("C:\\Program Files (x86)\\RPCS3"));
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join("Library/Application Support/rpcs3"));
        }
    }

    paths
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
