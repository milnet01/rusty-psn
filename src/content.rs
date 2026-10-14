//! Local content library — scans a directory of PS3 patches and DLC.
//!
//! Expected directory structure:
//! ```text
//! content_dir/
//! ├── Game Name/
//! │   ├── Patches/
//! │   │   └── SERIAL/
//! │   │       └── *.pkg
//! │   └── DLC/
//! │       └── *.zip  (containing .pkg + .rap / .edat)
//! ```

use std::path::{Path, PathBuf};

use log::{info, warn};

/// A scanned content library.
#[allow(dead_code)] // base_path retained for future CLI content browse support
pub struct ContentLibrary {
    pub games: Vec<ContentGame>,
    pub base_path: PathBuf,
}

/// A game entry in the content library.
pub struct ContentGame {
    pub name: String,
    pub patches: Vec<ContentPatch>,
    pub dlc: Vec<ContentDlc>,
}

/// A patch PKG file found in the Patches/ subdirectory.
pub struct ContentPatch {
    pub serial: String,
    pub pkg_path: PathBuf,
    pub pkg_filename: String,
}

/// A DLC ZIP file found in the DLC/ subdirectory.
pub struct ContentDlc {
    pub zip_path: PathBuf,
    pub zip_filename: String,
    pub size: u64,
}

/// Errors that can occur during content operations.
#[derive(Debug)]
pub enum ContentError {
    Io(std::io::Error),
    Zip(zip::result::ZipError),
    NoPkgFound,
    TooLarge,
    DuplicateFile(String),
    InstallFailed(String),
}

impl std::fmt::Display for ContentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContentError::Io(e) => write!(f, "I/O error: {e}"),
            ContentError::Zip(e) => write!(f, "ZIP error: {e}"),
            ContentError::NoPkgFound => write!(f, "No .pkg file found in ZIP"),
            ContentError::TooLarge => write!(f, "ZIP unpacks to far more than its own size"),
            ContentError::DuplicateFile(name) => write!(f, "ZIP holds two files named {name}"),
            ContentError::InstallFailed(reason) => write!(f, "Install failed: {reason}"),
        }
    }
}

