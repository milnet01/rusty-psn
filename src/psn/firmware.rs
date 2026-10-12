use log::info;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use quick_xml::events::Event;
use quick_xml::Reader;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::Sender;

use super::{DownloadError, DownloadStatus, CANCEL_ABORT, CANCEL_NONE};

/// Maximum firmware list response size: 1 MB.
const MAX_FIRMWARE_LIST_SIZE: u64 = 1_048_576;

/// PS5 firmware URL obfuscation key. Publicly known, embedded in PS5 system software.
/// Used by PySN, Chiaki, and other open-source PlayStation tools.
const PS5_OBF_KEY: &str = "tJMRE80IbXnE9YuG0jzTXgKEjIMoabr6";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Supported firmware platforms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirmwareConsole {
    PS3,
    PS4,
    PS5,
    PSVita,
}

impl FirmwareConsole {
    pub fn all() -> &'static [Self] {
        &[Self::PS3, Self::PS4, Self::PS5, Self::PSVita]
    }

    pub fn short_name(&self) -> &'static str {
        match self {
            Self::PS3 => "PS3",
            Self::PS4 => "PS4",
            Self::PS5 => "PS5",
            Self::PSVita => "PSVita",
        }
    }
}

impl fmt::Display for FirmwareConsole {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::PS3 => write!(f, "PlayStation 3"),
            Self::PS4 => write!(f, "PlayStation 4"),
            Self::PS5 => write!(f, "PlayStation 5"),
            Self::PSVita => write!(f, "PlayStation Vita"),
        }
    }
}

/// Firmware server regions with locale codes used in Sony's CDN URLs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirmwareRegion {
    US,
    EU,
    JP,
    KR,
    UK,
    MX,
    AU,
    SA,
    TW,
    RU,
    CN,
}

impl FirmwareRegion {
    pub fn locale_code(&self) -> &'static str {
        match self {
            Self::US => "us",
            Self::EU => "eu",
            Self::JP => "jp",
            Self::KR => "kr",
            Self::UK => "uk",
            Self::MX => "mx",
            Self::AU => "au",
            Self::SA => "sa",
            Self::TW => "tw",
            Self::RU => "ru",
            Self::CN => "cn",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::US,
            Self::EU,
            Self::JP,
            Self::KR,
            Self::UK,
            Self::MX,
            Self::AU,
            Self::SA,
            Self::TW,
            Self::RU,
            Self::CN,
        ]
    }
}

impl fmt::Display for FirmwareRegion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.locale_code().to_uppercase())
    }
}

/// The kind of firmware image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirmwareType {
    /// Full system update (PS3UPDAT.PUP, PS4UPDATE.PUP, etc.)
    Full,
    /// Incremental patch (PS3PATCH.PUP)
    Patch,
    /// Recovery image
    Recovery,
    /// Vita system data (fonts)
    SystemData,
    /// Vita pre-install data
    PreInstall,
}

impl fmt::Display for FirmwareType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Full => write!(f, "Full Update"),
            Self::Patch => write!(f, "Incremental Patch"),
            Self::Recovery => write!(f, "Recovery Image"),
            Self::SystemData => write!(f, "System Data"),
            Self::PreInstall => write!(f, "Pre-install Data"),
        }
    }
}

/// Information about an available firmware file.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct FirmwareInfo {
    pub console: FirmwareConsole,
    pub region: FirmwareRegion,
    pub version: String,
    pub download_url: String,
    pub size: Option<u64>,
    pub fw_type: FirmwareType,
}

impl FirmwareInfo {
    /// Build a human-readable display name for this firmware entry.
    pub fn display_name(&self) -> String {
        format!("{} v{} - {}", self.console.short_name(), self.version, self.fw_type)
    }

