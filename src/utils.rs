use std::fmt;
use std::path::{Path, PathBuf};

use log::{error, info};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tokio::fs;
use tokio::fs::{File, OpenOptions};

use tokio::io::{self, AsyncSeekExt, SeekFrom};

/// Controls how download directories are organized on disk.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum DirectoryFormat {
    /// `{serial}/` — e.g., `BLUS31147/`
    SerialOnly,
    /// `{serial} - {title}/` — e.g., `BLUS31147 - Batman Arkham Origins/`
    #[default]
    SerialTitle,
    /// `{title}/{serial}/` — e.g., `Batman - Arkham Origins/BLUS31147/`
    TitleSerial,
}

impl fmt::Display for DirectoryFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DirectoryFormat::SerialOnly => write!(f, "Serial only"),
            DirectoryFormat::SerialTitle => write!(f, "Serial - Title"),
            DirectoryFormat::TitleSerial => write!(f, "Title / Serial"),
        }
    }
}

impl DirectoryFormat {
    /// Returns a human-readable example of the directory structure.
    #[cfg(feature = "egui")]
    pub fn example(&self) -> &'static str {
        match self {
            DirectoryFormat::SerialOnly => "BLUS31147/",
            DirectoryFormat::SerialTitle => "BLUS31147 - Batman Arkham Origins/",
            DirectoryFormat::TitleSerial => "Batman - Arkham Origins/BLUS31147/",
        }
    }
}

/// Characters that are invalid in file/directory names on the current platform.
#[cfg(target_family = "windows")]
const INVALID_CHARS: [char; 7] = ['<', '>', '"', '/', '\\', '|', '*'];

#[cfg(target_family = "unix")]
const INVALID_CHARS: [char; 1] = ['/'];

/// Windows reserved device names that cannot be used as file/directory names.
const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3",
    "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Why a Files operation failed. PSN turns it into its own `DownloadError`, so the dependency
/// runs one way: PSN uses Files, Files does not use PSN.
#[derive(Debug)]
pub enum FileError {
    Io(io::Error),
    Security(crate::security::SecurityError),
    /// The file is shorter than the hash suffix it must carry.
    TooShort,
}

/// Maximum length for sanitized directory names (in characters).
/// Keeps well within the 255-byte filesystem limit while leaving room for serial prefixes.
const MAX_TITLE_LENGTH: usize = 100;

/// Sanitize a game title for use as a directory name.
///
/// Applies these rules in order:
/// 1. Strip null bytes
/// 2. Replace `: ` with ` - ` for readability (e.g., "Batman: Arkham Origins" → "Batman - Arkham Origins")
/// 3. Replace standalone `:` with ` -`
/// 4. Replace `?` with empty string (common in some titles)
/// 5. Replace remaining OS-invalid characters with `_`
/// 6. Replace `..` path traversal sequences
/// 7. Collapse multiple consecutive spaces into one
/// 8. Strip leading/trailing dots and spaces (Windows disallows these)
/// 9. Check against Windows reserved names and append `_` if matched
/// 10. Truncate to MAX_TITLE_LENGTH characters (UTF-8 safe)
fn sanitize_title(title: &str) -> String {
    // 1. Strip null bytes
    let mut result = title.replace('\0', "");

    // 2-3. Readable colon handling: `: ` → ` - `, standalone `:` → ` -`
    result = result.replace(": ", " - ");
    result = result.replace(':', " -");

    // 4. Strip question marks
    result = result.replace('?', "");

    // 5. Replace remaining OS-invalid chars with underscore
    result = result.replace(|c: char| INVALID_CHARS.contains(&c), "_");

    // 6. Replace path traversal sequences
    result = result.replace("..", "_");

    // 7. Collapse multiple consecutive spaces
    while result.contains("  ") {
        result = result.replace("  ", " ");
    }

    // 8. Strip leading/trailing dots and spaces
    let trimmed = result.trim_matches(|c: char| c == '.' || c == ' ');
    result = trimmed.to_string();

    // 9. Check Windows reserved names (case-insensitive)
    if WINDOWS_RESERVED.iter().any(|r| result.eq_ignore_ascii_case(r)) {
        result.push('_');
    }

    // 10. Truncate to MAX_TITLE_LENGTH (UTF-8 safe, don't split mid-char)
    if result.chars().count() > MAX_TITLE_LENGTH {
        result = result.chars().take(MAX_TITLE_LENGTH).collect();
        // Trim any trailing space from truncation
        result = result.trim_end().to_string();
    }

    result
}

