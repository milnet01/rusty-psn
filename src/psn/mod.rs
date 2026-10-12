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
use utils::{get_platform_variant, get_update_info_url, PlatformVariant};

#[cfg(feature = "egui")]
use crate::utils::create_pkg_path;
use crate::utils::DirectoryFormat;

/// Build a reqwest client configured for Sony PSN servers.
///
/// Sony uses non-standard TLS certificates, so `danger_accept_invalid_certs` is required.
/// The redirect policy validates that each hop stays within allowed Sony domains.
///
/// This client should be constructed once and reused — it holds a connection pool and
/// TLS session cache internally, and `Clone` is just an `Arc::clone`.
pub fn build_psn_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::ClientBuilder::default()
        .danger_accept_invalid_certs(true)
        .redirect(crate::security::sony_redirect_policy())
        .build()
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
    PackagesUnmergable(String),
}

/// Cancel signal values for download pause/abort.
pub const CANCEL_NONE: u8 = 0;
#[cfg(feature = "egui")]
pub const CANCEL_PAUSE: u8 = 1;
pub const CANCEL_ABORT: u8 = 2;

#[derive(Debug)]
pub enum DownloadError {
    // bool represents whether we received less data than expected.
    // Sony's servers like to drop out before the transfer is actually completed.
    HashMismatch(bool),
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

#[derive(Debug)]
pub enum UpdateError {
    InvalidSerial,
    NoUpdatesAvailable,
    UnhandledErrorResponse(String),
    Reqwest(reqwest::Error),
    XmlParsing(quick_xml::Error),
    XmlEncodingError(quick_xml::encoding::EncodingError),
    ManifestParsing(serde_json::Error),
    Security(crate::security::SecurityError),
    ResponseTooLarge(u64),
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

        // Check response size before reading the body into memory.
        if let Some(len) = response.content_length() {
            if len > crate::security::MAX_METADATA_SIZE {
                return Err(UpdateError::ResponseTooLarge(len));
            }
        }

        let response_txt = response.text().await.map_err(UpdateError::Reqwest)?;

        // Double-check the actual body size (chunked encoding can bypass Content-Length).
        if response_txt.len() as u64 > crate::security::MAX_METADATA_SIZE {
            return Err(UpdateError::ResponseTooLarge(response_txt.len() as u64));
        }

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
                parser::ParseError::XmlEncodingError(reason) => return Err(UpdateError::XmlEncodingError(reason)),
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
            if let Some(len) = manifest_response.content_length() {
                if len > crate::security::MAX_METADATA_SIZE {
                    return Err(UpdateError::ResponseTooLarge(len));
                }
            }
            let manifest_response_txt = manifest_response.text().await.map_err(UpdateError::Reqwest)?;
            if manifest_response_txt.len() as u64 > crate::security::MAX_METADATA_SIZE {
                return Err(UpdateError::ResponseTooLarge(manifest_response_txt.len() as u64));
            }
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
        download_path: &PathBuf,
        format: DirectoryFormat,
    ) -> Result<(), MergeError> {
        if !self.packages.iter().all(|pkg| pkg.part_number.is_some()) {
            return Err(MergeError::PackagesUnmergable(String::from(
                "some packages for the update are not a partial package",
            )));
        }

        let mut packages_sorted_by_part_number = self.packages.clone();
        // Safe: the all() check above guarantees every package has a part_number.
        packages_sorted_by_part_number.sort_by_key(|pkg| pkg.part_number.unwrap());
        let package_download_path = create_pkg_path(&download_path, &self.title_id, &self.title(), format);

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
    ) -> Result<(), DownloadError> {
        info!("Starting download for {serial} {}", self.version);
        info!("Sending pkg file request to url: {}", &self.url);

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
            return Ok(());
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

        let mut response = request.send().await.map_err(DownloadError::Reqwest)?;

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
            // 200 or other: server didn't honor Range (or file changed per ETag).
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

        if crate::utils::hash_file(&mut pkg_file, &self.sha1sum, self.hash_whole_file).await? {
            info!("Hash for {serial} {} matched, wrapping up...", self.version);
            let _ = tx.send(DownloadStatus::DownloadSuccess).await;
            Ok(())
        } else {
            error!("Hash mismatch for {serial} {}!", self.version);
            let _ = tx.send(DownloadStatus::DownloadFailure).await;
            Err(DownloadError::HashMismatch(received_data < self.size))
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

#[cfg(test)]
mod tests {
    fn test_client() -> reqwest::Client {
        super::build_psn_client().expect("failed to build test client")
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
