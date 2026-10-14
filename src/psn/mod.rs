use log::{error, info, warn};

pub mod firmware;
mod manifest_parser;
mod parser;
pub mod utils;

use std::path::PathBuf;
#[cfg(feature = "egui")]
use std::str::FromStr;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

#[cfg(feature = "egui")]
use reqwest::Url;
use tokio::io::{AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tokio::sync::mpsc::Sender;
#[cfg(feature = "egui")]
use utils::copy_pkg_file;
use utils::{get_platform_variant, get_update_info_url, read_capped_text, CappedReadError, PlatformVariant};

use crate::utils::DirectoryFormat;

/// Build a reqwest client configured for Sony PSN servers.
///
/// Sony's servers present certificates that fail normal validation, so the TLS config skips
/// certificate verification for Sony hosts only (`security::sony_tls_config`); any other host
/// is verified normally. The redirect policy validates that each hop stays within allowed Sony domains.
///
/// This client should be constructed once and reused — it holds a connection pool and
/// TLS session cache internally, and `Clone` is just an `Arc::clone`.
pub fn build_psn_client() -> Result<reqwest::Client, PsnClientError> {
    let tls = crate::security::sony_tls_config().map_err(PsnClientError::Tls)?;
    reqwest::ClientBuilder::default()
        .tls_backend_preconfigured(tls)
        .redirect(crate::security::sony_redirect_policy())
        .build()
        .map_err(PsnClientError::Reqwest)
}

#[derive(Debug)]
pub enum PsnClientError {
    Tls(rustls::Error),
    Reqwest(reqwest::Error),
}

impl std::fmt::Display for PsnClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PsnClientError::Tls(e) => write!(f, "TLS setup failed: {e}"),
            PsnClientError::Reqwest(e) => write!(f, "{e}"),
        }
    }
}

#[derive(Debug)]
pub enum DownloadStatus {
    Progress(u64),
    /// The download is resuming from the given byte offset (partial file existed on disk).
    Resuming(u64),

    Verifying,
    DownloadSuccess,
    DownloadFailure,
}

#[cfg(feature = "egui")]
#[derive(Debug)]
pub enum MergeStatus {
    PartProgress(usize),

    MergeSuccess,
}

#[cfg(feature = "egui")]
#[derive(Debug)]
pub enum MergeError {
    FilepathMismatch(String),
    FileMergeFailure,
    PackagesUnmergeable(String),
    Security(crate::security::SecurityError),
}

/// Cancel signal values for download pause/abort.
pub const CANCEL_NONE: u8 = 0;
#[cfg(feature = "egui")]
pub const CANCEL_PAUSE: u8 = 1;
pub const CANCEL_ABORT: u8 = 2;

#[derive(Debug)]
pub enum DownloadError {
    /// The checksum failed. `short_on_data` says less arrived than expected, which Sony's servers
    /// cause by dropping out early. `file` is the partial file kept on disk so a retry can resume it.
    HashMismatch {
        short_on_data: bool,
        file: Option<PathBuf>,
    },
    /// A file with no hash to check (firmware) ended before its stated size.
    Incomplete {
        received: u64,
        expected: u64,
    },
    Tokio(tokio::io::Error),
    Reqwest(reqwest::Error),
    Security(crate::security::SecurityError),
    /// Download was cancelled by user. Contains the file path if one was created,
    /// and whether the user requested abort (true) vs pause (false).
    #[allow(dead_code)]
    Cancelled {
        file_path: Option<PathBuf>,
        abort: bool,
    },
}

impl From<crate::utils::FileError> for DownloadError {
    fn from(e: crate::utils::FileError) -> Self {
        match e {
            crate::utils::FileError::Io(e) => DownloadError::Tokio(e),
            crate::utils::FileError::Security(e) => DownloadError::Security(e),
            crate::utils::FileError::TooShort => DownloadError::HashMismatch {
                short_on_data: true,
                file: None,
            },
        }
    }
}