/// Build the download directory path for a given game using the specified format.
pub fn create_pkg_path<P>(download_path: &P, serial: &str, title: &str, format: DirectoryFormat) -> PathBuf
where
    P: AsRef<Path>,
{
    let base = download_path.as_ref();
    let sanitized_title = sanitize_title(title);
    match format {
        DirectoryFormat::SerialOnly => base.join(serial),
        DirectoryFormat::SerialTitle => base.join(format!("{} - {}", serial, sanitized_title)),
        DirectoryFormat::TitleSerial => base.join(&sanitized_title).join(serial),
    }
}

/// Try to find and migrate an existing download directory from any historical format
/// to the currently selected format. Returns the target path (possibly after renaming).
/// Refuses a target outside the download folder, and never moves a folder from outside it.
async fn migrate_existing_dir(
    download_path: &Path,
    serial: &str,
    title: &str,
    format: DirectoryFormat,
) -> Result<PathBuf, FileError> {
    let target = create_pkg_path(&download_path, serial, title, format);

    crate::security::validate_path_containment(&target, download_path).map_err(FileError::Security)?;

    if target.exists() {
        return Ok(target);
    }

    let sanitized_title = sanitize_title(title);

    // Check all alternative formats and rename if found.
    let alternatives = [
        // SerialOnly format: {download_path}/{serial}/
        download_path.join(serial),
        // SerialTitle format: {download_path}/{serial} - {title}/
        download_path.join(format!("{} - {}", serial, sanitized_title)),
        // TitleSerial format: {download_path}/{title}/{serial}/
        download_path.join(&sanitized_title).join(serial),
    ];

    for alt in &alternatives {
        if alt == &target {
            continue; // Skip the target format itself
        }
        if alt.exists() && crate::security::validate_path_containment(alt, download_path).is_ok() {
            info!("Found download folder in old format at {:?}, migrating to {:?}", alt, target);
            // For TitleSerial format, the parent title directory may need to be created first.
            if let Some(parent) = target.parent() {
                if let Err(e) = fs::create_dir_all(parent).await {
                    error!("Failed to create parent directory for migration: {e}");
                    return Ok(alt.clone()); // Fall back to old path
                }
            }
            if let Err(e) = fs::rename(alt, &target).await {
                error!("Failed to migrate folder: {e}");
                return Ok(alt.clone()); // Fall back to old path
            }
            return Ok(target);
        }
    }

    Ok(target)
}

/// Open options for a file the app downloads or writes into the download folder.
///
/// On Unix the file is created with mode 0644, so a loose umask cannot leave it writable
/// by other users; an existing file keeps its mode. The open also refuses a link as the
/// final name (O_NOFOLLOW), so a link swapped in after the containment check cannot send
/// the write outside the download folder. A swapped folder higher up is not covered.
pub fn download_file_options() -> OpenOptions {
    #[allow(unused_mut)]
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    options.mode(0o644).custom_flags(libc::O_NOFOLLOW);
    options
}

pub async fn create_pkg_file(
    download_path: PathBuf,
    serial: &str,
    title: &str,
    pkg_name: &str,
    format: DirectoryFormat,
) -> Result<(File, PathBuf), FileError> {
    let mut target_path = match migrate_existing_dir(&download_path, serial, title, format).await {
        Ok(path) => path,
        Err(e) => {
            error!("Path containment check failed before migration: {e:?}");
            return Err(e);
        }
    };

    target_path.push(pkg_name);
    info!("Creating file for pkg at path {:?}", target_path);

    // Validate the target path stays within the download directory.
    if let Err(e) = crate::security::validate_path_containment(&target_path, &download_path) {
        error!("Path containment check failed: {e}");
        return Err(FileError::Security(e));
    }

    if let Some(parent) = target_path.parent() {
        match fs::create_dir_all(parent).await {
            Ok(_) => info!("Created directory for updates"),
            Err(e) => match e.kind() {
                io::ErrorKind::AlreadyExists => {}
                _ => return Err(FileError::Io(e)),
            },
        }
    } else {
        return Err(FileError::Io(io::Error::other("Target path has no parent directory")));
    }

    // Using OpenOptions to avoid the file getting truncated if it already exists
    // .create(true) preserves an existing file's contents.
    let file = download_file_options()
        .create(true)
        .read(true)
        .write(true)
        .open(&target_path)
        .await
        .map_err(FileError::Io)?;

    Ok((file, target_path))
}

