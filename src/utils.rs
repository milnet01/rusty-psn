use std::path::{Path, PathBuf};

use sha1_smol::Sha1;

use tokio::fs;
use tokio::fs::{File, OpenOptions};

use tokio::io::{self, AsyncBufReadExt, AsyncSeekExt, BufReader, SeekFrom};

use crate::psn::DownloadError;

fn sanitize_title(title: &str) -> String {
    let options = if cfg!(target_family = "windows") {
        sanitise_file_name::Options::default()
    } else {
        // No need to make filenames boring in other platforms just because
        // Windows can't have some fun.
        sanitise_file_name::Options {
            windows_safe: false,
            ..sanitise_file_name::Options::default()
        }
    };

    sanitise_file_name::sanitise_with_options(title, &options)
}

fn create_old_pkg_path<P>(download_path: P, serial: &str) -> PathBuf
where
    P: AsRef<Path>,
{
    download_path.as_ref().join(serial)
}

pub fn create_new_pkg_path<P>(download_path: &P, serial: &str, title: &str) -> PathBuf
where
    P: AsRef<Path>,
{
    let target_path = download_path.as_ref();
    let sanitized_title = sanitize_title(title);
    target_path.join(format!("{} - {}", serial, sanitized_title))
}

pub async fn create_pkg_file(download_path: PathBuf, serial: &str, title: &str, pkg_name: &str) -> Result<File, DownloadError> {
    let mut target_path = create_new_pkg_path(&download_path, serial, title);

    // Check for the old path format.
    let old_path = create_old_pkg_path(&download_path, serial);
    if old_path.exists() {
        info!("Found a folder with the old name format, trying to rename to current one.");

        if let Err(e) = fs::rename(&old_path, &target_path).await {
            error!("Failed to rename folder: {e}");
        }
    }

    target_path.push(pkg_name);
    info!("Creating file for pkg at path {:?}", target_path);

    let parent = target_path
        .parent()
        .ok_or_else(|| DownloadError::Tokio(io::Error::other("Target path has no parent directory")))?;
    // create_dir_all succeeds if the directory already exists.
    fs::create_dir_all(parent).await.map_err(DownloadError::Tokio)?;

    // Using OpenOptions to avoid the file getting truncated if it already exists
    // .create(true) preserves an existing file's contents.
    OpenOptions::default()
        .create(true)
        .read(true)
        .write(true)
        .open(target_path)
        .await
        .map_err(DownloadError::Tokio)
}

// Large enough to keep syscall overhead low, small enough that several concurrent
// downloads being verified at once don't allocate hundreds of megabytes.
const HASH_BUFFER_SIZE: usize = 1024 * 1024;

pub async fn hash_file(file: &mut File, hash: &str, hash_whole_file: bool) -> Result<bool, DownloadError> {
    // Last 0x20 bytes are the SHA1 hash for PS3 and PS Vita updates. PS4 updates don't include hash suffix.
    let suffix_size = if hash_whole_file { 0 } else { 0x20 };

    // If the file size is below the length of the embedded sha1-hash suffix,
    // don't bother hashing the contents. Download's borked.
    let file_length = file.metadata().await.map_err(DownloadError::Tokio)?.len();
    if file_length <= suffix_size {
        return Ok(false);
    }

    // Write operations during the download move the internal seek pointer.
    // Resetting it to 0 makes reader actually read the whole thing.
    file.seek(SeekFrom::Start(0)).await.map_err(DownloadError::Tokio)?;

    let mut hasher = Sha1::new();
    let mut reader = BufReader::with_capacity(HASH_BUFFER_SIZE, file);
    let mut remaining = file_length - suffix_size;

    while remaining > 0 {
        let chunk = reader.fill_buf().await.map_err(DownloadError::Tokio)?;
        if chunk.is_empty() {
            // The file got shorter than its reported length.
            return Ok(false);
        }

        // Never hash into the suffix, which may start partway through a chunk.
        let hashable_length = remaining.min(chunk.len() as u64) as usize;
        hasher.update(&chunk[..hashable_length]);
        reader.consume(hashable_length);
        remaining -= hashable_length as u64;
    }

    Ok(hasher.digest().to_string().eq_ignore_ascii_case(hash))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    async fn temp_file(name: &str, contents: &[u8]) -> File {
        let path = std::env::temp_dir().join(format!("rusty-psn-{}-{name}", std::process::id()));
        let mut file = OpenOptions::default()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(path)
            .await
            .unwrap();
        file.write_all(contents).await.unwrap();
        file.flush().await.unwrap();
        file
    }

    #[tokio::test]
    async fn hash_file_skips_suffix() {
        let mut contents = b"hello world".to_vec();
        contents.extend_from_slice(&[0xAA; 0x20]);
        let expected = Sha1::from(b"hello world").digest().to_string();

        let mut file = temp_file("suffix", &contents).await;
        assert!(hash_file(&mut file, &expected.to_uppercase(), false).await.unwrap());
        assert!(!hash_file(&mut file, &expected, true).await.unwrap());
    }

    #[tokio::test]
    async fn hash_file_rejects_short_files() {
        let mut file = temp_file("short", &[0; 0x20]).await;
        assert!(!hash_file(&mut file, "", false).await.unwrap());
    }
}