impl DownloadError {
    /// Deletes the partial file a failed download kept for resuming (S1). Call it once no retry will follow.
    pub fn discard_kept_file(&self) {
        if let DownloadError::HashMismatch { file: Some(path), .. } = self {
            match std::fs::remove_file(path) {
                Ok(()) => info!("Removed {} after its last failed attempt", path.display()),
                Err(e) => warn!("Could not remove {}: {e}", path.display()),
            }
        }
    }
}

/// Ends a download whose checksum failed. A file that arrived whole can never verify, so it is deleted now.
/// A short one is kept so a retry can resume it (S2), and the caller discards it once it stops retrying.
async fn checksum_failed(path: PathBuf, short_on_data: bool) -> DownloadError {
    if short_on_data {
        return DownloadError::HashMismatch {
            short_on_data,
            file: Some(path),
        };
    }
    match tokio::fs::remove_file(&path).await {
        Ok(()) => info!("Removed {}, which arrived whole but failed its checksum", path.display()),
        Err(e) => warn!("Could not remove {}: {e}", path.display()),
    }
    DownloadError::HashMismatch {
        short_on_data,
        file: None,
    }
}

#[derive(Debug)]
pub enum UpdateError {
    InvalidSerial,
    NoUpdatesAvailable,
    UnhandledErrorResponse(String),
    Reqwest(reqwest::Error),
    XmlParsing(quick_xml::Error),
    ManifestParsing(serde_json::Error),
    Security(crate::security::SecurityError),
    ResponseTooLarge(u64),
}

impl From<CappedReadError> for UpdateError {
    fn from(err: CappedReadError) -> Self {
        match err {
            CappedReadError::Reqwest(e) => UpdateError::Reqwest(e),
            CappedReadError::TooLarge(size) => UpdateError::ResponseTooLarge(size),
        }
    }
}

#[derive(Clone)]
pub struct UpdateInfo {
    pub title_id: String,
    pub tag_name: String,

    pub titles: Vec<String>,
    pub packages: Vec<PackageInfo>,
    pub platform_variant: PlatformVariant,
}

impl UpdateInfo {
    fn empty(platform_variant: PlatformVariant) -> UpdateInfo {
        UpdateInfo {
            title_id: String::new(),
            tag_name: String::new(),

            titles: Vec::new(),
            packages: Vec::new(),
            platform_variant,
        }
    }

    #[cfg(feature = "egui")]
    pub fn title(&self) -> String {
        if let Some(title) = self.titles.first() {
            title.clone()
        } else {
            String::new()
        }
    }

    pub async fn get_info(title_id: String, client: &reqwest::Client) -> Result<UpdateInfo, UpdateError> {
        let title_id = parse_title_id(&title_id);

        // Validate serial format before making any network requests.
        if let Err(e) = crate::security::validate_serial(&title_id) {
            warn!("Serial validation failed: {e}");
            return Err(UpdateError::InvalidSerial);
        }

        let platform_variant = match get_platform_variant(&title_id) {
            Some(variant) => variant,
            None => return Err(UpdateError::InvalidSerial),
        };
        let url = match get_update_info_url(&title_id, platform_variant) {
            Ok(url) => url,
            Err(err) => return Err(err),
        };

        info!("Querying for updates for serial: {}", title_id);

        let response = client.get(url).send().await.map_err(UpdateError::Reqwest)?;
        let response_txt = read_capped_text(response, crate::security::MAX_METADATA_SIZE).await?;

        if response_txt.is_empty() {
            return Err(UpdateError::NoUpdatesAvailable);
        }

        if response_txt.contains("Not found") {
            return Err(UpdateError::InvalidSerial);
        }

        let mut info = UpdateInfo::empty(platform_variant);
        match parser::parse_response(response_txt, &mut info) {
            Ok(()) => {
                if info.title_id.is_empty() || info.packages.is_empty() {
                    return Err(UpdateError::NoUpdatesAvailable);
                }

                // This abomination comes courtesy of BCUS98233.
                // For some ungodly reason, the title has a newline (/n), which of course causes issues
                // both when displaying the title and when trying to create a folder to put the files in.
                let titles = &info.titles;
                info.titles = titles.iter().map(|title| title.replace("\n", " ")).collect();
            }
            Err(e) => match e {
                parser::ParseError::ErrorCode(reason) => {
                    if reason == "NoSuchKey" {
                        return Err(UpdateError::InvalidSerial);
                    }

                    return Err(UpdateError::UnhandledErrorResponse(reason));
                }
                parser::ParseError::XmlParsing(reason) => return Err(UpdateError::XmlParsing(reason)),
                parser::ParseError::InvalidTitleId(id) => {
                    warn!("Sony's reply named an invalid title id: {id:?}");
                    return Err(UpdateError::InvalidSerial);
                }
            },
        }

        if platform_variant != PlatformVariant::PS4 {
            return Ok(info);
        }

        let mut parent_manifest_packages = info.packages;
        info.packages = Vec::new(); // previously fetched manifest packages are moved out of packages list and a new list of part packages will be filled-in instead

        for package in parent_manifest_packages.drain(..) {
            // Validate manifest URL before fetching (it comes from Sony's XML response).
            crate::security::validate_download_url(&package.manifest_url).map_err(UpdateError::Security)?;

            let manifest_response = client.get(&package.manifest_url).send().await.map_err(UpdateError::Reqwest)?;
            let manifest_response_txt = read_capped_text(manifest_response, crate::security::MAX_METADATA_SIZE).await?;
            match manifest_parser::parse_manifest_response(manifest_response_txt, &package, &mut info) {
                Ok(()) => {}
                Err(e) => {
                    match e {
                        manifest_parser::ParseError::NoPartsFound => return Err(UpdateError::NoUpdatesAvailable),
                        manifest_parser::ParseError::JsonParsing(reason) => return Err(UpdateError::ManifestParsing(reason)),
                    };
                }
            }
        }

        Ok(info)
    }