/// Regex-like check for a serial pattern: 4 uppercase letters followed by 5 digits.
pub fn looks_like_serial(name: &str) -> bool {
    if name.len() != 9 {
        return false;
    }
    let bytes = name.as_bytes();
    bytes[..4].iter().all(|b| b.is_ascii_uppercase()) && bytes[4..].iter().all(|b| b.is_ascii_digit())
}

/// Scan a download directory and detect folders that could be migrated to the target format.
/// Returns a list of (current_path, proposed_new_path) pairs.
#[cfg(feature = "egui")]
pub fn detect_migratable_directories(download_path: &Path, target_format: DirectoryFormat) -> Vec<(PathBuf, PathBuf)> {
    let mut migrations = Vec::new();

    let entries = match std::fs::read_dir(download_path) {
        Ok(entries) => entries,
        Err(_) => return migrations,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Detect SerialOnly format: exactly a serial (e.g., "BLUS31147")
        if looks_like_serial(&name) {
            let new_path = create_pkg_path(&download_path, &name, &name, target_format);
            if new_path != path {
                migrations.push((path, new_path));
            }
            continue;
        }

        // Detect SerialTitle format: "SERIAL - Title" (e.g., "BLUS31147 - Batman Arkham Origins")
        if name.len() > 12 && looks_like_serial(&name[..9]) && name[9..].starts_with(" - ") {
            let serial = &name[..9];
            let title = &name[12..];
            let new_path = create_pkg_path(&download_path, serial, title, target_format);
            if new_path != path {
                migrations.push((path, new_path));
            }
            continue;
        }

        // Detect TitleSerial format: a title dir containing a serial subdir
        if target_format != DirectoryFormat::TitleSerial {
            if let Ok(sub_entries) = std::fs::read_dir(&path) {
                for sub in sub_entries.flatten() {
                    let sub_name = match sub.file_name().into_string() {
                        Ok(n) => n,
                        Err(_) => continue,
                    };
                    if sub.path().is_dir() && looks_like_serial(&sub_name) {
                        let new_path = create_pkg_path(&download_path, &sub_name, &name, target_format);
                        if new_path != sub.path() {
                            migrations.push((sub.path(), new_path));
                        }
                    }
                }
            }
        }
    }

    migrations
}

/// Execute a set of directory migrations (renames).
/// Returns the number of successful migrations.
#[cfg(feature = "egui")]
pub async fn migrate_directories(migrations: &[(PathBuf, PathBuf)], download_path: &Path) -> usize {
    let mut success_count = 0;
    for (old_path, new_path) in migrations {
        let contained = [old_path, new_path]
            .iter()
            .try_for_each(|path| crate::security::validate_path_containment(path, download_path));
        if let Err(e) = contained {
            error!("Skipping migration of {:?}: {e}", old_path);
            continue;
        }
        // Create parent directories for the new path if needed (e.g., TitleSerial format).
        if let Some(parent) = new_path.parent() {
            if let Err(e) = fs::create_dir_all(parent).await {
                error!("Failed to create directory {:?}: {e}", parent);
                continue;
            }
        }
        match fs::rename(old_path, new_path).await {
            Ok(()) => {
                info!("Migrated {:?} -> {:?}", old_path, new_path);
                success_count += 1;
            }
            Err(e) => {
                error!("Failed to migrate {:?} -> {:?}: {e}", old_path, new_path);
            }
        }
    }
    success_count
}

const CHUNK_SIZE: usize = 1024 * 1024 * 128;

/// Verify a downloaded file's SHA1 hash. Runs the CPU-intensive hashing on the
/// tokio blocking thread pool so it doesn't starve async I/O tasks.
pub async fn hash_file(file: &mut File, hash: &str, hash_whole_file: bool) -> Result<bool, FileError> {
    // Last 0x20 bytes are the SHA1 hash for PS3 updates. PS4 updates don't include hash suffix.
    let suffix_size: u64 = if hash_whole_file { 0 } else { 0x20 };

    // If the file size is below the length of the embedded sha1-hash suffix,
    // don't bother hashing the contents. Download's borked.
    let file_length = file.metadata().await.map_err(FileError::Io)?.len();
    if file_length <= suffix_size {
        return Ok(false);
    }

    // Reset seek pointer so we read from the start.
    file.seek(SeekFrom::Start(0)).await.map_err(FileError::Io)?;

    // Clone the handle and convert to a std::fs::File for blocking I/O.
    let std_file = file.try_clone().await.map_err(FileError::Io)?.into_std().await;
    let hash_owned = hash.to_string();

    tokio::task::spawn_blocking(move || hash_file_blocking(std_file, &hash_owned, file_length, suffix_size))
        .await
        .map_err(|e| FileError::Io(std::io::Error::other(format!("hash task panicked: {e}"))))?
}

