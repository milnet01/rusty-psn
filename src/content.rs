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
//! │       └── *.zip  (containing .pkg + .rap)
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
    InstallFailed(String),
}

impl std::fmt::Display for ContentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContentError::Io(e) => write!(f, "I/O error: {e}"),
            ContentError::Zip(e) => write!(f, "ZIP error: {e}"),
            ContentError::NoPkgFound => write!(f, "No .pkg file found in ZIP"),
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

/// Extract a DLC ZIP to a temporary directory and return paths to the PKG and RAP files.
///
/// The caller is responsible for cleaning up the temp directory (or use `tempfile::TempDir`
/// which auto-cleans on drop).
pub fn extract_dlc_zip(zip_path: &Path) -> Result<(PathBuf, Option<PathBuf>, tempfile::TempDir), ContentError> {
    let file = std::fs::File::open(zip_path).map_err(ContentError::Io)?;
    let mut archive = zip::ZipArchive::new(file).map_err(ContentError::Zip)?;

    let temp_dir = tempfile::tempdir().map_err(ContentError::Io)?;

    let mut pkg_path = None;
    let mut rap_path = None;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(ContentError::Zip)?;
        let Some(enclosed_name) = file.enclosed_name() else {
            continue; // Skip entries with suspicious paths
        };
        let file_name = enclosed_name.to_path_buf();

        let outpath = temp_dir.path().join(&file_name);

        if file.is_dir() {
            std::fs::create_dir_all(&outpath).map_err(ContentError::Io)?;
        } else {
            if let Some(parent) = outpath.parent() {
                std::fs::create_dir_all(parent).map_err(ContentError::Io)?;
            }
            let mut outfile = std::fs::File::create(&outpath).map_err(ContentError::Io)?;
            std::io::copy(&mut file, &mut outfile).map_err(ContentError::Io)?;

            let ext = outpath.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext.eq_ignore_ascii_case("pkg") {
                pkg_path = Some(outpath.clone());
            } else if ext.eq_ignore_ascii_case("rap") {
                rap_path = Some(outpath.clone());
            }
        }
    }

    let pkg_path = pkg_path.ok_or(ContentError::NoPkgFound)?;
    Ok((pkg_path, rap_path, temp_dir))
}

/// Install a DLC ZIP to RPCS3: extract, install PKG, copy RAP, clean up.
pub async fn install_dlc_zip(rpcs3_exe: &Path, zip_path: &Path, exdata_dir: &Path) -> Result<(), ContentError> {
    info!("Installing DLC from ZIP: {}", zip_path.display());

    // Extract (sync, but fast — zips are small)
    let (pkg_path, rap_path, _temp_dir) = extract_dlc_zip(zip_path)?;

    // Install PKG via RPCS3
    crate::rpcs3::install_pkg(rpcs3_exe, &pkg_path)
        .await
        .map_err(|e| ContentError::InstallFailed(e.to_string()))?;

    // Copy RAP to exdata directory
    if let Some(rap) = rap_path {
        let rap_filename = rap.file_name().expect("RAP file should have a filename");
        let dest = exdata_dir.join(rap_filename);
        std::fs::copy(&rap, &dest).map_err(ContentError::Io)?;
        info!("Copied RAP license to {}", dest.display());
    }

    // _temp_dir drops here, automatically cleaning up extracted files
    info!("DLC installation complete");
    Ok(())
}