    #[cfg(feature = "egui")]
    pub async fn merge_parts(
        &self,
        tx: Sender<MergeStatus>,
        download_path: &std::path::Path,
        format: DirectoryFormat,
    ) -> Result<(), MergeError> {
        if !self.packages.iter().all(|pkg| pkg.part_number.is_some()) {
            return Err(MergeError::PackagesUnmergeable(String::from(
                "some packages for the update are not a partial package",
            )));
        }

        let mut packages_sorted_by_part_number = self.packages.clone();
        // Safe: the all() check above guarantees every package has a part_number.
        packages_sorted_by_part_number.sort_by_key(|pkg| pkg.part_number.unwrap());
        // The same lookup the download used, so a folder a failed migration left in place is found.
        let package_download_path = crate::utils::migrate_existing_dir(download_path, &self.title_id, &self.title(), format)
            .await
            .map_err(|e| match e {
                crate::utils::FileError::Security(e) => MergeError::Security(e),
                other => {
                    error!("could not find the download folder: {other:?}");
                    MergeError::FileMergeFailure
                }
            })?;

        info!("Starting merge for {}", self.title());

        for package in self.packages.iter() {
            let file_name = match package.file_name() {
                Some(name) => name,
                None => {
                    return Err(MergeError::FilepathMismatch(String::from(
                        "could not deduce filename from a package url",
                    )))
                }
            };

            // Safe: the all() check at the top of merge_parts() guarantees part_number is Some.
            let part_number = package.part_number.unwrap();
            let expected_end_of_file_name = format!("_{}.pkg", part_number - 1);
            if !file_name.ends_with(&expected_end_of_file_name) {
                return Err(MergeError::FilepathMismatch(String::from(
                    "package name does not end with expected index and extension",
                )));
            }

            let merged_file_name = file_name.replace(&expected_end_of_file_name, ".pkg");
            let mut merged_path = package_download_path.clone();
            merged_path.push(&merged_file_name);
            let mut package_path = package_download_path.clone();
            package_path.push(&file_name);
            for path in [&merged_path, &package_path] {
                crate::security::validate_path_containment(path, download_path).map_err(MergeError::Security)?;
            }
            match copy_pkg_file(&package_path, &merged_path, package.offset).await {
                Ok(read_length) => {
                    let _ = tx.send(MergeStatus::PartProgress(part_number)).await;
                    info!("merged {} bytes from {} to {}", read_length, file_name, merged_file_name);
                }
                Err(err) => {
                    error!("could not merge files: {err}");
                    return Err(MergeError::FileMergeFailure);
                }
            };
        }

        let _ = tx.send(MergeStatus::MergeSuccess).await;
        Ok(())
    }
}