/// Synchronous SHA1 hashing, designed to run inside `spawn_blocking`.
fn hash_file_blocking(file: std::fs::File, hash: &str, file_length: u64, suffix_size: u64) -> Result<bool, FileError> {
    use std::io::{BufRead, BufReader};

    let mut hasher = Sha1::new();

    let file_length_without_suffix: usize = (file_length - suffix_size).try_into().map_err(|_| FileError::TooShort)?;

    let mut reader = BufReader::with_capacity(CHUNK_SIZE, file);
    let mut processed_length = 0;
    loop {
        let chunk_buffer = reader.fill_buf().map_err(FileError::Io)?;
        let chunk_length = chunk_buffer.len();
        if chunk_length == 0 {
            break;
        }

        let previously_processed_length: usize = processed_length;
        processed_length += chunk_length;
        // While iterating through the file a chunk being processed may already include some hash suffix bits which should not be hashed.
        // In such case file chunk is stripped of those extra suffix bits.
        let suffix_part_in_chunk = processed_length > file_length_without_suffix;
        let hashable_buffer = if suffix_part_in_chunk {
            let last_before_suffix = file_length_without_suffix
                .checked_sub(previously_processed_length)
                .ok_or(FileError::TooShort)?;
            &chunk_buffer[..last_before_suffix]
        } else {
            chunk_buffer
        };

        hasher.update(hashable_buffer);
        reader.consume(chunk_length);
        if suffix_part_in_chunk {
            break;
        }
    }

    // Sony gives the checksum in either case (PSVita replies use upper case).
    Ok(hex::encode(hasher.finalize()).eq_ignore_ascii_case(hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hash_file_accepts_upper_case_checksum() {
        // Sony's PSVita update XML gives sha1sum in upper case (PCSA00007 01.01, 2026-10-12).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.pkg");
        let body = b"package body";
        let mut content = body.to_vec();
        content.extend_from_slice(&[0u8; 0x20]);
        std::fs::write(&path, &content).unwrap();
        let expected = hex::encode(Sha1::digest(body)).to_uppercase();

        let mut file = File::open(&path).await.unwrap();
        assert!(hash_file(&mut file, &expected, false).await.unwrap());
    }

    #[tokio::test]
    async fn create_pkg_file_does_not_migrate_outside_download_folder() {
        // A serial carrying "../" makes both the old-format folder and the migration target
        // sit outside the download folder. Nothing may be moved there.
        let root = tempfile::tempdir().unwrap();
        let download_path = root.path().join("downloads");
        std::fs::create_dir(&download_path).unwrap();
        let outside_old = root.path().join("escape");
        std::fs::create_dir(&outside_old).unwrap();

        let result = create_pkg_file(download_path, "../escape", "Title", "x.pkg", DirectoryFormat::SerialTitle).await;

        assert!(matches!(result, Err(FileError::Security(_))));
        assert!(outside_old.exists(), "the folder outside the download folder was moved");
        assert!(!root.path().join("escape - Title").exists());
    }

    #[tokio::test]
    async fn create_pkg_file_does_not_migrate_from_outside_download_folder() {
        // With Title / Serial and an existing Title folder, "../escape" resolves back inside the
        // download folder, but the old Serial-only folder it would be moved from sits outside.
        // It must stay put.
        let root = tempfile::tempdir().unwrap();
        let download_path = root.path().join("downloads");
        std::fs::create_dir_all(download_path.join("Title")).unwrap();
        let outside_old = root.path().join("escape");
        std::fs::create_dir(&outside_old).unwrap();

        let result = create_pkg_file(download_path, "../escape", "Title", "x.pkg", DirectoryFormat::TitleSerial).await;

        assert!(result.is_ok());
        assert!(outside_old.exists(), "a folder from outside the download folder was moved in");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn download_file_options_refuse_a_link() {
        // A link swapped in after the containment check must not be followed (RPSN-0040).
        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("outside.txt");
        std::fs::write(&outside, b"untouched").unwrap();
        let link = root.path().join("x.pkg");
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        let result = download_file_options().create(true).write(true).open(&link).await;

        assert!(result.is_err(), "a link was opened for writing");
        assert_eq!(std::fs::read(&outside).unwrap(), b"untouched");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn create_pkg_file_is_not_writable_by_group_or_others() {
        use std::os::unix::fs::PermissionsExt;
        // A loose umask must not leave a download writable by other users (RPSN-0037).
        // SAFETY: umask only swaps the process file-mode mask; restored below.
        let previous = unsafe { libc::umask(0o002) };
        let root = tempfile::tempdir().unwrap();
        let result = create_pkg_file(
            root.path().to_path_buf(),
            "BLUS31147",
            "Title",
            "x.pkg",
            DirectoryFormat::SerialTitle,
        )
        .await;
        unsafe { libc::umask(previous) };

        let (_file, path) = result.unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o022, 0, "download created with mode {mode:o}");
    }

    #[cfg(feature = "egui")]
    #[tokio::test]
    async fn migrate_directories_skips_moves_outside_download_folder() {
        let root = tempfile::tempdir().unwrap();
        let download_path = root.path().join("downloads");
        let inside_old = download_path.join("BLUS31147");
        std::fs::create_dir_all(&inside_old).unwrap();
        let inside_new = download_path.join("Title").join("BLUS31147");
        let outside_new = root.path().join("escaped");
        let second_old = download_path.join("BCES00001");
        std::fs::create_dir_all(&second_old).unwrap();

        let migrations = vec![
            (inside_old.clone(), inside_new.clone()),
            (second_old.clone(), outside_new.clone()),
        ];
        let count = migrate_directories(&migrations, &download_path).await;

        assert_eq!(count, 1);
        assert!(inside_new.exists());
        assert!(second_old.exists(), "a folder was moved out of the download folder");
        assert!(!outside_new.exists());
    }

    #[test]
    fn sanitize_colon_to_dash() {
        assert_eq!(sanitize_title("Batman: Arkham Origins"), "Batman - Arkham Origins");
        assert_eq!(
            sanitize_title("Assassin's Creed: Brotherhood"),
            "Assassin's Creed - Brotherhood"
        );
    }

    #[test]
    fn sanitize_standalone_colon() {
        assert_eq!(sanitize_title("Title:Subtitle"), "Title -Subtitle");
    }

    #[test]
    fn sanitize_question_mark() {
        assert_eq!(sanitize_title("Catherine: Full Body?"), "Catherine - Full Body");
    }

    #[test]
    fn sanitize_path_traversal() {
        assert_eq!(sanitize_title("../../etc/passwd"), "____etc_passwd");
        assert_eq!(sanitize_title("game..update"), "game_update");
    }

    #[test]
    fn sanitize_null_bytes() {
        assert_eq!(sanitize_title("Game\0Title"), "GameTitle");
    }

    #[test]
    fn sanitize_leading_trailing_dots_spaces() {
        assert_eq!(sanitize_title("  .Game Title.  "), "Game Title");
        assert_eq!(sanitize_title("...hidden"), "_.hidden");
    }

    #[test]
    fn sanitize_windows_reserved_names() {
        assert_eq!(sanitize_title("CON"), "CON_");
        assert_eq!(sanitize_title("nul"), "nul_");
        assert_eq!(sanitize_title("LPT1"), "LPT1_");
    }

    #[test]
    fn sanitize_length_limit() {
        let long_title = "A".repeat(200);
        let sanitized = sanitize_title(&long_title);
        assert_eq!(sanitized.chars().count(), MAX_TITLE_LENGTH);
    }

    #[test]
    fn sanitize_preserves_unicode() {
        assert_eq!(sanitize_title("龍が如く0"), "龍が如く0");
        assert_eq!(sanitize_title("Hustle Kings(TM)"), "Hustle Kings(TM)");
    }

    #[test]
    fn sanitize_newline_in_title() {
        // BCUS98233 is known to have a newline in its title.
        // The newline replacement happens in psn/mod.rs, but sanitize_title
        // should still handle it gracefully if one slips through.
        assert_eq!(sanitize_title("Game\nTitle"), "Game\nTitle");
    }

    #[test]
    fn sanitize_multiple_spaces_collapsed() {
        assert_eq!(sanitize_title("Game     Title"), "Game Title");
    }

    #[test]
    fn sanitize_normal_titles_unchanged() {
        assert_eq!(sanitize_title("God of War III"), "God of War III");
        assert_eq!(sanitize_title("LittleBigPlanet"), "LittleBigPlanet");
        assert_eq!(sanitize_title("inFAMOUS 2"), "inFAMOUS 2");
    }
}