    /// Extract the PUP filename from the download URL.
    fn pup_filename(&self) -> String {
        reqwest::Url::parse(&self.download_url)
            .ok()
            .and_then(|u| u.path_segments()?.next_back().map(|s| s.to_string()))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("firmware_{}.PUP", self.console.short_name()))
    }

    /// Download this firmware file.
    ///
    /// Saves to `{download_path}/Firmware/{console_short}/v{version} {pup_filename}`.
    /// No hash verification is performed — Sony does not provide SHA1 hashes for firmware files.
    pub async fn start_download(
        &self,
        tx: Sender<DownloadStatus>,
        download_path: PathBuf,
        cancel_signal: Arc<AtomicU8>,
        client: &reqwest::Client,
    ) -> Result<(), DownloadError> {
        crate::security::validate_download_url(&self.download_url).map_err(DownloadError::Security)?;

        let fw_dir = download_path.join("Firmware").join(self.console.short_name());
        tokio::fs::create_dir_all(&fw_dir).await.map_err(DownloadError::Tokio)?;

        let pup_name = self.pup_filename();
        let output_filename = format!("v{} {}", self.version, pup_name);
        let output_path = fw_dir.join(&output_filename);

        crate::security::validate_path_containment(&output_path, &download_path).map_err(DownloadError::Security)?;

        // If file already exists at expected size, skip download.
        if output_path.exists() {
            if let Some(expected) = self.size {
                let existing = tokio::fs::metadata(&output_path).await.map_err(DownloadError::Tokio)?.len();
                if existing == expected {
                    info!("Firmware already downloaded: {}", output_path.display());
                    let _ = tx.send(DownloadStatus::DownloadSuccess).await;
                    return Ok(());
                }
            }
        }

        info!("Downloading firmware from {}", &self.download_url);
        let mut response = client.get(&self.download_url).send().await.map_err(DownloadError::Reqwest)?;

        let mut file = tokio::fs::File::create(&output_path).await.map_err(DownloadError::Tokio)?;
        let mut downloaded: u64 = 0;

        while let Some(chunk) = response.chunk().await.map_err(DownloadError::Reqwest)? {
            let signal = cancel_signal.load(Ordering::Relaxed);
            if signal != CANCEL_NONE {
                let abort = signal == CANCEL_ABORT;
                let _ = file.sync_all().await;
                return Err(DownloadError::Cancelled {
                    file_path: Some(output_path),
                    abort,
                });
            }

            downloaded += chunk.len() as u64;
            let _ = tx.send(DownloadStatus::Progress(chunk.len() as u64)).await;
            file.write_all(&chunk).await.map_err(DownloadError::Tokio)?;
        }

        file.sync_all().await.map_err(DownloadError::Tokio)?;

        info!("Firmware download complete: {} ({} bytes)", output_path.display(), downloaded);
        let _ = tx.send(DownloadStatus::DownloadSuccess).await;
        Ok(())
    }
}

#[derive(Debug)]
pub enum FirmwareError {
    Reqwest(reqwest::Error),
    ParseFailed(String),
    Security(crate::security::SecurityError),
    NoFirmwareFound,
    ResponseTooLarge(u64),
}

impl fmt::Display for FirmwareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reqwest(e) => write!(f, "Network error: {e}"),
            Self::ParseFailed(msg) => write!(f, "Parse error: {msg}"),
            Self::Security(e) => write!(f, "Security check failed: {e}"),
            Self::NoFirmwareFound => write!(f, "No firmware found for this region"),
            Self::ResponseTooLarge(size) => write!(f, "Response too large ({size} bytes)"),
        }
    }
}

// ---------------------------------------------------------------------------
// URL construction
// ---------------------------------------------------------------------------

fn get_firmware_list_url(console: FirmwareConsole, region: FirmwareRegion) -> String {
    let locale = region.locale_code();
    match console {
        FirmwareConsole::PS3 => {
            format!("https://f{locale}01.ps3.update.playstation.net/update/ps3/list/{locale}/ps3-updatelist.txt")
        }
        FirmwareConsole::PS4 => {
            format!("https://f{locale}01.ps4.update.playstation.net/update/ps4/list/{locale}/ps4-updatelist.xml")
        }
        FirmwareConsole::PS5 => {
            format!(
                "http://f{locale}01.ps5.update.playstation.net/update/ps5/official/{PS5_OBF_KEY}/list/{locale}/updatelist.xml"
            )
        }
        FirmwareConsole::PSVita => {
            format!("https://f{locale}01.psp2.update.playstation.net/update/psp2/list/{locale}/psp2-updatelist.xml")
        }
    }
}