pub fn parse_title_id(title_id: &str) -> String {
    title_id
        .trim()
        .replace("-", "") // strip the dash that some sites put in a title id, eg. BCES-xxxxx
        .to_uppercase()
}

#[derive(Clone)]
pub struct PackageInfo {
    pub url: String,
    pub size: u64,
    pub version: String,
    pub sha1sum: String,
    pub hash_whole_file: bool,
    pub manifest_url: String,
    #[cfg_attr(not(feature = "egui"), allow(dead_code))]
    pub offset: u64,
    pub part_number: Option<usize>,
}

impl PackageInfo {
    fn empty() -> PackageInfo {
        PackageInfo {
            url: String::new(),
            size: 0,
            version: String::new(),
            sha1sum: String::new(),
            hash_whole_file: false,
            manifest_url: String::new(),
            offset: 0,
            part_number: None,
        }
    }

    pub fn id(&self) -> String {
        match self.part_number {
            Some(part_idx) => format!("{0} - Part {1}", self.version, part_idx),
            None => self.version.to_owned(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn start_download(
        &self,
        tx: Sender<DownloadStatus>,
        download_path: PathBuf,
        serial: String,
        title: String,
        format: DirectoryFormat,
        cancel_signal: Arc<AtomicU8>,
        client: &reqwest::Client,
    ) -> Result<PathBuf, DownloadError> {
        info!("Starting download for {serial} {}", self.version);
        info!("Sending pkg file request to url: {}", self.url);

        // Validate download URL before making the request (URL comes from Sony's XML response).
        crate::security::validate_download_url(&self.url).map_err(DownloadError::Security)?;

        // HEAD request to discover the final URL (after redirects), filename, and ETag.
        // This replaces the previous approach of sending a GET just to extract the filename.
        // The HEAD is cheap and also provides the ETag needed for safe resume via If-Range.
        let head_response = client.head(&self.url).send().await.map_err(DownloadError::Reqwest)?;
        let etag = head_response.headers().get(reqwest::header::ETAG).cloned();

        let file_name = head_response
            .url()
            .path_segments()
            .and_then(|s| s.into_iter().next_back())
            .and_then(|n| if n.is_empty() { None } else { Some(n.to_string()) })
            .unwrap_or_else(|| String::from("update.pkg"));

        info!("HEAD response received, file name is {file_name}");

        let (mut pkg_file, pkg_file_path) =
            crate::utils::create_pkg_file(download_path, &serial, &title, &file_name, format).await?;
        let existing_size = pkg_file.metadata().await.map_err(DownloadError::Tokio)?.len();

        // Check if the file is already complete by verifying its hash.
        let _ = tx.send(DownloadStatus::Verifying).await;

        if crate::utils::hash_file(&mut pkg_file, &self.sha1sum, self.hash_whole_file).await? {
            info!(
                "File for {serial} {} already existed and was complete, wrapping up...",
                self.version
            );
            let _ = tx.send(DownloadStatus::DownloadSuccess).await;
            return Ok(pkg_file_path);
        }

        // File is not complete. Determine whether we can resume from a partial download.
        // Resume if the file has content but is smaller than expected.
        let resume_offset = if existing_size > 0 && existing_size < self.size {
            existing_size
        } else {
            0
        };

        // Build GET request with optional Range/If-Range headers for resume.
        let mut request = client.get(&self.url);
        if resume_offset > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={}-", resume_offset));
            // If-Range with ETag ensures that if the server-side file changed since we started,
            // the server returns 200 (full file) instead of 206, preventing corrupt reassembly.
            if let Some(etag_value) = &etag {
                request = request.header(reqwest::header::IF_RANGE, etag_value.clone());
            }
            info!("Attempting resume from byte {resume_offset} for {serial} {}", self.version);
        }

        // An error status (4xx/5xx) must not reach the file: the partial download stays for a later resume.
        let mut response = request
            .send()
            .await
            .map_err(DownloadError::Reqwest)?
            .error_for_status()
            .map_err(DownloadError::Reqwest)?;

        // A 206 that starts anywhere but the resume point would be appended at the wrong place,
        // so the file is fetched whole instead.
        if resume_offset > 0
            && response.status() == reqwest::StatusCode::PARTIAL_CONTENT
            && content_range_start(response.headers()) != Some(resume_offset)
        {
            warn!(
                "Server resumed {serial} {} at {:?} instead of byte {resume_offset}; starting fresh",
                self.version,
                response.headers().get(reqwest::header::CONTENT_RANGE)
            );
            response = client
                .get(&self.url)
                .send()
                .await
                .map_err(DownloadError::Reqwest)?
                .error_for_status()
                .map_err(DownloadError::Reqwest)?;
        }

        // Determine actual write offset based on server response.
        let actual_offset = if resume_offset > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
            // 206: server honored the Range request. Seek to end and append.
            info!("Server returned 206 Partial Content, resuming from byte {resume_offset}");
            let _ = tx.send(DownloadStatus::Resuming(resume_offset)).await;
            pkg_file
                .seek(SeekFrom::Start(resume_offset))
                .await
                .map_err(DownloadError::Tokio)?;
            resume_offset
        } else {
            // 200: server didn't honor Range (or file changed per ETag).
            // Truncate and start fresh.
            if resume_offset > 0 {
                info!(
                    "Server returned {} instead of 206, starting fresh download",
                    response.status()
                );
            }
            pkg_file.set_len(0).await.map_err(DownloadError::Tokio)?;
            pkg_file.seek(SeekFrom::Start(0)).await.map_err(DownloadError::Tokio)?;
            0
        };

        let mut received_data = actual_offset;

        // If resuming, report pre-existing bytes as progress so the UI progress bar
        // starts at the correct position.
        if actual_offset > 0 {
            let _ = tx.send(DownloadStatus::Progress(actual_offset)).await;
        }

        while let Some(download_chunk) = response.chunk().await.map_err(DownloadError::Reqwest)? {
            // Check for cancellation before writing the chunk.
            let signal = cancel_signal.load(Ordering::Relaxed);
            if signal != CANCEL_NONE {
                let abort = signal == CANCEL_ABORT;
                info!(
                    "Download {} for {serial} {}",
                    if abort { "aborted" } else { "paused" },
                    self.version
                );
                let _ = pkg_file.sync_all().await;
                return Err(DownloadError::Cancelled {
                    file_path: Some(pkg_file_path),
                    abort,
                });
            }

            let download_chunk = download_chunk.as_ref();
            let download_chunk_len = download_chunk.len() as u64;

            received_data += download_chunk_len;

            let _ = tx.send(DownloadStatus::Progress(download_chunk_len)).await;

            if let Err(e) = pkg_file.write_all(download_chunk).await {
                error!("Failed to write chunk data: {e}");
                return Err(DownloadError::Tokio(e));
            }
        }

        if let Err(e) = pkg_file.sync_all().await {
            error!("Failed to flush all data to file: {e}");
            return Err(DownloadError::Tokio(e));
        }

        if received_data < self.size {
            warn!(
                "Received less data than expected for pkg file! Expected {} bytes, received {} bytes.",
                self.size, received_data
            )
        }

        info!(
            "No more chunks available, hashing received file for {serial} {}",
            self.version
        );

        let _ = tx.send(DownloadStatus::Verifying).await;

        // A file shorter than the hash suffix it must carry is short on data, and is kept for resume like one.
        let (verified, too_short) = match crate::utils::hash_file(&mut pkg_file, &self.sha1sum, self.hash_whole_file).await {
            Ok(verified) => (verified, false),
            Err(crate::utils::FileError::TooShort) => (false, true),
            Err(e) => return Err(e.into()),
        };

        if verified {
            info!("Hash for {serial} {} matched, wrapping up...", self.version);
            let _ = tx.send(DownloadStatus::DownloadSuccess).await;
            Ok(pkg_file_path)
        } else {
            error!("Hash mismatch for {serial} {}!", self.version);
            let _ = tx.send(DownloadStatus::DownloadFailure).await;
            drop(pkg_file); // Windows cannot delete a file that is still open.
            Err(checksum_failed(pkg_file_path, too_short || received_data < self.size).await)
        }
    }

    #[cfg(feature = "egui")]
    pub fn file_name(&self) -> Option<String> {
        let pkg_url = match Url::from_str(&self.url) {
            Ok(url) => url,
            Err(_) => return None,
        };

        let file_name = pkg_url.path_segments().and_then(|s| s.into_iter().next_back()).and_then(|n| {
            if n.is_empty() {
                None
            } else {
                Some(n.to_string())
            }
        });

        file_name
    }
}

/// The first byte a 206 reply's `Content-Range: bytes START-END/TOTAL` says it carries.
fn content_range_start(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    let value = headers.get(reqwest::header::CONTENT_RANGE)?.to_str().ok()?;
    let (unit, range) = value.trim().split_once(' ')?;
    if !unit.eq_ignore_ascii_case("bytes") {
        return None;
    }
    range.split_once('-')?.0.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{checksum_failed, content_range_start, DownloadError};

    #[test]
    fn content_range_start_reads_the_first_byte() {
        // A 206 at the wrong offset used to be appended as if it resumed (RPSN-0118).
        let range = |value: &str| {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(reqwest::header::CONTENT_RANGE, value.parse().unwrap());
            content_range_start(&headers)
        };
        assert_eq!(range("bytes 100-199/200"), Some(100));
        assert_eq!(range("Bytes 0-9/*"), Some(0));
        assert_eq!(range("bytes */200"), None);
        assert_eq!(range("items 1-2/3"), None);
        assert_eq!(content_range_start(&reqwest::header::HeaderMap::new()), None);
    }

    #[tokio::test]
    async fn a_whole_file_that_fails_its_checksum_is_deleted() {
        // Resuming cannot fix a file that arrived whole, so it must not be kept (S1, RPSN-0116).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update.pkg");
        std::fs::write(&path, b"corrupt").unwrap();

        let err = checksum_failed(path.clone(), false).await;

        assert!(!path.exists());
        assert!(matches!(err, DownloadError::HashMismatch { file: None, .. }));
    }

    #[tokio::test]
    async fn a_short_file_is_kept_for_resume_then_discarded() {
        // A short file stays for the retry to resume (S2) and goes once retries stop (S1, RPSN-0116).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update.pkg");
        std::fs::write(&path, b"partial").unwrap();

        let err = checksum_failed(path.clone(), true).await;
        assert!(path.exists());

        err.discard_kept_file();
        assert!(!path.exists());
    }

    fn test_client() -> reqwest::Client {
        super::build_psn_client().expect("failed to build test client")
    }

    #[tokio::test]
    async fn psn_client_checks_certificates_outside_sony() {
        // The certificate bypass Sony needs must not reach any other host (RPSN-0031).
        let result = test_client().get("https://self-signed.badssl.com/").send().await;
        assert!(result.is_err(), "a self-signed certificate on a non-Sony host was accepted");
    }

    #[cfg(feature = "egui")]
    #[tokio::test]
    async fn merge_parts_refuses_folder_outside_download_folder() {
        // A title id carrying "../" points the merge at a folder outside the download folder.
        let root = tempfile::tempdir().unwrap();
        let download_path = root.path().join("downloads");
        std::fs::create_dir(&download_path).unwrap();
        let outside = root.path().join("escape - Title");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("UP0000-CUSA00001_00-PART_0.pkg"), b"part").unwrap();

        let mut package = super::PackageInfo::empty();
        package.url = "https://gs2.ww.prod.dl.playstation.net/ppkg/UP0000-CUSA00001_00-PART_0.pkg".to_string();
        package.part_number = Some(1);
        let info = super::UpdateInfo {
            title_id: "../escape".to_string(),
            tag_name: String::new(),
            titles: vec!["Title".to_string()],
            packages: vec![package],
            platform_variant: super::PlatformVariant::PS4,
        };
        let (tx, _rx) = tokio::sync::mpsc::channel(10);

        let result = info
            .merge_parts(tx, &download_path, crate::utils::DirectoryFormat::SerialTitle)
            .await;

        assert!(matches!(result, Err(super::MergeError::Security(_))));
        assert!(
            !outside.join("UP0000-CUSA00001_00-PART.pkg").exists(),
            "a merged file was written outside"
        );
    }