/// Scan a content directory and return a library of games with their patches and DLC.
pub fn scan_content_directory(base_path: &Path) -> ContentLibrary {
    let mut games = Vec::new();

    let entries = match std::fs::read_dir(base_path) {
        Ok(e) => e,
        Err(e) => {
            warn!("Cannot read content directory {}: {e}", base_path.display());
            return ContentLibrary {
                games,
                base_path: base_path.to_path_buf(),
            };
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();

        // Skip Firmware directory
        if name == "Firmware" {
            continue;
        }

        let patches_dir = path.join("Patches");
        let dlc_dir = path.join("DLC");

        let mut patches = Vec::new();
        let mut dlc = Vec::new();

        // Scan Patches/ subdirectory: Patches/SERIAL/*.pkg
        if patches_dir.is_dir() {
            if let Ok(serial_entries) = std::fs::read_dir(&patches_dir) {
                for serial_entry in serial_entries.flatten() {
                    let serial_path = serial_entry.path();
                    if !serial_path.is_dir() {
                        continue;
                    }
                    let serial = serial_entry.file_name().to_string_lossy().to_string();

                    if let Ok(pkg_entries) = std::fs::read_dir(&serial_path) {
                        for pkg_entry in pkg_entries.flatten() {
                            let pkg_path = pkg_entry.path();
                            if pkg_path.is_file() && pkg_path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pkg")) {
                                patches.push(ContentPatch {
                                    serial: serial.clone(),
                                    pkg_filename: pkg_entry.file_name().to_string_lossy().to_string(),
                                    pkg_path,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Scan DLC/ subdirectory: DLC/*.zip or DLC/subfolder/*.zip or DLC/subfolder/{pkg,rap}
        if dlc_dir.is_dir() {
            if let Ok(dlc_entries) = std::fs::read_dir(&dlc_dir) {
                for dlc_entry in dlc_entries.flatten() {
                    let dlc_path = dlc_entry.path();
                    if dlc_path.is_file() && dlc_path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
                        let size = std::fs::metadata(&dlc_path).map(|m| m.len()).unwrap_or(0);
                        dlc.push(ContentDlc {
                            zip_filename: dlc_entry.file_name().to_string_lossy().to_string(),
                            zip_path: dlc_path,
                            size,
                        });
                    }
                }
            }
        }

        // Sort for consistent display
        patches.sort_by(|a, b| a.serial.cmp(&b.serial).then(a.pkg_filename.cmp(&b.pkg_filename)));
        dlc.sort_by(|a, b| a.zip_filename.cmp(&b.zip_filename));

        if !patches.is_empty() || !dlc.is_empty() {
            games.push(ContentGame { name, patches, dlc });
        }
    }

    games.sort_by(|a, b| a.name.cmp(&b.name));
    info!(
        "Content library scan: {} games ({} total patches, {} total DLC)",
        games.len(),
        games.iter().map(|g| g.patches.len()).sum::<usize>(),
        games.iter().map(|g| g.dlc.len()).sum::<usize>(),
    );

    ContentLibrary {
        games,
        base_path: base_path.to_path_buf(),
    }
}

/// The packages and licences extracted from a DLC ZIP, in archive order.
/// The files sit directly in `temp_dir`, which is removed when this is dropped.
pub struct ExtractedDlc {
    pub pkgs: Vec<PathBuf>,
    pub raps: Vec<PathBuf>,
    pub edats: Vec<PathBuf>,
    pub temp_dir: tempfile::TempDir,
}

/// How far past its own size a DLC ZIP may unpack. Packages and licences are encrypted,
/// so they barely compress; the fixed part covers small or stored archives.
const DLC_UNPACK_RATIO: u64 = 4;
const DLC_UNPACK_SLACK: u64 = 16 * 1024 * 1024;

/// Extract every PKG, RAP and EDAT file in a DLC ZIP into the top of one temporary folder.
///
/// RPCS3's `--installpkg` reads only the files at the top of a folder it is given, so
/// the ZIP's own subfolders are dropped. Two files with the same name are refused.
/// The folder sits beside the ZIP: the system temp folder is RAM on many systems, and a
/// Flatpak RPCS3 cannot see it. A ZIP that unpacks far past its own size is refused.
pub fn extract_dlc_zip(zip_path: &Path) -> Result<ExtractedDlc, ContentError> {
    let file = std::fs::File::open(zip_path).map_err(ContentError::Io)?;
    let zip_size = file.metadata().map_err(ContentError::Io)?.len();
    let mut budget = zip_size.saturating_mul(DLC_UNPACK_RATIO).saturating_add(DLC_UNPACK_SLACK);
    let mut archive = zip::ZipArchive::new(file).map_err(ContentError::Zip)?;

    let temp_dir = match zip_path
        .parent()
        .map(|dir| tempfile::Builder::new().prefix(".rusty-psn-dlc-").tempdir_in(dir))
    {
        Some(Ok(dir)) => dir,
        other => {
            if let Some(Err(e)) = other {
                warn!(
                    "Cannot unpack beside {}: {e}; using the system temp folder",
                    zip_path.display()
                );
            }
            tempfile::tempdir().map_err(ContentError::Io)?
        }
    };

    let mut pkgs = Vec::new();
    let mut raps = Vec::new();
    let mut edats = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(ContentError::Zip)?;
        if file.is_dir() {
            continue;
        }
        let Some(enclosed_name) = file.enclosed_name() else {
            continue; // Skip entries with suspicious paths
        };
        let Some(file_name) = enclosed_name.file_name() else {
            continue;
        };
        let ext = Path::new(file_name).extension().and_then(|e| e.to_str()).unwrap_or("");
        let list = if ext.eq_ignore_ascii_case("pkg") {
            &mut pkgs
        } else if ext.eq_ignore_ascii_case("rap") {
            &mut raps
        } else if ext.eq_ignore_ascii_case("edat") {
            &mut edats
        } else {
            continue;
        };
        // Case-insensitive, as RPCS3 lower-cases licence extensions and Windows ignores case.
        if !seen.insert(file_name.to_string_lossy().to_lowercase()) {
            return Err(ContentError::DuplicateFile(file_name.to_string_lossy().into_owned()));
        }

        // The header's size can lie, so the copy itself is bounded too.
        if file.size() > budget {
            return Err(ContentError::TooLarge);
        }
        let outpath = temp_dir.path().join(file_name);
        let mut outfile = std::fs::File::create(&outpath).map_err(ContentError::Io)?;
        let written = std::io::copy(&mut std::io::Read::take(&mut file, budget + 1), &mut outfile).map_err(ContentError::Io)?;
        if written > budget {
            return Err(ContentError::TooLarge);
        }
        budget -= written;
        list.push(outpath);
    }

    if pkgs.is_empty() {
        return Err(ContentError::NoPkgFound);
    }
    Ok(ExtractedDlc {
        pkgs,
        raps,
        edats,
        temp_dir,
    })
}

/// Install a DLC ZIP to RPCS3: extract it, then hand RPCS3 the one folder.
///
/// RPCS3 copies the RAP and EDAT licences into its own exdata folder, then installs
/// every PKG. rusty-psn cannot see whether the licences landed: it reports what
/// `install_pkg` reports.
pub async fn install_dlc_zip(rpcs3_exe: &Path, zip_path: &Path) -> Result<(), ContentError> {
    info!("Installing DLC from ZIP: {}", zip_path.display());

    // Extract (sync, but fast — zips are small)
    let dlc = extract_dlc_zip(zip_path)?;
    info!(
        "Handing RPCS3 {} package(s), {} RAP and {} EDAT licence(s)",
        dlc.pkgs.len(),
        dlc.raps.len(),
        dlc.edats.len()
    );

    crate::rpcs3::install_pkg(rpcs3_exe, dlc.temp_dir.path())
        .await
        .map_err(|e| ContentError::InstallFailed(e.to_string()))?;

    // dlc.temp_dir drops here, removing the extracted files
    info!("DLC installation complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn extract_dlc_zip_inflates_entries_and_skips_escaping_paths() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("dlc.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in [
            ("game/dlc.pkg", b"pkg body"),
            ("game/dlc.rap", b"rap body"),
            ("../escape.pkg", b"bad body"),
        ] {
            writer.start_file(name, options).unwrap();
            writer.write_all(body).unwrap();
        }
        writer.finish().unwrap();

        let dlc = extract_dlc_zip(&zip_path).unwrap();

        assert_eq!(dlc.pkgs.len(), 1);
        assert_eq!(std::fs::read(&dlc.pkgs[0]).unwrap(), b"pkg body");
        assert_eq!(std::fs::read(&dlc.raps[0]).unwrap(), b"rap body");
        assert!(dlc.pkgs[0].starts_with(dlc.temp_dir.path()));
        assert!(!dlc.temp_dir.path().parent().unwrap().join("escape.pkg").exists());
    }

    #[test]
    fn extract_dlc_zip_returns_every_pkg_and_rap() {
        // A bundle used to install only its last package and licence (RPSN-0111).
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("bundle.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for name in ["a.pkg", "a.rap", "b.PKG", "b.rap", "readme.txt"] {
            writer.start_file(name, options).unwrap();
            writer.write_all(name.as_bytes()).unwrap();
        }
        writer.finish().unwrap();

        let dlc = extract_dlc_zip(&zip_path).unwrap();
        let names = |paths: &[PathBuf]| -> Vec<String> {
            paths
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect()
        };

        assert_eq!(names(&dlc.pkgs), ["a.pkg", "b.PKG"]);
        assert_eq!(names(&dlc.raps), ["a.rap", "b.rap"]);
    }

    /// The file names directly inside `dir`, sorted; a subfolder is listed with a trailing `/`.
    fn top_level_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                let name = entry.file_name().to_string_lossy().into_owned();
                if entry.path().is_dir() {
                    format!("{name}/")
                } else {
                    name
                }
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn extract_dlc_zip_puts_packages_and_licences_at_the_top_of_one_folder() {
        // RPCS3 reads only the files at the top of the folder it is given (RPSN-0112).
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("dlc.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for name in ["game/dlc.pkg", "game/licence/dlc.rap", "game/dlc.EDAT", "game/readme.txt"] {
            writer.start_file(name, options).unwrap();
            writer.write_all(name.as_bytes()).unwrap();
        }
        writer.finish().unwrap();

        let dlc = extract_dlc_zip(&zip_path).unwrap();

        assert_eq!(top_level_names(dlc.temp_dir.path()), ["dlc.EDAT", "dlc.pkg", "dlc.rap"]);
    }

    #[test]
    fn extract_dlc_zip_unpacks_beside_the_zip() {
        // The system temp folder is RAM on many systems, and Flatpak RPCS3 cannot see it (RPSN-0113).
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("dlc.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        writer
            .start_file("dlc.pkg", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"pkg body").unwrap();
        writer.finish().unwrap();

        let dlc = extract_dlc_zip(&zip_path).unwrap();

        assert_eq!(dlc.temp_dir.path().parent(), Some(dir.path()));
    }

    #[test]
    fn extract_dlc_zip_refuses_a_zip_that_unpacks_far_past_its_own_size() {
        // Encrypted packages barely compress, so a large expansion is not a DLC zip (RPSN-0113).
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("bomb.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        writer.start_file("bomb.pkg", options).unwrap();
        let zeros = vec![0u8; 1024 * 1024];
        for _ in 0..20 {
            writer.write_all(&zeros).unwrap();
        }
        writer.finish().unwrap();

        let result = extract_dlc_zip(&zip_path);

        assert!(
            matches!(result, Err(ContentError::TooLarge)),
            "a 20 MiB expansion of a tiny zip was unpacked"
        );
    }

    #[test]
    fn extract_dlc_zip_refuses_two_files_with_one_name() {
        // Flattening would let the second overwrite the first.
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("dlc.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for name in ["a/dlc.pkg", "b/DLC.PKG"] {
            writer.start_file(name, options).unwrap();
            writer.write_all(name.as_bytes()).unwrap();
        }
        writer.finish().unwrap();

        assert!(matches!(extract_dlc_zip(&zip_path), Err(ContentError::DuplicateFile(name)) if name == "DLC.PKG"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn install_dlc_zip_hands_rpcs3_one_folder_with_every_file() {
        // RPCS3 places the licences itself when given the folder (RPSN-0112).
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("dlc.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for name in ["a.pkg", "a.rap", "b.pkg", "b.edat"] {
            writer.start_file(name, options).unwrap();
            writer.write_all(name.as_bytes()).unwrap();
        }
        writer.finish().unwrap();

        // A stand-in RPCS3 that records each --installpkg argument and what it held.
        let log = dir.path().join("calls.log");
        let fake = dir.path().join("rpcs3");
        std::fs::write(
            &fake,
            format!("#!/bin/sh\necho \"$2\" >> '{0}'\nls \"$2\" >> '{0}'\n", log.display()),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

        install_dlc_zip(&fake, &zip_path).await.unwrap();

        let calls = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = calls.lines().collect();
        assert_eq!(lines.len(), 5, "one call naming a folder, then its four files: {calls}");
        assert_eq!(&lines[1..], ["a.pkg", "a.rap", "b.edat", "b.pkg"]);
    }
}