// ---------------------------------------------------------------------------
// Fetch
// ---------------------------------------------------------------------------

/// Fetch available firmware information for a given console and region.
pub async fn fetch_firmware(
    client: &reqwest::Client,
    console: FirmwareConsole,
    region: FirmwareRegion,
) -> Result<Vec<FirmwareInfo>, FirmwareError> {
    let url = get_firmware_list_url(console, region);

    crate::security::validate_download_url(&url).map_err(FirmwareError::Security)?;

    info!("Fetching firmware list from {url}");

    let response = client.get(&url).send().await.map_err(FirmwareError::Reqwest)?;

    if let Some(len) = response.content_length() {
        if len > MAX_FIRMWARE_LIST_SIZE {
            return Err(FirmwareError::ResponseTooLarge(len));
        }
    }

    let body = response.text().await.map_err(FirmwareError::Reqwest)?;

    if body.len() as u64 > MAX_FIRMWARE_LIST_SIZE {
        return Err(FirmwareError::ResponseTooLarge(body.len() as u64));
    }

    if body.is_empty() {
        return Err(FirmwareError::NoFirmwareFound);
    }

    match console {
        FirmwareConsole::PS3 => parse_ps3_firmware(&body, region),
        _ => parse_xml_firmware(&body, console, region),
    }
}

// ---------------------------------------------------------------------------
// PS3 text-format parser
// ---------------------------------------------------------------------------

/// Parse the PS3 firmware update list (semicolon-delimited key=value text format).
///
/// Format (one entry per line):
/// ```text
/// Dest=84;SystemSoftwareVersion=4.9300;CDN=http://.../PS3UPDAT.PUP;CDN_Timeout=30;
/// ```
fn parse_ps3_firmware(body: &str, region: FirmwareRegion) -> Result<Vec<FirmwareInfo>, FirmwareError> {
    let mut entries = Vec::new();

    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut version = None;
        let mut cdn_url = None;

        for field in line.split(';') {
            if let Some((key, value)) = field.split_once('=') {
                match key.trim() {
                    "SystemSoftwareVersion" => version = Some(value.trim().to_string()),
                    "CDN" => cdn_url = Some(value.trim().to_string()),
                    _ => {}
                }
            }
        }

        let (Some(version), Some(url)) = (version, cdn_url) else {
            continue;
        };

        let fw_type = if url.contains("PS3UPDAT.PUP") {
            FirmwareType::Full
        } else if url.contains("PS3PATCH.PUP") {
            FirmwareType::Patch
        } else {
            continue;
        };

        entries.push(FirmwareInfo {
            console: FirmwareConsole::PS3,
            region,
            version,
            download_url: url,
            size: None, // PS3 firmware list doesn't include file size.
            fw_type,
        });
    }

    if entries.is_empty() {
        return Err(FirmwareError::NoFirmwareFound);
    }

    Ok(entries)
}

// ---------------------------------------------------------------------------
// XML firmware parser (PS4, PS5, PSVita)
// ---------------------------------------------------------------------------