    #[cfg(feature = "egui")]
    #[tokio::test]
    async fn merge_parts_finds_parts_where_a_failed_migration_left_them() {
        // The download fell back to the old folder; the merge looked only in the new one (RPSN-0118).
        let root = tempfile::tempdir().unwrap();
        let download_path = root.path().join("downloads");
        let old_folder = download_path.join("CUSA00001 - Title");
        std::fs::create_dir_all(&old_folder).unwrap();
        std::fs::write(old_folder.join("UP0000-CUSA00001_00-PART_0.pkg"), b"part").unwrap();
        // A file where the Title / ID layout needs its Title folder makes the migration fail.
        std::fs::write(download_path.join("Title"), b"").unwrap();

        let mut package = super::PackageInfo::empty();
        package.url = "https://gs2.ww.prod.dl.playstation.net/ppkg/UP0000-CUSA00001_00-PART_0.pkg".to_string();
        package.part_number = Some(1);
        let info = super::UpdateInfo {
            title_id: "CUSA00001".to_string(),
            tag_name: String::new(),
            titles: vec!["Title".to_string()],
            packages: vec![package],
            platform_variant: super::PlatformVariant::PS4,
        };
        let (tx, _rx) = tokio::sync::mpsc::channel(10);

        let result = info
            .merge_parts(tx, &download_path, crate::utils::DirectoryFormat::TitleSerial)
            .await;

        assert!(matches!(result, Ok(())), "the merge did not find the downloaded part");
        assert!(old_folder.join("UP0000-CUSA00001_00-PART.pkg").exists());
    }

    #[tokio::test]
    async fn parse_ac3() {
        match super::UpdateInfo::get_info("NPUB30826".to_string(), &test_client()).await {
            Ok(info) => assert!(info.packages.len() == 1),
            Err(e) => panic!("Failed to get info for NPUB30826: {:?}", e),
        }
    }

    #[tokio::test]
    async fn parse_lpb() {
        match super::UpdateInfo::get_info("BCUS98148".to_string(), &test_client()).await {
            Ok(info) => assert!(info.packages.len() == 13),
            Err(e) => panic!("Failed to get info for BCUS98148: {:?}", e),
        }
    }

    #[tokio::test]
    async fn parse_infamous2() {
        match super::UpdateInfo::get_info("NPUA80638".to_string(), &test_client()).await {
            Ok(info) => assert!(info.packages.len() == 3),
            Err(e) => panic!("Failed to get info for NPUA80638: {:?}", e),
        }
    }

    #[tokio::test]
    async fn parse_tokyo_jungle() {
        match super::UpdateInfo::get_info("NPUA80523".to_string(), &test_client()).await {
            Ok(info) => assert!(info.packages.len() == 1),
            Err(e) => panic!("Failed to get info for NPUA80523: {:?}", e),
        }
    }

    // PSVita: Hustle Kings (known to have 1 update)
    #[tokio::test]
    async fn parse_vita_hustle_kings() {
        match super::UpdateInfo::get_info("PCSA00007".to_string(), &test_client()).await {
            Ok(info) => {
                assert!(!info.packages.is_empty(), "Expected at least one package for PCSA00007");
                assert_eq!(info.platform_variant, super::utils::PlatformVariant::PSVita);
            }
            Err(e) => panic!("Failed to get info for PCSA00007: {:?}", e),
        }
    }

    // PSVita: Killzone Mercenary (known to have updates)
    #[tokio::test]
    async fn parse_vita_killzone() {
        match super::UpdateInfo::get_info("PCSA00107".to_string(), &test_client()).await {
            Ok(info) => {
                assert!(!info.packages.is_empty(), "Expected at least one package for PCSA00107");
                assert_eq!(info.platform_variant, super::utils::PlatformVariant::PSVita);
            }
            Err(super::UpdateError::NoUpdatesAvailable) => {
                // Acceptable — Sony may have removed the update metadata.
            }
            Err(e) => panic!("Failed to get info for PCSA00107: {:?}", e),
        }
    }
}