/// Parse XML firmware lists for PS4, PS5, and PSVita.
///
/// All three use a similar `<update_data_list>` structure with `<image>` elements
/// containing the download URL as text content and file size as an attribute.
/// Version information comes from parent `<system_pup>` or `<version>` elements.
fn parse_xml_firmware(body: &str, console: FirmwareConsole, region: FirmwareRegion) -> Result<Vec<FirmwareInfo>, FirmwareError> {
    let mut entries = Vec::new();
    let mut reader = Reader::from_str(body);
    reader.config_mut().trim_text(true);

    // Current parsing state — updated as we traverse elements.
    let mut current_version = String::new();
    let mut current_fw_type = FirmwareType::Full;
    let mut in_image = false;
    let mut image_size: Option<u64> = None;

    let mut event_buf = Vec::new();

    loop {
        match reader.read_event_into(&mut event_buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                match e.name().as_ref() {
                    // PS4/PS5: <system_pup label="13.50" ...>
                    b"system_pup" => {
                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            if attr.key.as_ref() == b"label" {
                                current_version = attr.unescape_value().unwrap_or_default().to_string();
                            }
                        }
                    }
                    // PSVita: <version label="3.74" ...>
                    b"version" => {
                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            if attr.key.as_ref() == b"label" {
                                current_version = attr.unescape_value().unwrap_or_default().to_string();
                            }
                        }
                    }
                    // <update_data update_type="full">
                    b"update_data" => {
                        current_fw_type = FirmwareType::Full;
                    }
                    // PS4: <recovery_pup type="default">
                    b"recovery_pup" => {
                        current_fw_type = FirmwareType::Recovery;
                    }
                    // PSVita: <recovery spkg_type="systemdata|preinst">
                    b"recovery" => {
                        current_fw_type = FirmwareType::Recovery;
                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            if attr.key.as_ref() == b"spkg_type" {
                                let val = attr.unescape_value().unwrap_or_default();
                                current_fw_type = match val.as_ref() {
                                    "systemdata" => FirmwareType::SystemData,
                                    "preinst" => FirmwareType::PreInstall,
                                    _ => FirmwareType::Recovery,
                                };
                            }
                        }
                    }
                    // <image size="503293952">URL</image>
                    b"image" => {
                        in_image = true;
                        image_size = None;
                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            if attr.key.as_ref() == b"size" {
                                let val = attr.unescape_value().unwrap_or_default();
                                image_size = val.parse::<u64>().ok();
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_image {
                    match e.decode() {
                        Ok(url_text) => {
                            let url = url_text.trim().to_string();
                            if !url.is_empty() {
                                entries.push(FirmwareInfo {
                                    console,
                                    region,
                                    version: current_version.clone(),
                                    download_url: url,
                                    size: image_size,
                                    fw_type: current_fw_type,
                                });
                            }
                        }
                        Err(e) => return Err(FirmwareError::ParseFailed(format!("Encoding error: {e}"))),
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                if e.name().as_ref() == b"image" {
                    in_image = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(FirmwareError::ParseFailed(format!("XML error: {e}"))),
            _ => {}
        }
        event_buf.clear();
    }

    if entries.is_empty() {
        return Err(FirmwareError::NoFirmwareFound);
    }

    Ok(entries)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> reqwest::Client {
        crate::psn::build_psn_client().expect("failed to build test client")
    }

    #[tokio::test]
    async fn fetch_ps3_firmware_us() {
        match fetch_firmware(&test_client(), FirmwareConsole::PS3, FirmwareRegion::US).await {
            Ok(entries) => {
                assert!(!entries.is_empty(), "Expected at least one PS3 firmware entry");
                let full = entries.iter().find(|e| e.fw_type == FirmwareType::Full);
                assert!(full.is_some(), "Expected a Full Update entry for PS3");
                let full = full.unwrap();
                assert!(!full.version.is_empty(), "Version should not be empty");
                assert!(full.download_url.contains("PS3UPDAT.PUP"), "URL should point to PS3UPDAT.PUP");
            }
            Err(e) => panic!("Failed to fetch PS3 firmware: {e}"),
        }
    }

    #[tokio::test]
    async fn fetch_ps4_firmware_eu() {
        match fetch_firmware(&test_client(), FirmwareConsole::PS4, FirmwareRegion::EU).await {
            Ok(entries) => {
                assert!(!entries.is_empty(), "Expected at least one PS4 firmware entry");
                let full = entries.iter().find(|e| e.fw_type == FirmwareType::Full);
                assert!(full.is_some(), "Expected a Full Update entry for PS4");
                assert!(full.unwrap().size.is_some(), "PS4 firmware should have size info");
            }
            Err(e) => panic!("Failed to fetch PS4 firmware: {e}"),
        }
    }

    #[tokio::test]
    async fn fetch_vita_firmware_us() {
        match fetch_firmware(&test_client(), FirmwareConsole::PSVita, FirmwareRegion::US).await {
            Ok(entries) => {
                assert!(!entries.is_empty(), "Expected at least one PSVita firmware entry");
                let full = entries.iter().find(|e| e.fw_type == FirmwareType::Full);
                assert!(full.is_some(), "Expected a Full Update entry for PSVita");
            }
            Err(e) => panic!("Failed to fetch PSVita firmware: {e}"),
        }
    }

    #[tokio::test]
    async fn fetch_ps5_firmware_us() {
        match fetch_firmware(&test_client(), FirmwareConsole::PS5, FirmwareRegion::US).await {
            Ok(entries) => {
                assert!(!entries.is_empty(), "Expected at least one PS5 firmware entry");
                let full = entries.iter().find(|e| e.fw_type == FirmwareType::Full);
                assert!(full.is_some(), "Expected a Full Update entry for PS5");
            }
            Err(e) => panic!("Failed to fetch PS5 firmware: {e}"),
        }
    }

    #[test]
    fn parse_ps3_text_format() {
        let body = r#"# US
Dest=84;CompatibleSystemSoftwareVersion=4.9300-;
Dest=84;ImageVersion=00010b94;SystemSoftwareVersion=4.9300;CDN=http://dus01.ps3.update.playstation.net/update/ps3/image/us/2026_0318_abc/PS3UPDAT.PUP;CDN_Timeout=30;
Dest=84;IncrementalUpdateVersion=00010b72-00010b72;ImageVersion=00010b94;SystemSoftwareVersion=4.9300;CDN=http://dus01.ps3.update.playstation.net/update/ps3/image/us/2026_0318_abc/PS3PATCH.PUP;CDN_Timeout=30;
"#;
        let entries = parse_ps3_firmware(body, FirmwareRegion::US).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].fw_type, FirmwareType::Full);
        assert_eq!(entries[0].version, "4.9300");
        assert!(entries[0].download_url.contains("PS3UPDAT.PUP"));
        assert_eq!(entries[1].fw_type, FirmwareType::Patch);
    }

    #[test]
    fn parse_ps4_xml_format() {
        let body = r#"<?xml version="1.0" ?>
<update_data_list>
    <region id="eu">
        <system_pup label="13.50" version="13.508.000">
            <update_data update_type="full">
                <image size="503293952">http://deu01.ps4.update.playstation.net/PS4UPDATE.PUP?dest=eu</image>
            </update_data>
        </system_pup>
        <recovery_pup type="default">
            <system_pup label="13.50" version="13.508.000"/>
            <image size="1083601408">http://deu01.ps4.update.playstation.net/PS4UPDATE_REC.PUP?dest=eu</image>
        </recovery_pup>
    </region>
</update_data_list>"#;
        let entries = parse_xml_firmware(body, FirmwareConsole::PS4, FirmwareRegion::EU).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].fw_type, FirmwareType::Full);
        assert_eq!(entries[0].version, "13.50");
        assert_eq!(entries[0].size, Some(503293952));
        assert_eq!(entries[1].fw_type, FirmwareType::Recovery);
    }

    #[test]
    fn parse_vita_xml_format() {
        let body = r#"<?xml version="1.0" encoding="UTF-8"?>
<update_data_list>
    <region id="us">
        <version system_version="03.740.000" label="3.74">
            <update_data update_type="full">
                <image size="133834240">http://dus01.psp2.update.playstation.net/PSP2UPDAT.PUP?dest=us</image>
            </update_data>
        </version>
        <recovery spkg_type="systemdata">
            <image spkg_version="01.000.010" size="56778752">http://dus01.psp2.update.playstation.net/PSP2UPDAT_SD.PUP?dest=us</image>
        </recovery>
        <recovery spkg_type="preinst">
            <image spkg_version="01.000.000" size="128798720">http://dus01.psp2.update.playstation.net/PSP2UPDAT_PRE.PUP?dest=us</image>
        </recovery>
    </region>
</update_data_list>"#;
        let entries = parse_xml_firmware(body, FirmwareConsole::PSVita, FirmwareRegion::US).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].fw_type, FirmwareType::Full);
        assert_eq!(entries[0].version, "3.74");
        assert_eq!(entries[0].size, Some(133834240));
        assert_eq!(entries[1].fw_type, FirmwareType::SystemData);
        assert_eq!(entries[2].fw_type, FirmwareType::PreInstall);
    }
}
