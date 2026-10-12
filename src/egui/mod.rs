use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::DerefMut;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytesize::ByteSize;
use copypasta::{ClipboardContext, ClipboardProvider};
use eframe::egui;
use egui_notify::{Toast, ToastLevel, Toasts};
use log::{error, info, warn};
use notify_rust::Notification;
use poll_promise::Promise;
use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;
use tokio::select;

use crate::psn::*;

pub struct DownloadProgressStatus {
    progress: u64,
    last_received_status: DownloadStatus,
    /// Bytes downloaded at the last speed sample point.
    speed_sample_bytes: u64,
    /// Timestamp of the last speed sample.
    speed_sample_time: std::time::Instant,
    /// Current speed in bytes per second (rolling average).
    speed_bps: f64,
}

impl DownloadProgressStatus {
    /// Update progress and recalculate speed using an exponential moving average.
    /// `min_interval_ms` controls how often the speed is recalculated (e.g., 500 for downloads, 1000 for firmware).
    fn record_progress(&mut self, bytes: u64, min_interval_ms: u64) {
        self.progress += bytes;
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.speed_sample_time);
        if elapsed.as_millis() >= min_interval_ms as u128 {
            let bytes_since = self.progress.saturating_sub(self.speed_sample_bytes);
            let instant_speed = bytes_since as f64 / elapsed.as_secs_f64();
            self.speed_bps = self.speed_bps * 0.3 + instant_speed * 0.7;
            self.speed_sample_bytes = self.progress;
            self.speed_sample_time = now;
        }
    }
}

pub struct ActiveDownload {
    title_id: String,
    pkg_id: String,
    title: String,
    pkg: PackageInfo,
    retry_count: u32,

    size: u64,
    progress_status: Arc<std::sync::Mutex<DownloadProgressStatus>>,
    cancel_signal: Arc<AtomicU8>,

    promise: Promise<Result<(), DownloadError>>,
}

/// Maximum number of automatic retries when Sony's servers drop the connection mid-transfer.
const MAX_AUTO_RETRIES: u32 = 3;

struct FailedDownload {
    title_id: String,
    pkg_id: String,
}

struct PendingDownload {
    title_id: String,
    title: String,
    pkg: PackageInfo,
}

/// Stored so resume can re-queue the download. The `title` and `pkg` fields are consumed
/// indirectly when the draw_entry_pkg closure re-creates the download from its own parameters.
#[allow(dead_code)]
struct PausedDownload {
    title_id: String,
    pkg_id: String,
    title: String,
    pkg: PackageInfo,
}

struct ActiveFirmwareDownload {
    name: String,
    size: u64,
    progress_status: Arc<std::sync::Mutex<DownloadProgressStatus>>,
    cancel_signal: Arc<AtomicU8>,
    promise: Promise<Result<(), DownloadError>>,
}

/// Per-frame cached download status, built once and read many times per frame.
#[derive(Clone, Copy)]
struct CachedDownloadInfo {
    status: ActiveDownloadStatus,
    speed_bps: f64,
    progress_bytes: u64,
    total_bytes: u64,
}

impl Default for CachedDownloadInfo {
    fn default() -> Self {
        CachedDownloadInfo {
            status: ActiveDownloadStatus::NotStarted,
            speed_bps: 0.0,
            progress_bytes: 0,
            total_bytes: 0,
        }
    }
}

#[derive(Default, Clone, Copy, PartialEq)]
enum ResultSortBy {
    #[default]
    SearchOrder,
    Serial,
    Title,
    UpdateCount,
    TotalSize,
}

impl ResultSortBy {
    fn label(self) -> &'static str {
        match self {
            ResultSortBy::SearchOrder => "Search Order",
            ResultSortBy::Serial => "Serial",
            ResultSortBy::Title => "Title",
            ResultSortBy::UpdateCount => "Update Count",
            ResultSortBy::TotalSize => "Total Size",
        }
    }
}

#[derive(Default, Clone, Copy, PartialEq)]
enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

struct DownloadHistoryEntry {
    title_id: String,
    pkg_id: String,
    title: String,
    size: u64,
    completed_at: std::time::Instant,
}

/// Maximum number of concurrent batch search API calls.
const MAX_CONCURRENT_SEARCHES: usize = 3;

fn default_max_concurrent() -> u32 {
    3
}

pub struct MergeProgressStatus {
    progress: usize,
    last_received_status: MergeStatus,
}

pub struct ActiveMerge {
    title_id: String,

    progress_status: Arc<std::sync::Mutex<MergeProgressStatus>>,

    promise: Promise<Result<(), MergeError>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
enum AppTheme {
    #[default]
    Dark,
    Light,
    PlayStation,
}

impl AppTheme {
    fn label(self) -> &'static str {
        match self {
            AppTheme::Dark => "Dark",
            AppTheme::Light => "Light",
            AppTheme::PlayStation => "PlayStation",
        }
    }

    fn apply(self, ctx: &egui::Context) {
        match self {
            AppTheme::Dark => ctx.set_visuals(egui::Visuals::dark()),
            AppTheme::Light => ctx.set_visuals(egui::Visuals::light()),
            AppTheme::PlayStation => {
                let mut visuals = egui::Visuals::dark();
                let ps_blue = egui::Color32::from_rgb(0, 111, 205);
                let ps_blue_dim = egui::Color32::from_rgb(0, 80, 150);
                let ps_blue_faint = egui::Color32::from_rgb(0, 50, 100);

                // Selection and interaction.
                visuals.selection.bg_fill = ps_blue;
                visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
                visuals.widgets.hovered.bg_fill = ps_blue_dim;
                visuals.widgets.active.bg_fill = ps_blue;
                visuals.hyperlink_color = ps_blue;

                // Non-interactive widgets (progress bars, labels).
                visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(28, 32, 45);
                visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(40, 45, 60));

                // Inactive widgets (buttons at rest).
                visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(30, 35, 50);
                visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, ps_blue_faint);
                visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(200, 210, 230));

                // Open widgets (combo boxes, menus).
                visuals.widgets.open.bg_fill = ps_blue_dim;

                // Backgrounds.
                visuals.panel_fill = egui::Color32::from_rgb(18, 20, 28);
                visuals.window_fill = egui::Color32::from_rgb(22, 24, 32);
                visuals.extreme_bg_color = egui::Color32::from_rgb(12, 14, 20);

                // Window border with subtle blue.
                visuals.window_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(40, 50, 75));

                ctx.set_visuals(visuals);
            }
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct AppSettings {
    pkg_download_path: PathBuf,
    show_toasts: bool,
    show_notifications: bool,
    #[serde(default)]
    directory_format: crate::utils::DirectoryFormat,
    #[serde(default = "default_max_concurrent")]
    max_concurrent_downloads: u32,
    #[serde(default)]
    rpcs3_config_dir: Option<PathBuf>,
    #[serde(default)]
    rpcs3_executable: Option<PathBuf>,
    #[serde(default)]
    content_directory: Option<PathBuf>,
    #[serde(default)]
    theme: AppTheme,
}

impl Default for AppSettings {
    fn default() -> AppSettings {
        AppSettings {
            pkg_download_path: default_download_path(),
            show_toasts: true,
            show_notifications: false,
            directory_format: crate::utils::DirectoryFormat::default(),
            max_concurrent_downloads: default_max_concurrent(),
            rpcs3_config_dir: None,
            rpcs3_executable: None,
            content_directory: None,
            theme: AppTheme::default(),
        }
    }
}

// Values that shouldn't be persisted from run to run.
struct VolatileData {
    rt_handle: Option<Handle>,
    toasts: Toasts,

    clipboard: Option<Box<dyn ClipboardProvider>>,

    serial_query: String,
    update_results: Vec<UpdateInfo>,

    show_settings_window: bool,
    show_mismatch_warning_window: bool,
    show_about_window: bool,
    show_migration_dialog: bool,
    migration_preview: Vec<(PathBuf, PathBuf)>,
    migration_promise: Option<Promise<usize>>,

    settings_dirty: bool,
    modified_settings: AppSettings,

    download_queue: Vec<ActiveDownload>,
    failed_downloads: Vec<FailedDownload>,
    completed_downloads: Vec<(String, String)>,
    pending_downloads: VecDeque<PendingDownload>,
    paused_downloads: Vec<PausedDownload>,

    merge_queue: Vec<ActiveMerge>,
    failed_merges: Vec<String>,
    completed_merges: Vec<String>,

    search_promise: Option<Promise<Result<UpdateInfo, UpdateError>>>,

    // Batch search state
    batch_search_queue: VecDeque<String>,
    batch_search_active: Vec<(String, Promise<Result<UpdateInfo, UpdateError>>)>,
    batch_search_total: usize,
    batch_search_completed: usize,
    batch_search_errors: Vec<(String, String)>,

    // RPCS3 integration state
    rpcs3_scan_promise: Option<Promise<Result<Vec<crate::rpcs3::InstalledGame>, crate::rpcs3::Rpcs3Error>>>,
    rpcs3_installed_games: Vec<crate::rpcs3::InstalledGame>,
    show_rpcs3_library_window: bool,

    // Firmware download state
    show_firmware_window: bool,
    firmware_console_idx: usize,
    firmware_region_idx: usize,
    firmware_fetch_promise: Option<Promise<Result<Vec<crate::psn::firmware::FirmwareInfo>, crate::psn::firmware::FirmwareError>>>,
    firmware_results: Vec<crate::psn::firmware::FirmwareInfo>,
    firmware_download: Option<ActiveFirmwareDownload>,

    /// Per-frame download status cache. Built once at the start of each frame.
    download_status_cache: HashMap<(String, String), CachedDownloadInfo>,

    // Results sort/filter state.
    results_filter: String,
    results_sort_by: ResultSortBy,
    results_sort_direction: SortDirection,

    // Keyboard shortcut flags.
    focus_search_bar: bool,

    // Download history (session-only).
    download_history: Vec<DownloadHistoryEntry>,
    show_download_history_window: bool,

    // RPCS3 install state. Key is (title_id, pkg_id).
    #[allow(clippy::type_complexity)]
    rpcs3_install_promises: Vec<(String, String, Promise<Result<(), crate::rpcs3::Rpcs3Error>>)>,
    rpcs3_installed_pkgs: HashSet<(String, String)>,

    // Content browser state.
    show_content_browser_window: bool,
    content_library: Option<crate::content::ContentLibrary>,
    content_filter: String,
    /// Tracks DLC installs in progress: zip filename → promise.
    content_install_promises: Vec<(String, Promise<Result<(), crate::content::ContentError>>)>,
    content_installed_items: HashSet<String>,

    /// Cached egui context for use in code paths without direct UI access (e.g., auto-retry).
    egui_ctx: Option<egui::Context>,

    /// Shared HTTP client for all PSN requests. Constructed once, reused across all
    /// searches and downloads. reqwest::Client is cheaply cloneable (Arc internally).
    psn_client: Option<reqwest::Client>,
}

impl Default for VolatileData {
    fn default() -> VolatileData {
        let clipboard: Option<Box<dyn ClipboardProvider>> = {
            match ClipboardContext::new() {
                Ok(clip) => Some(Box::new(clip)),
                Err(e) => {
                    error!("Failed to init clipboard: {e}");
                    None
                }
            }
        };

        VolatileData {
            rt_handle: None,
            toasts: Toasts::default().reverse(true).with_anchor(egui_notify::Anchor::BottomRight),

            clipboard,

            serial_query: String::new(),
            update_results: Vec::new(),

            show_settings_window: false,
            show_mismatch_warning_window: false,
            show_about_window: false,
            show_migration_dialog: false,
            migration_preview: Vec::new(),
            migration_promise: None,

            settings_dirty: false,
            modified_settings: AppSettings::default(),

            download_queue: Vec::new(),
            failed_downloads: Vec::new(),
            completed_downloads: Vec::new(),
            pending_downloads: VecDeque::new(),
            paused_downloads: Vec::new(),

            merge_queue: Vec::new(),
            failed_merges: Vec::new(),
            completed_merges: Vec::new(),

            search_promise: None,

            batch_search_queue: VecDeque::new(),
            batch_search_active: Vec::new(),
            batch_search_total: 0,
            batch_search_completed: 0,
            batch_search_errors: Vec::new(),

            rpcs3_scan_promise: None,
            rpcs3_installed_games: Vec::new(),
            show_rpcs3_library_window: false,

            show_firmware_window: false,
            firmware_console_idx: 0,
            firmware_region_idx: 0,
            firmware_fetch_promise: None,
            firmware_results: Vec::new(),
            firmware_download: None,

            download_status_cache: HashMap::new(),

            results_filter: String::new(),
            results_sort_by: ResultSortBy::default(),
            results_sort_direction: SortDirection::default(),

            focus_search_bar: false,

            download_history: Vec::new(),
            show_download_history_window: false,

            rpcs3_install_promises: Vec::new(),
            rpcs3_installed_pkgs: HashSet::new(),

            show_content_browser_window: false,
            content_library: None,
            content_filter: String::new(),
            content_install_promises: Vec::new(),
            content_installed_items: HashSet::new(),

            egui_ctx: None,

            psn_client: match crate::psn::build_psn_client() {
                Ok(c) => Some(c),
                Err(e) => {
                    error!("Failed to build HTTP client: {e}");
                    None
                }
            },
        }
    }
}

#[derive(Default, Deserialize, Serialize)]
pub struct UpdatesApp {
    #[serde(skip)]
    v: VolatileData,
    settings: AppSettings,
}

impl eframe::App for UpdatesApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Build per-frame download status cache (avoids repeated linear scans).
        self.v.download_status_cache = self.build_download_status_cache();

        // Handle global keyboard shortcuts.
        self.handle_keyboard_shortcuts(ctx);

        // Menu bar must come before CentralPanel.
        self.draw_menu_bar(ctx);
        // Status bar must come before CentralPanel.
        self.draw_status_bar(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            self.draw_search_bar(ui);
            ui.separator();
            self.draw_results_list(ctx, ui);
        });

        if self.v.show_settings_window {
            self.draw_settings_window(ctx);
        }

        if self.v.show_mismatch_warning_window {
            self.draw_hash_mismatch_window(ctx);
        }

        if self.v.show_about_window {
            self.draw_about_window(ctx);
        }

        if self.v.show_migration_dialog {
            self.draw_migration_dialog(ctx);
        }

        if self.v.show_rpcs3_library_window {
            self.draw_rpcs3_library_window(ctx);
        }

        if self.v.show_firmware_window {
            self.draw_firmware_window(ctx);
        }

        if self.v.show_download_history_window {
            self.draw_download_history_window(ctx);
        }

        if self.v.show_content_browser_window {
            self.draw_content_browser_window(ctx);
        }

        let mut toasts = Vec::new();

        // Check the status of the search promise.
        self.handle_search_promise(&mut toasts);
        // Process batch search queue (parallel searches with concurrency limit).
        self.handle_batch_search_queue(&mut toasts);
        // Check in on active downloads.
        self.handle_download_promises(&mut toasts);
        self.handle_merge_promises(&mut toasts);
        self.handle_rpcs3_scan_promise(&mut toasts);
        self.handle_firmware_promises(&mut toasts);
        self.handle_rpcs3_install_promises(&mut toasts);
        self.handle_content_install_promises(&mut toasts);

        for (msg, level) in toasts {
            self.show_notifications(msg, level);
        }

        self.v.toasts.show(ctx);
    }
}

impl UpdatesApp {
    pub fn new(cc: &eframe::CreationContext<'_>, rt_handle: Handle) -> Self {
        let mut fonts = egui::FontDefinitions::default();

        fonts.font_data.insert(
            "noto".to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
                "../../resources/NotoSans-Regular.ttf"
            ))),
        );

        fonts.font_data.insert(
            "notojp".to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
                "../../resources/NotoSansJP-Regular.otf"
            ))),
        );

        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "noto".to_owned());
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(1, "notojp".to_owned());

        cc.egui_ctx.set_fonts(fonts);

        let mut app: Self = if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default()
        } else {
            Default::default()
        };

        app.v.rt_handle = Some(rt_handle);
        app.v.egui_ctx = Some(cc.egui_ctx.clone());

        // Apply the persisted theme on startup.
        app.settings.theme.apply(&cc.egui_ctx);

        app
    }

    fn handle_search_promise(&mut self, toasts: &mut Vec<(String, ToastLevel)>) -> Option<()> {
        let is_ready = {
            let promise = self.v.search_promise.as_ref()?;
            promise.ready().is_some()
        };

        if is_ready {
            let promise = self.v.search_promise.take()?;
            let promise_ready = promise.block_and_take();

            match promise_ready {
                Ok(update_info) => {
                    info!("Received search results for serial {}", update_info.title_id);
                    self.v.update_results.push(update_info);
                }
                Err(ref e) => {
                    match e {
                        UpdateError::UnhandledErrorResponse(e) => {
                            toasts.push((
                                format!("Unexpected error received in a response from PSN ({e})."),
                                ToastLevel::Error,
                            ));
                        }
                        UpdateError::InvalidSerial => {
                            toasts.push((
                                String::from("The provided serial didn't give any results, double-check your input."),
                                ToastLevel::Error,
                            ));
                        }
                        UpdateError::NoUpdatesAvailable => {
                            toasts.push((
                                String::from("The provided serial doesn't have any available updates."),
                                ToastLevel::Error,
                            ));
                        }
                        UpdateError::Reqwest(e) => {
                            toasts.push((format!("There was an error completing the request ({e})."), ToastLevel::Error));
                        }
                        UpdateError::XmlParsing(e) => {
                            toasts.push((
                                format!("Error parsing response from Sony, try again later ({e})."),
                                ToastLevel::Error,
                            ));
                        }
                        UpdateError::XmlEncodingError(e) => {
                            toasts.push((
                                format!("Error parsing response from Sony, try again later ({e})."),
                                ToastLevel::Error,
                            ));
                        }
                        UpdateError::ManifestParsing(e) => {
                            toasts.push((
                                format!("Error parsing manifest response from Sony, try again later ({e})."),
                                ToastLevel::Error,
                            ));
                        }
                        UpdateError::Security(e) => {
                            toasts.push((format!("Security check failed: {e}"), ToastLevel::Error));
                        }
                        UpdateError::ResponseTooLarge(size) => {
                            toasts.push((
                                format!("Response too large ({} bytes), possible server error.", size),
                                ToastLevel::Error,
                            ));
                        }
                    }

                    error!("Error received from updates query: {e:?}");
                }
            }
        }

        Some(())
    }

    fn handle_download_promises(&mut self, toasts: &mut Vec<(String, ToastLevel)>) {
        let mut entries_to_remove = Vec::new();
        // Collect downloads to auto-retry (Sony server drops). Processed after the loop
        // to avoid borrow conflicts with self.
        let mut auto_retry_queue: Vec<(PackageInfo, String, String, u32)> = Vec::new();
        let mut pause_queue: Vec<PausedDownload> = Vec::new();
        let mut abort_file_cleanup: Vec<PathBuf> = Vec::new();

        for (i, download) in self.v.download_queue.iter_mut().enumerate() {
            // Check if the download promise is resolved (finished or failed).
            if let Some(r) = download.promise.ready() {
                // Queue up for removal.
                entries_to_remove.push(i);

                match r {
                    Ok(_) => {
                        info!("Download completed! ({} {})", &download.title_id, &download.pkg_id);

                        toasts.push((
                            format!("{} v{} downloaded successfully!", &download.title_id, &download.pkg_id),
                            ToastLevel::Success,
                        ));
                        self.v
                            .completed_downloads
                            .push((download.title_id.clone(), download.pkg_id.clone()));
                        self.v.download_history.push(DownloadHistoryEntry {
                            title_id: download.title_id.clone(),
                            pkg_id: download.pkg_id.clone(),
                            title: download.title.clone(),
                            size: download.size,
                            completed_at: std::time::Instant::now(),
                        });
                    }
                    Err(e) => {
                        match e {
                            DownloadError::HashMismatch(short_on_data) => {
                                if *short_on_data && download.retry_count < MAX_AUTO_RETRIES {
                                    // Sony dropped the connection. Auto-retry with resume.
                                    info!(
                                        "Short transfer for {} {}, auto-retrying ({}/{})",
                                        download.title_id,
                                        download.pkg_id,
                                        download.retry_count + 1,
                                        MAX_AUTO_RETRIES
                                    );
                                    toasts.push((
                                        format!(
                                            "{} v{}: Transfer interrupted, retrying ({}/{})...",
                                            download.title_id,
                                            download.pkg_id,
                                            download.retry_count + 1,
                                            MAX_AUTO_RETRIES
                                        ),
                                        ToastLevel::Warning,
                                    ));
                                    auto_retry_queue.push((
                                        download.pkg.clone(),
                                        download.title_id.clone(),
                                        download.title.clone(),
                                        download.retry_count + 1,
                                    ));
                                } else {
                                    // True hash mismatch or max retries exceeded.
                                    self.v.failed_downloads.push(FailedDownload {
                                        title_id: download.title_id.clone(),
                                        pkg_id: download.pkg_id.clone(),
                                    });
                                    toasts.push((
                                        format!(
                                            "Failed to download {} v{}: Hash mismatch.",
                                            download.title_id, download.pkg_id
                                        ),
                                        ToastLevel::Error,
                                    ));
                                    if *short_on_data {
                                        self.v.show_mismatch_warning_window = true;
                                    }
                                }
                            }
                            DownloadError::Tokio(err) => {
                                self.v.failed_downloads.push(FailedDownload {
                                    title_id: download.title_id.clone(),
                                    pkg_id: download.pkg_id.clone(),
                                });
                                toasts.push((
                                    format!("Failed to download {} v{}: {err}", download.title_id, download.pkg_id),
                                    ToastLevel::Error,
                                ));
                            }
                            DownloadError::Reqwest(err) => {
                                self.v.failed_downloads.push(FailedDownload {
                                    title_id: download.title_id.clone(),
                                    pkg_id: download.pkg_id.clone(),
                                });
                                toasts.push((
                                    format!("Failed to download {} v{}: {err}", download.title_id, download.pkg_id),
                                    ToastLevel::Error,
                                ));
                            }
                            DownloadError::Security(err) => {
                                self.v.failed_downloads.push(FailedDownload {
                                    title_id: download.title_id.clone(),
                                    pkg_id: download.pkg_id.clone(),
                                });
                                toasts.push((
                                    format!("Security check failed for {} v{}: {err}", download.title_id, download.pkg_id),
                                    ToastLevel::Error,
                                ));
                            }
                            DownloadError::Cancelled { file_path, abort } => {
                                if *abort {
                                    info!("Aborted download {} {}, cleaning up", download.title_id, download.pkg_id);
                                    if let Some(path) = file_path {
                                        abort_file_cleanup.push(path.clone());
                                    }
                                    toasts.push((
                                        format!("{} v{} aborted", download.title_id, download.pkg_id),
                                        ToastLevel::Warning,
                                    ));
                                } else {
                                    info!("Paused download {} {}", download.title_id, download.pkg_id);
                                    pause_queue.push(PausedDownload {
                                        title_id: download.title_id.clone(),
                                        pkg_id: download.pkg_id.clone(),
                                        title: download.title.clone(),
                                        pkg: download.pkg.clone(),
                                    });
                                    toasts.push((format!("{} v{} paused", download.title_id, download.pkg_id), ToastLevel::Info));
                                }
                            }
                        }

                        if !matches!(e, DownloadError::Cancelled { .. }) {
                            error!(
                                "Error received from pkg download ({} {}): {:?}",
                                download.title_id, download.pkg_id, e
                            );
                        }
                    }
                }
            }
        }

        for index in entries_to_remove.into_iter().rev() {
            self.v.download_queue.remove(index);
        }

        // Process auto-retries after the borrow on download_queue is released.
        // The partial file on disk is preserved, so start_download() will automatically
        // resume from where it left off via HTTP Range headers.
        for (pkg, title_id, title, retry_count) in auto_retry_queue {
            let download = self.start_download_with_retry(title_id, title, pkg, retry_count);
            self.add_download(download);
        }

        // Process paused downloads — move to paused list for later resume.
        for paused in pause_queue {
            self.v.paused_downloads.push(paused);
        }

        // Delete partial files for aborted downloads.
        for path in abort_file_cleanup {
            if let Err(e) = std::fs::remove_file(&path) {
                warn!("Failed to delete partial file {}: {e}", path.display());
            } else {
                info!("Deleted partial file: {}", path.display());
            }
        }

        // Promote pending downloads into active slots.
        let max = self.settings.max_concurrent_downloads as usize;
        while self.v.download_queue.len() < max {
            if let Some(pending) = self.v.pending_downloads.pop_front() {
                let active = self.start_download_with_retry(pending.title_id, pending.title, pending.pkg, 0);
                self.v.download_queue.push(active);
            } else {
                break;
            }
        }
    }

    fn handle_merge_promises(&mut self, toasts: &mut Vec<(String, ToastLevel)>) {
        let mut finished_merge_indexes: Vec<usize> = Vec::new();
        for i in 0..self.v.merge_queue.len() {
            let merge = &mut self.v.merge_queue[i];
            if let Some(result) = merge.promise.ready() {
                match result {
                    Ok(_) => {
                        info!("Merge completed for {}", &merge.title_id);

                        toasts.push((format!("{} merged successfully!", &merge.title_id), ToastLevel::Success));
                        self.v.completed_merges.push(merge.title_id.clone());
                    }
                    Err(e) => {
                        self.v.failed_merges.push(merge.title_id.clone());

                        match e {
                            MergeError::FilepathMismatch(reason) | MergeError::PackagesUnmergable(reason) => {
                                toasts.push((format!("Failed to merge {}: {reason}", merge.title_id), ToastLevel::Error));
                            }
                            MergeError::FileMergeFailure => {
                                toasts.push((
                                    format!("Failed to merge {}. Check the log for details.", merge.title_id),
                                    ToastLevel::Error,
                                ));
                            }
                        }

                        error!("Could not merge files for {}, reason: {:?}", merge.title_id, e);
                    }
                }

                finished_merge_indexes.push(i);
            }
        }

        for idx in finished_merge_indexes.iter().rev() {
            self.v.merge_queue.remove(*idx);
        }
    }

    fn handle_batch_search_queue(&mut self, toasts: &mut Vec<(String, ToastLevel)>) {
        // Drain completed batch search promises.
        let mut completed_indices = Vec::new();
        for (i, (serial, promise)) in self.v.batch_search_active.iter().enumerate() {
            if promise.ready().is_some() {
                completed_indices.push((i, serial.clone()));
            }
        }

        for (i, serial) in completed_indices.into_iter().rev() {
            let (_, promise) = self.v.batch_search_active.remove(i);
            self.v.batch_search_completed += 1;

            match promise.block_and_take() {
                Ok(update_info) => {
                    info!("Batch search result for {}: {} updates", serial, update_info.packages.len());
                    self.v.update_results.push(update_info);
                }
                Err(e) => {
                    let msg = match &e {
                        UpdateError::InvalidSerial => "invalid serial".to_string(),
                        UpdateError::NoUpdatesAvailable => "no updates available".to_string(),
                        UpdateError::Reqwest(e) => format!("request error: {e}"),
                        _ => format!("{e:?}"),
                    };
                    info!("Batch search failed for {serial}: {msg}");
                    self.v.batch_search_errors.push((serial.clone(), msg));
                }
            }
        }

        // Fill available search slots from the queue.
        while self.v.batch_search_active.len() < MAX_CONCURRENT_SEARCHES {
            let serial = match self.v.batch_search_queue.pop_front() {
                Some(s) => s,
                None => break,
            };

            // Skip if already in results (from manual search or earlier batch).
            if self.v.update_results.iter().any(|u| u.title_id == serial) {
                self.v.batch_search_completed += 1;
                continue;
            }

            let _guard = self
                .v
                .rt_handle
                .as_ref()
                .expect("rt_handle set in UpdatesApp::new before any UI code runs")
                .enter();
            let query = serial.clone();
            let c = self.v.psn_client.clone().expect("psn_client built in VolatileData::default");
            let promise = Promise::spawn_async(async move { UpdateInfo::get_info(query, &c).await });
            self.v.batch_search_active.push((serial, promise));
        }

        // Show summary toast when batch completes.
        if self.v.batch_search_total > 0
            && self.v.batch_search_completed >= self.v.batch_search_total
            && self.v.batch_search_active.is_empty()
            && self.v.batch_search_queue.is_empty()
        {
            let errors = self.v.batch_search_errors.len();
            let found = self.v.batch_search_total - errors;
            let msg = if errors > 0 {
                format!("Batch search complete: {found} found, {errors} failed.")
            } else {
                format!("Batch search complete: {found} title(s) found.")
            };
            toasts.push((msg, ToastLevel::Info));
            self.v.batch_search_total = 0; // Reset so the toast doesn't fire again.
        }
    }

    fn trigger_batch_import(&mut self) {
        let file = rfd::FileDialog::new().add_filter("Text files", &["txt"]).pick_file();

        let path = match file {
            Some(p) => p,
            None => return, // User cancelled.
        };

        match crate::batch::parse_batch_file(&path) {
            Ok(result) => {
                if !result.warnings.is_empty() {
                    self.show_notifications(
                        format!("Batch import: skipped {} invalid serial(s)", result.warnings.len()),
                        ToastLevel::Warning,
                    );
                }

                // Filter out serials already in results or already queued.
                let new_serials: Vec<String> = result
                    .serials
                    .into_iter()
                    .filter(|s| !self.v.update_results.iter().any(|u| u.title_id == *s))
                    .filter(|s| !self.v.batch_search_queue.iter().any(|q| q == s))
                    .filter(|s| !self.v.batch_search_active.iter().any(|(id, _)| id == s))
                    .collect();

                if new_serials.is_empty() {
                    self.show_notifications("All serials from the batch file are already loaded.", ToastLevel::Info);
                    return;
                }

                self.v.batch_search_total = new_serials.len();
                self.v.batch_search_completed = 0;
                self.v.batch_search_errors.clear();
                self.v.batch_search_queue = new_serials.into();

                self.show_notifications(
                    format!("Batch import: {} serial(s) queued for search.", self.v.batch_search_total),
                    ToastLevel::Info,
                );
            }
            Err(e) => {
                self.show_notifications(format!("Failed to import batch file: {e}"), ToastLevel::Error);
            }
        }
    }

    fn trigger_rpcs3_scan(&mut self) {
        let config_dir = match &self.settings.rpcs3_config_dir {
            Some(dir) => dir.clone(),
            None => {
                self.show_notifications("RPCS3 not configured. Set the path in Settings.", ToastLevel::Error);
                return;
            }
        };

        // Resolve dev_hdd0 path from vfs.yml.
        let vfs_path = config_dir.join("vfs.yml");
        let dev_hdd0_path = if vfs_path.exists() {
            match crate::rpcs3::config::parse_vfs_yml(&vfs_path, &config_dir) {
                Ok(vfs) => vfs.dev_hdd0_path,
                Err(e) => {
                    warn!("Failed to parse vfs.yml: {e}, using default");
                    config_dir.join("dev_hdd0")
                }
            }
        } else {
            config_dir.join("dev_hdd0")
        };

        let installation = crate::rpcs3::Rpcs3Installation {
            config_dir,
            dev_hdd0_path,
            executable: None, // Manual config path — executable detected separately
        };

        info!("Starting RPCS3 library scan");
        let _guard = self
            .v
            .rt_handle
            .as_ref()
            .expect("rt_handle set in UpdatesApp::new before any UI code runs")
            .enter();
        self.v.rpcs3_scan_promise = Some(Promise::spawn_async(async move {
            // scan_library is sync but we wrap it in async for consistency with the promise pattern.
            crate::rpcs3::scan_library(&installation)
        }));
    }

    fn handle_rpcs3_scan_promise(&mut self, toasts: &mut Vec<(String, ToastLevel)>) {
        let is_ready = self.v.rpcs3_scan_promise.as_ref().is_some_and(|p| p.ready().is_some());

        if !is_ready {
            return;
        }

        let promise = self.v.rpcs3_scan_promise.take().expect("checked above");
        match promise.block_and_take() {
            Ok(games) => {
                let count = games.len();
                info!("RPCS3 scan found {count} installed games");

                // Feed all serials into the batch search queue for PSN update checking.
                let serials: Vec<String> = games
                    .iter()
                    .map(|g| g.serial.clone())
                    .filter(|s| !self.v.update_results.iter().any(|u| u.title_id == *s))
                    .filter(|s| !self.v.batch_search_queue.iter().any(|q| q == s))
                    .filter(|s| !self.v.batch_search_active.iter().any(|(id, _)| id == s))
                    .collect();

                let search_count = serials.len();
                self.v.batch_search_total = search_count;
                self.v.batch_search_completed = 0;
                self.v.batch_search_errors.clear();
                self.v.batch_search_queue = serials.into();

                self.v.rpcs3_installed_games = games;
                self.v.show_rpcs3_library_window = true;

                toasts.push((
                    format!("RPCS3 scan: {count} game(s) found, checking {search_count} for updates..."),
                    ToastLevel::Info,
                ));
            }
            Err(e) => {
                toasts.push((format!("RPCS3 scan failed: {e}"), ToastLevel::Error));
            }
        }
    }

    fn draw_rpcs3_library_window(&mut self, ctx: &egui::Context) {
        let mut show = self.v.show_rpcs3_library_window;

        // Pre-compute outdated games list outside the window closure to minimize cloning.
        // Only the title_id/title/pkg tuples needed for downloading are cloned, not the full lists.
        let game_count = self.v.rpcs3_installed_games.len();
        let games_to_update: Vec<_> = self
            .v
            .rpcs3_installed_games
            .iter()
            .filter_map(|game| {
                let installed_ver = game.installed_version.as_ref()?;
                let update = self.v.update_results.iter().find(|u| u.title_id == game.serial)?;
                let latest_pkg = update.packages.last()?;
                if crate::rpcs3::compare_versions(installed_ver, &latest_pkg.version) == std::cmp::Ordering::Less {
                    Some((update.title_id.clone(), update.title(), latest_pkg.clone()))
                } else {
                    None
                }
            })
            .collect();
        let outdated_count = games_to_update.len();

        // Collect download actions from the window, then apply them after the closure.
        let mut download_actions: Vec<(String, String, PackageInfo)> = Vec::new();

        egui::Window::new("RPCS3 Library")
            .open(&mut show)
            .resizable(true)
            .default_size([700.0, 400.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{game_count} game(s) installed, {outdated_count} with updates available"
                    ));

                    if outdated_count > 0 && ui.button("Update All Outdated").clicked() {
                        download_actions = games_to_update;
                    }
                });

                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Grid::new("rpcs3_library_grid")
                        .striped(true)
                        .min_col_width(60.0)
                        .show(ui, |ui| {
                            // Header row.
                            ui.strong("Serial");
                            ui.strong("Title");
                            ui.strong("Installed");
                            ui.strong("Available");
                            ui.strong("Status");
                            ui.end_row();

                            for i in 0..self.v.rpcs3_installed_games.len() {
                                let game = &self.v.rpcs3_installed_games[i];
                                ui.label(&game.serial);
                                ui.label(&game.title);

                                let installed_ver = game.installed_version.as_deref().unwrap_or("--");
                                ui.label(installed_ver);

                                // Look up the PSN update info for this serial.
                                let update_info = self.v.update_results.iter().find(|u| u.title_id == game.serial);
                                let available_ver = update_info
                                    .and_then(|u| u.packages.last())
                                    .map(|p| p.version.as_str())
                                    .unwrap_or("--");
                                ui.label(available_ver);

                                // Status column.
                                if let (Some(inst), Some(update)) = (&game.installed_version, update_info) {
                                    if let Some(latest) = update.packages.last() {
                                        match crate::rpcs3::compare_versions(inst, &latest.version) {
                                            std::cmp::Ordering::Less => {
                                                ui.label(
                                                    egui::RichText::new("Update available")
                                                        .color(egui::Rgba::from_rgb(1.0, 1.0, 0.6)),
                                                );
                                            }
                                            _ => {
                                                ui.label(
                                                    egui::RichText::new("Up to date").color(egui::Rgba::from_rgb(0.0, 1.0, 0.0)),
                                                );
                                            }
                                        }
                                    } else {
                                        ui.label("No updates");
                                    }
                                } else if update_info.is_none()
                                    && (self.v.batch_search_active.iter().any(|(s, _)| s == &game.serial)
                                        || self.v.batch_search_queue.iter().any(|s| s == &game.serial))
                                {
                                    ui.label(egui::RichText::new("Checking...").color(egui::Rgba::from_rgb(0.6, 0.6, 0.6)));
                                } else {
                                    ui.label("--");
                                }

                                ui.end_row();
                            }
                        });
                });
            });

        // Apply queued download actions after the window closure.
        for (title_id, title, pkg) in download_actions {
            self.queue_download_no_ui(title_id, title, pkg);
        }

        if !show {
            self.v.show_rpcs3_library_window = false;
        }
    }

    fn draw_firmware_window(&mut self, ctx: &egui::Context) {
        let mut show = self.v.show_firmware_window;

        // Collect download action outside the window closure to avoid borrow issues.
        let mut download_fw: Option<crate::psn::firmware::FirmwareInfo> = None;

        egui::Window::new("Download Firmware")
            .open(&mut show)
            .resizable(true)
            .default_size([550.0, 350.0])
            .show(ctx, |ui| {
                let consoles = crate::psn::firmware::FirmwareConsole::all();
                let regions = crate::psn::firmware::FirmwareRegion::all();

                ui.horizontal(|ui| {
                    ui.label("Console:");
                    egui::ComboBox::from_id_salt("fw_console")
                        .selected_text(consoles[self.v.firmware_console_idx].to_string())
                        .show_ui(ui, |ui| {
                            for (i, console) in consoles.iter().enumerate() {
                                ui.selectable_value(&mut self.v.firmware_console_idx, i, console.to_string());
                            }
                        });

                    ui.label("Region:");
                    egui::ComboBox::from_id_salt("fw_region")
                        .selected_text(regions[self.v.firmware_region_idx].to_string())
                        .show_ui(ui, |ui| {
                            for (i, region) in regions.iter().enumerate() {
                                ui.selectable_value(&mut self.v.firmware_region_idx, i, region.to_string());
                            }
                        });

                    let fetching = self.v.firmware_fetch_promise.is_some();
                    ui.add_enabled_ui(!fetching, |ui| {
                        if ui.button("Check for Firmware").clicked() {
                            let console = consoles[self.v.firmware_console_idx];
                            let region = regions[self.v.firmware_region_idx];
                            let client = self.v.psn_client.clone().expect("psn_client built in VolatileData::default");
                            let ctx_clone = ui.ctx().clone();
                            let _guard = self
                                .v
                                .rt_handle
                                .as_ref()
                                .expect("rt_handle set in UpdatesApp::new before any UI code runs")
                                .enter();
                            self.v.firmware_results.clear();
                            self.v.firmware_fetch_promise = Some(Promise::spawn_async(async move {
                                let result = crate::psn::firmware::fetch_firmware(&client, console, region).await;
                                ctx_clone.request_repaint();
                                result
                            }));
                        }
                    });
                    if fetching {
                        ui.spinner();
                    }
                });

                ui.separator();

                if self.v.firmware_results.is_empty() && self.v.firmware_fetch_promise.is_none() {
                    ui.label("Select a console and region, then click \"Check for Firmware\".");
                }

                if !self.v.firmware_results.is_empty() {
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                        egui::Grid::new("firmware_grid")
                            .striped(true)
                            .min_col_width(80.0)
                            .show(ui, |ui| {
                                ui.strong("Type");
                                ui.strong("Version");
                                ui.strong("Size");
                                ui.strong("");
                                ui.end_row();

                                for entry in &self.v.firmware_results {
                                    ui.label(entry.fw_type.to_string());
                                    ui.label(&entry.version);
                                    ui.label(
                                        entry
                                            .size
                                            .map(|s| ByteSize::b(s).to_string())
                                            .unwrap_or_else(|| "--".to_string()),
                                    );

                                    let downloading = self.v.firmware_download.is_some();
                                    ui.add_enabled_ui(!downloading, |ui| {
                                        if ui.button("Download").clicked() {
                                            download_fw = Some(entry.clone());
                                        }
                                    });
                                    ui.end_row();
                                }
                            });
                    });
                }

                // Active firmware download progress.
                if let Some(dl) = &self.v.firmware_download {
                    ui.separator();
                    ui.label(format!("Downloading: {}", dl.name));

                    let status = dl.progress_status.lock().unwrap();
                    let progress = status.progress;
                    let speed = status.speed_bps;
                    drop(status);

                    if dl.size > 0 {
                        let fraction = progress as f32 / dl.size as f32;
                        ui.add(egui::ProgressBar::new(fraction).text(format!(
                            "{} / {} ({}/s)",
                            ByteSize::b(progress),
                            ByteSize::b(dl.size),
                            ByteSize::b(speed as u64),
                        )));
                    } else {
                        ui.add(egui::ProgressBar::new(0.0).text(format!(
                            "{} downloaded ({}/s)",
                            ByteSize::b(progress),
                            ByteSize::b(speed as u64),
                        )));
                    }

                    if ui.button("Cancel").clicked() {
                        dl.cancel_signal.store(CANCEL_ABORT, Ordering::Relaxed);
                    }
                }
            });

        // Trigger firmware download if requested.
        if let Some(fw) = download_fw {
            self.start_firmware_download(fw);
        }

        if !show {
            self.v.show_firmware_window = false;
        }
    }

    fn start_firmware_download(&mut self, fw: crate::psn::firmware::FirmwareInfo) {
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let name = fw.display_name();
        let size = fw.size.unwrap_or(0);
        let download_path = self.settings.pkg_download_path.clone();
        let cancel_signal = Arc::new(AtomicU8::new(CANCEL_NONE));
        let cancel_for_task = cancel_signal.clone();

        let status = Arc::new(std::sync::Mutex::new(DownloadProgressStatus {
            progress: 0,
            last_received_status: DownloadStatus::Progress(0),
            speed_sample_bytes: 0,
            speed_sample_time: std::time::Instant::now(),
            speed_bps: 0.0,
        }));
        let async_status = status.clone();
        let ctx = self.v.egui_ctx.clone().unwrap_or_default();

        let client = self.v.psn_client.clone().expect("psn_client built in VolatileData::default");
        let _guard = self
            .v
            .rt_handle
            .as_ref()
            .expect("rt_handle set in UpdatesApp::new before any UI code runs")
            .enter();

        let promise = Promise::spawn_async(async move {
            let download_fut = fw.start_download(tx.clone(), download_path, cancel_for_task, &client);

            let progress_fut = async {
                while let Some(status_msg) = rx.recv().await {
                    let mut locked = async_status.lock().unwrap();
                    if let DownloadStatus::Progress(bytes) = &status_msg {
                        locked.record_progress(*bytes, 1000);
                    }
                    locked.last_received_status = status_msg;
                    drop(locked);
                    ctx.request_repaint();
                }
            };

            tokio::select! {
                result = download_fut => result,
                _ = progress_fut => Ok(()),
            }
        });

        self.v.firmware_download = Some(ActiveFirmwareDownload {
            name,
            size,
            progress_status: status,
            cancel_signal,
            promise,
        });
    }

    fn handle_firmware_promises(&mut self, toasts: &mut Vec<(String, ToastLevel)>) {
        // Handle firmware fetch promise.
        if let Some(ref promise) = self.v.firmware_fetch_promise {
            if promise.ready().is_some() {
                let promise = self.v.firmware_fetch_promise.take().unwrap();
                match promise.block_and_take() {
                    Ok(entries) => {
                        info!("Firmware fetch returned {} entries", entries.len());
                        self.v.firmware_results = entries;
                    }
                    Err(e) => {
                        toasts.push((format!("Firmware check failed: {e}"), ToastLevel::Error));
                    }
                }
            }
        }

        // Handle firmware download promise.
        if let Some(ref dl) = self.v.firmware_download {
            if dl.promise.ready().is_some() {
                let dl = self.v.firmware_download.take().unwrap();
                match dl.promise.block_and_take() {
                    Ok(()) => {
                        toasts.push((format!("{} downloaded successfully!", dl.name), ToastLevel::Success));
                    }
                    Err(DownloadError::Cancelled { .. }) => {
                        toasts.push((format!("{} download cancelled.", dl.name), ToastLevel::Info));
                    }
                    Err(e) => {
                        error!("Firmware download failed: {e:?}");
                        toasts.push((format!("Firmware download failed: {e:?}"), ToastLevel::Error));
                    }
                }
            }
        }
    }

    fn handle_rpcs3_install_promises(&mut self, toasts: &mut Vec<(String, ToastLevel)>) {
        let mut i = 0;
        while i < self.v.rpcs3_install_promises.len() {
            if self.v.rpcs3_install_promises[i].2.ready().is_some() {
                let (tid, pid, promise) = self.v.rpcs3_install_promises.remove(i);
                match promise.block_and_take() {
                    Ok(()) => {
                        toasts.push((format!("Installed {tid} to RPCS3"), ToastLevel::Success));
                        self.v.rpcs3_installed_pkgs.insert((tid, pid));
                    }
                    Err(e) => {
                        toasts.push((format!("RPCS3 install failed: {e}"), ToastLevel::Error));
                    }
                }
            } else {
                i += 1;
            }
        }
    }

    /// Build a cache of all download statuses. Called once per frame to avoid
    /// repeated linear scans of the download/completed/failed/paused/pending lists.
    fn build_download_status_cache(&self) -> HashMap<(String, String), CachedDownloadInfo> {
        let capacity = self.v.download_queue.len()
            + self.v.completed_downloads.len()
            + self.v.failed_downloads.len()
            + self.v.paused_downloads.len()
            + self.v.pending_downloads.len();
        let mut cache = HashMap::with_capacity(capacity);

        // Active downloads (with speed/progress data).
        for dl in &self.v.download_queue {
            let status_guard = dl.progress_status.lock().unwrap();
            let status = match status_guard.last_received_status {
                DownloadStatus::Resuming(offset) => {
                    ActiveDownloadStatus::Resuming(status_guard.progress as f32 / dl.size.max(1) as f32, offset)
                }
                DownloadStatus::Progress(_) => {
                    ActiveDownloadStatus::Downloading(status_guard.progress as f32 / dl.size.max(1) as f32)
                }
                DownloadStatus::Verifying => ActiveDownloadStatus::Verifying,
                _ => ActiveDownloadStatus::NotStarted,
            };
            cache.insert(
                (dl.title_id.clone(), dl.pkg_id.clone()),
                CachedDownloadInfo {
                    status,
                    speed_bps: status_guard.speed_bps,
                    progress_bytes: status_guard.progress,
                    total_bytes: dl.size,
                },
            );
        }

        for (title_id, pkg_id) in &self.v.completed_downloads {
            cache.insert(
                (title_id.clone(), pkg_id.clone()),
                CachedDownloadInfo {
                    status: ActiveDownloadStatus::Completed,
                    ..CachedDownloadInfo::default()
                },
            );
        }
        for f in &self.v.failed_downloads {
            cache.insert(
                (f.title_id.clone(), f.pkg_id.clone()),
                CachedDownloadInfo {
                    status: ActiveDownloadStatus::Failed,
                    ..CachedDownloadInfo::default()
                },
            );
        }
        for p in &self.v.paused_downloads {
            cache.insert(
                (p.title_id.clone(), p.pkg_id.clone()),
                CachedDownloadInfo {
                    status: ActiveDownloadStatus::Paused,
                    ..CachedDownloadInfo::default()
                },
            );
        }
        for p in &self.v.pending_downloads {
            cache.insert(
                (p.title_id.clone(), p.pkg.id()),
                CachedDownloadInfo {
                    status: ActiveDownloadStatus::Pending,
                    ..CachedDownloadInfo::default()
                },
            );
        }

        cache
    }

    /// Look up a package's cached download status. O(1) per call.
    fn cached_download_info(&self, title_id: &str, pkg: &PackageInfo) -> CachedDownloadInfo {
        self.v
            .download_status_cache
            .get(&(title_id.to_string(), pkg.id()))
            .copied()
            .unwrap_or_default()
    }

    fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            if i.modifiers.command && i.key_pressed(egui::Key::F) {
                self.v.focus_search_bar = true;
            }
            if i.key_pressed(egui::Key::Escape) {
                self.v.show_settings_window = false;
                self.v.show_mismatch_warning_window = false;
                self.v.show_about_window = false;
                self.v.show_rpcs3_library_window = false;
                self.v.show_firmware_window = false;
                self.v.show_migration_dialog = false;
                self.v.show_download_history_window = false;
                self.v.show_content_browser_window = false;
            }
            if i.modifiers.command && i.key_pressed(egui::Key::S) && !self.v.show_settings_window {
                self.v.modified_settings = self.settings.clone();
                self.v.show_settings_window = true;
            }
        });
    }

    fn draw_download_history_window(&mut self, ctx: &egui::Context) {
        let mut show = self.v.show_download_history_window;
        egui::Window::new("Download History")
            .open(&mut show)
            .resizable(true)
            .default_size([550.0, 350.0])
            .show(ctx, |ui| {
                if self.v.download_history.is_empty() {
                    ui.label("No downloads completed this session.");
                    return;
                }
                ui.horizontal(|ui| {
                    ui.label(format!("{} download(s) completed", self.v.download_history.len()));
                    if ui.button("Clear History").clicked() {
                        self.v.download_history.clear();
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Grid::new("history_grid")
                        .striped(true)
                        .min_col_width(60.0)
                        .show(ui, |ui| {
                            ui.strong("Serial");
                            ui.strong("Version");
                            ui.strong("Title");
                            ui.strong("Size");
                            ui.strong("Time");
                            ui.end_row();

                            for entry in self.v.download_history.iter().rev() {
                                ui.label(&entry.title_id);
                                ui.label(&entry.pkg_id);
                                ui.label(&entry.title);
                                ui.label(ByteSize::b(entry.size).to_string());
                                let secs = entry.completed_at.elapsed().as_secs();
                                let time_str = if secs < 60 {
                                    format!("{secs}s ago")
                                } else if secs < 3600 {
                                    format!("{}m ago", secs / 60)
                                } else {
                                    format!("{}h {}m ago", secs / 3600, (secs % 3600) / 60)
                                };
                                ui.label(time_str);
                                ui.end_row();
                            }
                        });
                });
            });
        if !show {
            self.v.show_download_history_window = false;
        }
    }

    fn draw_content_browser_window(&mut self, ctx: &egui::Context) {
        let mut show = self.v.show_content_browser_window;
        egui::Window::new("Content Browser")
            .open(&mut show)
            .resizable(true)
            .default_size([650.0, 500.0])
            .show(ctx, |ui| {
                if self.settings.content_directory.is_none() {
                    ui.label("No content directory configured. Set it in Settings > Content Library.");
                    if ui.button("Open Settings").clicked() {
                        self.v.modified_settings = self.settings.clone();
                        self.v.show_settings_window = true;
                    }
                    return;
                }

                let has_rpcs3 = self.settings.rpcs3_executable.is_some();

                // Toolbar
                ui.horizontal(|ui| {
                    ui.label("Filter:");
                    ui.text_edit_singleline(&mut self.v.content_filter);
                    if ui.button("Rescan").clicked() {
                        if let Some(ref dir) = self.settings.content_directory {
                            self.v.content_library = Some(crate::content::scan_content_directory(dir));
                        }
                    }
                });
                ui.separator();

                let Some(ref library) = self.v.content_library else {
                    ui.label("Loading...");
                    return;
                };

                let filter_lower = self.v.content_filter.to_lowercase();
                let filtered_games: Vec<usize> = library
                    .games
                    .iter()
                    .enumerate()
                    .filter(|(_, g)| filter_lower.is_empty() || g.name.to_lowercase().contains(&filter_lower))
                    .map(|(i, _)| i)
                    .collect();

                ui.label(format!("{} game(s) shown (of {})", filtered_games.len(), library.games.len()));
                ui.add_space(3.0);

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for &game_idx in &filtered_games {
                        let game = &library.games[game_idx];
                        let id = egui::Id::new(("content_game", game_idx));
                        egui::CollapsingHeader::new(egui::RichText::new(&game.name).strong())
                            .id_salt(id)
                            .show(ui, |ui| {
                                // Patches section
                                if !game.patches.is_empty() {
                                    ui.label(egui::RichText::new(format!("Patches ({})", game.patches.len())).underline());
                                    for patch in &game.patches {
                                        ui.horizontal(|ui| {
                                            ui.label(format!("  {} / {}", patch.serial, patch.pkg_filename));
                                            let patch_key = patch.pkg_path.to_string_lossy().to_string();
                                            let is_installed = self.v.content_installed_items.contains(&patch_key);
                                            let is_installing =
                                                self.v.content_install_promises.iter().any(|(k, _)| *k == patch_key);

                                            if is_installed {
                                                ui.label(
                                                    egui::RichText::new("Installed").color(egui::Rgba::from_rgb(0.0, 1.0, 0.0)),
                                                );
                                            } else if is_installing {
                                                ui.label(
                                                    egui::RichText::new("Installing...")
                                                        .color(egui::Rgba::from_rgb(1.0, 0.8, 0.0)),
                                                );
                                            } else if has_rpcs3 && ui.small_button("Install").clicked() {
                                                let exe = self.settings.rpcs3_executable.clone().unwrap();
                                                let pkg = patch.pkg_path.clone();
                                                let key = patch_key.clone();
                                                let promise = Promise::spawn_async(async move {
                                                    crate::rpcs3::install_pkg(&exe, &pkg)
                                                        .await
                                                        .map_err(|e| crate::content::ContentError::InstallFailed(e.to_string()))
                                                });
                                                self.v.content_install_promises.push((key, promise));
                                            }
                                        });
                                    }
                                    ui.add_space(3.0);
                                }

                                // DLC section
                                if !game.dlc.is_empty() {
                                    ui.label(egui::RichText::new(format!("DLC ({})", game.dlc.len())).underline());
                                    for dlc_item in &game.dlc {
                                        ui.horizontal(|ui| {
                                            ui.label(format!("  {} ({})", dlc_item.zip_filename, ByteSize::b(dlc_item.size)));
                                            let dlc_key = dlc_item.zip_path.to_string_lossy().to_string();
                                            let is_installed = self.v.content_installed_items.contains(&dlc_key);
                                            let is_installing =
                                                self.v.content_install_promises.iter().any(|(k, _)| *k == dlc_key);

                                            if is_installed {
                                                ui.label(
                                                    egui::RichText::new("Installed").color(egui::Rgba::from_rgb(0.0, 1.0, 0.0)),
                                                );
                                            } else if is_installing {
                                                ui.label(
                                                    egui::RichText::new("Installing...")
                                                        .color(egui::Rgba::from_rgb(1.0, 0.8, 0.0)),
                                                );
                                            } else if has_rpcs3 && ui.small_button("Install").clicked() {
                                                let exe = self.settings.rpcs3_executable.clone().unwrap();
                                                let zip = dlc_item.zip_path.clone();
                                                let key = dlc_key.clone();
                                                // Resolve exdata directory from RPCS3 config
                                                let exdata = self
                                                    .settings
                                                    .rpcs3_config_dir
                                                    .as_ref()
                                                    .map(|d| d.join("dev_hdd0/home/00000001/exdata"))
                                                    .unwrap_or_default();
                                                let promise = Promise::spawn_async(async move {
                                                    crate::content::install_dlc_zip(&exe, &zip, &exdata).await
                                                });
                                                self.v.content_install_promises.push((key, promise));
                                            }
                                        });
                                    }
                                }
                            });
                    }
                });
            });
        if !show {
            self.v.show_content_browser_window = false;
        }
    }

    fn handle_content_install_promises(&mut self, toasts: &mut Vec<(String, ToastLevel)>) {
        let mut i = 0;
        while i < self.v.content_install_promises.len() {
            if self.v.content_install_promises[i].1.ready().is_some() {
                let (key, promise) = self.v.content_install_promises.remove(i);
                match promise.block_and_take() {
                    Ok(()) => {
                        // Extract a short display name from the key (file path)
                        let display = Path::new(&key)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| key.clone());
                        toasts.push((format!("Installed: {display}"), ToastLevel::Success));
                        self.v.content_installed_items.insert(key);
                    }
                    Err(e) => {
                        toasts.push((format!("Install failed: {e}"), ToastLevel::Error));
                    }
                }
            } else {
                i += 1;
            }
        }
    }

    /// Queue a download, respecting the max concurrent downloads setting.
    /// If all slots are full, the download is added to the pending queue.
    fn queue_download(&mut self, title_id: String, title: String, pkg: PackageInfo, ui: &egui::Ui) {
        if self.v.download_queue.len() < self.settings.max_concurrent_downloads as usize {
            let active = self.start_download(title_id, title, pkg, ui);
            self.v.download_queue.push(active);
        } else {
            self.v.pending_downloads.push_back(PendingDownload { title_id, title, pkg });
        }
    }

    /// Queue a download without a UI reference (for "Download All" and pending promotion).
    fn queue_download_no_ui(&mut self, title_id: String, title: String, pkg: PackageInfo) {
        if self.v.download_queue.len() < self.settings.max_concurrent_downloads as usize {
            let active = self.start_download_with_retry(title_id, title, pkg, 0);
            self.v.download_queue.push(active);
        } else {
            self.v.pending_downloads.push_back(PendingDownload { title_id, title, pkg });
        }
    }

    fn start_download(&self, serial: String, title: String, pkg: PackageInfo, ui: &egui::Ui) -> ActiveDownload {
        self.start_download_inner(serial, title, pkg, 0, ui.ctx().clone())
    }

    /// Re-queue a download with an incremented retry count (used for auto-retry on short transfers).
    /// Uses the cached egui context instead of requiring a UI reference.
    fn start_download_with_retry(&self, serial: String, title: String, pkg: PackageInfo, retry_count: u32) -> ActiveDownload {
        // Use the cached real context so that request_repaint() actually triggers UI updates.
        // Falls back to a default context only if the cache was somehow never initialized.
        let ctx = self.v.egui_ctx.clone().unwrap_or_default();
        self.start_download_inner(serial, title, pkg, retry_count, ctx)
    }

    fn start_download_inner(
        &self,
        serial: String,
        title: String,
        pkg: PackageInfo,
        retry_count: u32,
        ctx: egui::Context,
    ) -> ActiveDownload {
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let id = serial.clone();
        let pkg_id = pkg.id();
        let download_size = pkg.size;
        let download_path = self.settings.pkg_download_path.clone();
        let dir_format = self.settings.directory_format;
        let pkg_for_task = pkg.clone();
        let title_for_task = title.clone();

        let status = Arc::new(std::sync::Mutex::new(DownloadProgressStatus {
            progress: 0,
            last_received_status: DownloadStatus::Verifying,
            speed_sample_bytes: 0,
            speed_sample_time: std::time::Instant::now(),
            speed_bps: 0.0,
        }));
        let async_status = status.clone();

        let cancel_signal = Arc::new(AtomicU8::new(CANCEL_NONE));
        let cancel_for_task = cancel_signal.clone();

        let dl_client = self.v.psn_client.clone().expect("psn_client built in VolatileData::default");
        let _guard = self
            .v
            .rt_handle
            .as_ref()
            .expect("rt_handle set in UpdatesApp::new before any UI code runs")
            .enter();
        let download_promise = Promise::spawn_async(async move {
            let download_fut = pkg_for_task.start_download(
                tx.clone(),
                download_path.clone(),
                serial.clone(),
                title_for_task.clone(),
                dir_format,
                cancel_for_task,
                &dl_client,
            );
            tokio::pin!(download_fut);
            loop {
                select! {
                    result = &mut download_fut => {
                        ctx.request_repaint();
                        return result
                    }
                    progress = rx.recv() => {
                        if let Some(progress_status) = progress {
                            let mut guard = async_status.lock().unwrap();
                            let download = guard.deref_mut();
                            if let DownloadStatus::Progress(progress) = progress_status {
                                download.record_progress(progress, 500);
                            }

                            download.last_received_status = progress_status;
                            ctx.request_repaint();
                        }
                    }
                }
            }
        });

        ActiveDownload {
            title_id: id,
            pkg_id,
            title,
            pkg,
            retry_count,

            size: download_size,
            progress_status: status,
            cancel_signal,

            promise: download_promise,
        }
    }

    fn start_merge_parts(&self, update_info: UpdateInfo, ui: &egui::Ui) -> ActiveMerge {
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let download_path = self.settings.pkg_download_path.clone();
        let title_id = update_info.title_id.clone();
        let dir_format = self.settings.directory_format;

        let status = Arc::new(std::sync::Mutex::new(MergeProgressStatus {
            progress: 0,
            last_received_status: MergeStatus::PartProgress(0),
        }));
        let ctx = ui.ctx().clone();
        let async_status = status.clone();

        let _guard = self
            .v
            .rt_handle
            .as_ref()
            .expect("rt_handle set in UpdatesApp::new before any UI code runs")
            .enter();
        let merge_promise = Promise::spawn_async(async move {
            let merge_fut = update_info.merge_parts(tx, &download_path, dir_format);
            tokio::pin!(merge_fut);
            loop {
                select! {
                    result = &mut merge_fut => {
                        ctx.request_repaint();
                        return result
                    },
                    progress = rx.recv() => {
                        let mut guard = async_status.lock().unwrap();
                        let merge = guard.deref_mut();
                        if let Some(status) = progress {
                            if let MergeStatus::PartProgress(progress) = status {
                                merge.progress = progress;
                            }

                            merge.last_received_status = status;
                            ctx.request_repaint();
                        }
                    }
                };
            }
        });

        ActiveMerge {
            title_id,

            progress_status: status,

            promise: merge_promise,
        }
    }

    fn show_notifications<S: Into<String>>(&mut self, msg: S, level: ToastLevel) {
        let msg = msg.into();

        if self.settings.show_toasts {
            let mut toast = Toast::basic(&msg);
            toast.level(level);
            toast.duration(Some(Duration::from_secs(10)));

            self.v.toasts.add(toast);
        } else {
            info!("A toast was supposed to be showed, but they are disabled.")
        }

        if self.settings.show_notifications {
            let mut notification = Notification::new();
            notification.summary("rusty-psn");
            notification.body(&msg);

            if let Err(e) = notification.show() {
                error!("Failed to show system notification: {e}");
            }
        } else {
            info!("System notifications are disabled in settings, not showing.")
        }
    }

    fn draw_search_bar(&mut self, ui: &mut egui::Ui) {
        // Row 1: Serial search input.
        ui.horizontal(|ui| {
            ui.label("Title Serial:");

            let serial_input = ui.text_edit_singleline(&mut self.v.serial_query);
            if self.v.focus_search_bar {
                serial_input.request_focus();
                self.v.focus_search_bar = false;
            }
            let input_submitted = serial_input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

            serial_input.context_menu(|ui| {
                ui.add_enabled_ui(self.v.clipboard.is_some(), |ui| {
                    if let Some(clip_ctx) = self.v.clipboard.as_mut() {
                        if ui.button("Paste").clicked() {
                            match clip_ctx.get_contents() {
                                Ok(contents) => self.v.serial_query.push_str(&contents),
                                Err(e) => {
                                    warn!("Failed to paste clipboard contents: {e}")
                                }
                            }

                            ui.close();
                        }

                        ui.add_enabled_ui(!self.v.serial_query.is_empty(), |ui| {
                            if ui.button("Clear").clicked() {
                                self.v.serial_query = String::new();
                                ui.close();
                            }
                        });
                    }
                });
            });

            ui.separator();

            ui.add_enabled_ui(!self.v.serial_query.is_empty() && self.v.search_promise.is_none(), |ui| {
                if !input_submitted && !ui.button("Search for updates").clicked() {
                    return;
                }

                let already_searched = self
                    .v
                    .update_results
                    .iter()
                    .any(|e| e.title_id == parse_title_id(&self.v.serial_query));
                if already_searched {
                    self.show_notifications("Provided title id results already shown", ToastLevel::Info);
                    return;
                }

                info!("Fetching updates for '{}'", self.v.serial_query);

                let _guard = self
                    .v
                    .rt_handle
                    .as_ref()
                    .expect("rt_handle set in UpdatesApp::new before any UI code runs")
                    .enter();
                let serial_query = self.v.serial_query.clone();
                let ctx = ui.ctx().clone();
                let c = self.v.psn_client.clone().expect("psn_client built in VolatileData::default");
                let promise = Promise::spawn_async(async move {
                    let result = UpdateInfo::get_info(serial_query, &c).await;
                    ctx.request_repaint();
                    result
                });

                self.v.search_promise = Some(promise);
            });

            if ui.button("⚙").clicked() {
                self.v.modified_settings = self.settings.clone();
                self.v.show_settings_window = true;
            }

            // Batch search progress indicator (inline in search bar).
            let batch_active = self.v.batch_search_total > 0
                && (self.v.batch_search_completed < self.v.batch_search_total || !self.v.batch_search_active.is_empty());
            if batch_active {
                ui.separator();
                let done = self.v.batch_search_completed;
                let total = self.v.batch_search_total;
                let fraction = done as f32 / total.max(1) as f32;
                ui.add(
                    egui::ProgressBar::new(fraction)
                        .text(format!("Searching {done}/{total}..."))
                        .desired_width(120.0)
                        .animate(true),
                );
            }
        });

        // Row 2: Action buttons — always visible.
        ui.horizontal(|ui| {
            if ui.button("Import Batch...").clicked() {
                self.trigger_batch_import();
            }

            if ui.button("Scan RPCS3 Library").clicked() {
                if self.settings.rpcs3_config_dir.is_some() {
                    self.trigger_rpcs3_scan();
                } else {
                    // Try auto-detect first.
                    if let Some(installation) = crate::rpcs3::auto_detect::detect_rpcs3() {
                        self.settings.rpcs3_config_dir = Some(installation.config_dir);
                        self.trigger_rpcs3_scan();
                        self.show_notifications("RPCS3 auto-detected and scan started.", ToastLevel::Info);
                    } else {
                        self.show_notifications(
                            "RPCS3 not found. Go to Settings (⚙) to configure the RPCS3 path.",
                            ToastLevel::Warning,
                        );
                    }
                }
            }

            if ui.button("Download Firmware").clicked() {
                self.v.show_firmware_window = true;
            }

            ui.separator();

            ui.add_enabled_ui(!self.v.update_results.is_empty(), |ui| {
                if ui.button("Download All").clicked() {
                    info!("Downloading all updates for all results");
                    for update in self.v.update_results.clone().iter() {
                        let title = update.title();
                        for pkg in &update.packages {
                            if self.get_active_download(&update.title_id, pkg).is_none() {
                                self.queue_download_no_ui(update.title_id.clone(), title.clone(), pkg.clone());
                            }
                        }
                    }
                }

                if ui.button("Download All Latest").clicked() {
                    info!("Downloading latest updates for all results");
                    for update in self.v.update_results.clone().iter() {
                        if let Some(latest_pkg) = update.packages.last() {
                            if self.get_active_download(&update.title_id, latest_pkg).is_none() {
                                self.queue_download_no_ui(update.title_id.clone(), update.title(), latest_pkg.clone());
                            }
                        }
                    }
                }

                if ui.button("Clear results").clicked() {
                    self.v.update_results = Vec::new();
                }
            });

            if ui.button("Open Downloads").clicked() {
                open_folder(&self.settings.pkg_download_path);
            }
        });
    }

    fn draw_results_list(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        // Build filtered/sorted index list up front so the toolbar can show "showing X of Y".
        let filter_lower = self.v.results_filter.to_lowercase();
        let mut indices: Vec<usize> = (0..self.v.update_results.len())
            .filter(|&i| {
                if filter_lower.is_empty() {
                    return true;
                }
                let u = &self.v.update_results[i];
                u.title_id.to_lowercase().contains(&filter_lower) || u.title().to_lowercase().contains(&filter_lower)
            })
            .collect();

        let sort_by = self.v.results_sort_by;
        let sort_dir = self.v.results_sort_direction;
        indices.sort_by(|&a, &b| {
            let ua = &self.v.update_results[a];
            let ub = &self.v.update_results[b];
            let ord = match sort_by {
                ResultSortBy::SearchOrder => a.cmp(&b),
                ResultSortBy::Serial => ua.title_id.cmp(&ub.title_id),
                ResultSortBy::Title => ua.title().cmp(&ub.title()),
                ResultSortBy::UpdateCount => ua.packages.len().cmp(&ub.packages.len()),
                ResultSortBy::TotalSize => {
                    let sa: u64 = ua.packages.iter().map(|p| p.size).sum();
                    let sb: u64 = ub.packages.iter().map(|p| p.size).sum();
                    sa.cmp(&sb)
                }
            };
            if sort_dir == SortDirection::Descending {
                ord.reverse()
            } else {
                ord
            }
        });

        // Sort + filter toolbar with a clearer visual hierarchy than a flat row of labels.
        if !self.v.update_results.is_empty() {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;

                ui.label(egui::RichText::new("Filter:").strong());
                let filter_resp = ui.add(
                    egui::TextEdit::singleline(&mut self.v.results_filter)
                        .desired_width(200.0)
                        .hint_text("Serial or title…"),
                );
                filter_resp.on_hover_text("Show only results whose serial or title contains this text");
                if !self.v.results_filter.is_empty() && ui.small_button("\u{2715}").on_hover_text("Clear filter").clicked() {
                    self.v.results_filter.clear();
                }

                ui.separator();

                ui.label(egui::RichText::new("Sort by:").strong());
                egui::ComboBox::from_id_salt("results_sort")
                    .selected_text(self.v.results_sort_by.label())
                    .width(140.0)
                    .show_ui(ui, |ui| {
                        for opt in [
                            ResultSortBy::SearchOrder,
                            ResultSortBy::Serial,
                            ResultSortBy::Title,
                            ResultSortBy::UpdateCount,
                            ResultSortBy::TotalSize,
                        ] {
                            ui.selectable_value(&mut self.v.results_sort_by, opt, opt.label());
                        }
                    });

                let (arrow, hover) = match self.v.results_sort_direction {
                    SortDirection::Ascending => ("\u{2191}", "Ascending — click to flip to descending"),
                    SortDirection::Descending => ("\u{2193}", "Descending — click to flip to ascending"),
                };
                if ui
                    .add(egui::Button::new(egui::RichText::new(arrow).strong().size(15.0)))
                    .on_hover_text(hover)
                    .clicked()
                {
                    self.v.results_sort_direction = match self.v.results_sort_direction {
                        SortDirection::Ascending => SortDirection::Descending,
                        SortDirection::Descending => SortDirection::Ascending,
                    };
                }

                // Result count, right-aligned.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let total = self.v.update_results.len();
                    let visible = indices.len();
                    let text = if visible == total {
                        format!("{total} result{}", if total == 1 { "" } else { "s" })
                    } else {
                        format!("Showing {visible} of {total}")
                    };
                    ui.label(egui::RichText::new(text).weak());
                });
            });
            ui.add_space(2.0);
            ui.separator();
        }

        let result_count = self.v.update_results.len();
        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            if indices.is_empty() && !self.v.update_results.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("No results match the current filter.").weak());
                });
                return;
            }
            for &i in &indices {
                let update = self.v.update_results[i].clone();
                self.draw_result_entry(ctx, ui, &update, result_count);
            }
        });
    }

    fn draw_result_entry(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, update: &UpdateInfo, result_count: usize) {
        let total_updates_size = update.packages.iter().map(|pkg| pkg.size).sum::<u64>();

        let title_id = &update.title_id;
        let update_count = update.packages.len();
        let platform_variant = update.platform_variant;

        // Aggregate per-package status counts for the header status pill.
        let completed_count = update
            .packages
            .iter()
            .filter(|pkg| self.cached_download_info(title_id, pkg).status == ActiveDownloadStatus::Completed)
            .count();
        let failed_count = update
            .packages
            .iter()
            .filter(|pkg| self.cached_download_info(title_id, pkg).status == ActiveDownloadStatus::Failed)
            .count();
        let paused_count = update
            .packages
            .iter()
            .filter(|pkg| self.cached_download_info(title_id, pkg).status == ActiveDownloadStatus::Paused)
            .count();
        let in_progress_count = update
            .packages
            .iter()
            .filter(|pkg| {
                matches!(
                    self.cached_download_info(title_id, pkg).status,
                    ActiveDownloadStatus::Downloading(_) | ActiveDownloadStatus::Resuming(_, _) | ActiveDownloadStatus::Verifying
                )
            })
            .count();

        let id = egui::Id::new(format!("pkg_header_{title_id}"));

        egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, result_count == 1)
            .show_header(ui, |ui| {
                let title = update.title();

                // Left side: platform badge + title (or serial fallback) + metadata strip.
                platform_badge(ui, platform_variant);
                ui.add_space(4.0);

                if !title.is_empty() {
                    ui.label(egui::RichText::new(&title).strong().size(14.0));
                    ui.label(egui::RichText::new("\u{00B7}").weak());
                }
                ui.label(egui::RichText::new(title_id).monospace().weak());
                ui.label(egui::RichText::new("\u{00B7}").weak());
                ui.label(
                    egui::RichText::new(format!("{update_count} update{}", if update_count == 1 { "" } else { "s" })).weak(),
                );
                ui.label(egui::RichText::new("\u{00B7}").weak());
                ui.label(egui::RichText::new(format!("{}", ByteSize::b(total_updates_size))).weak());

                // Right side: actions and aggregate status pill, packed right-to-left so the
                // primary action sits at the rightmost edge in the conventional position.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // PS4 multipart merge controls (rightmost when applicable).
                    if platform_variant == utils::PlatformVariant::PS4 {
                        let is_multipart = update.packages.len() > 1;
                        let all_pkgs_completed = update
                            .packages
                            .iter()
                            .all(|pkg| self.cached_download_info(title_id, pkg).status == ActiveDownloadStatus::Completed);
                        let is_mergable = is_multipart && all_pkgs_completed;
                        let hover_text = if is_multipart {
                            "All parts need to be completed for merge to be available"
                        } else {
                            "This PS4 update is not a multipart update"
                        };

                        let merge_status = self.title_merge_status(update);
                        match merge_status {
                            ActiveMergeStatus::Merging(progress) => {
                                let width = ui.available_width().min(150.0);
                                ui.add(egui::ProgressBar::new(progress).desired_width(width).show_percentage());
                                status_pill(ui, "Merging parts…", STATUS_COLOR_PAUSED);
                            }
                            ActiveMergeStatus::Merged => {
                                status_pill(ui, "Parts merged", STATUS_COLOR_DONE);
                            }
                            ActiveMergeStatus::Failed => {
                                status_pill(ui, "Parts merge failed", STATUS_COLOR_FAILED);
                            }
                            _ => {}
                        }

                        let merge_btn = ui
                            .add_enabled(is_mergable, egui::Button::new("Merge parts"))
                            .on_disabled_hover_text(hover_text);
                        if merge_btn.clicked() {
                            self.v.merge_queue.push(self.start_merge_parts(update.clone(), ui));
                        }
                    }

                    // Primary action: download just the latest package.
                    if let Some(latest_pkg) = update.packages.last() {
                        let resp = ui
                            .button("Download latest")
                            .on_hover_text("Queue only the most recent update");
                        if resp.clicked() && self.get_active_download(title_id, latest_pkg).is_none() {
                            info!("Downloading latest update {} for serial {title_id}", latest_pkg.id());
                            self.queue_download(title_id.to_string(), title.clone(), latest_pkg.clone(), ui);
                        }
                    }

                    // Secondary action: download every update.
                    let resp = ui.button("Download all").on_hover_text("Queue every update for this title");
                    if resp.clicked() {
                        info!("Downloading all updates for serial {title_id} ({update_count})");
                        for pkg in update.packages.iter() {
                            if self.get_active_download(title_id, pkg).is_none() {
                                info!("Downloading update {} for serial {title_id} (group)", pkg.id());
                                self.queue_download(title_id.to_string(), title.clone(), pkg.clone(), ui);
                            }
                        }
                    }

                    ui.add_space(6.0);

                    // Aggregate status pill (rendered to the left of the action buttons).
                    if completed_count == update_count {
                        status_pill(ui, "All downloaded", STATUS_COLOR_DONE);
                    } else if failed_count > 0 {
                        let text = if completed_count > 0 {
                            format!("{completed_count}/{update_count} done · {failed_count} failed")
                        } else {
                            format!("{failed_count} failed")
                        };
                        status_pill(ui, &text, STATUS_COLOR_FAILED);
                    } else if in_progress_count > 0 {
                        let text = if completed_count > 0 {
                            format!("{completed_count}/{update_count} done · {in_progress_count} active")
                        } else {
                            format!("{in_progress_count} downloading")
                        };
                        status_pill(ui, &text, STATUS_COLOR_ACTIVE);
                    } else if paused_count > 0 {
                        status_pill(ui, &format!("{paused_count} paused"), STATUS_COLOR_PAUSED);
                    } else if completed_count > 0 {
                        status_pill(ui, &format!("{completed_count}/{update_count} done"), STATUS_COLOR_DONE);
                    }
                });
            })
            .body(|ui| {
                ui.add_space(4.0);

                let pkg_count = update.packages.len();
                for (pkg_idx, pkg) in update.packages.iter().enumerate() {
                    let is_latest = pkg_idx == pkg_count - 1;
                    self.draw_entry_pkg(ui, pkg, title_id, update.title(), is_latest);
                    ui.add_space(4.0);
                }
            });

        ui.add_space(6.0);
    }

    fn draw_entry_pkg(&mut self, ui: &mut egui::Ui, pkg: &PackageInfo, title_id: &str, title: String, is_latest: bool) {
        ui.group(|ui| {
            let cached = self.cached_download_info(title_id, pkg);
            let download_status = cached.status;
            let is_active = matches!(
                download_status,
                ActiveDownloadStatus::Downloading(_) | ActiveDownloadStatus::Resuming(_, _) | ActiveDownloadStatus::Verifying
            );

            let mut pause_clicked = false;
            let mut abort_clicked = false;
            let mut download_clicked = false;

            // Row 1: identity (version + Latest badge) on the left; status + actions on the right.
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("v{}", pkg.id())).strong().size(14.0));
                if is_latest {
                    ui.label(
                        egui::RichText::new("\u{2605} Latest")
                            .color(LATEST_BADGE_COLOR)
                            .strong()
                            .small(),
                    );
                }
                ui.label(egui::RichText::new(format!("{}", ByteSize::b(pkg.size))).weak());
                if pkg.offset > 0 {
                    ui.label(egui::RichText::new(format!("\u{00B7} offset {}", pkg.offset)).weak().small());
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    match download_status {
                        ActiveDownloadStatus::NotStarted => {
                            if ui.button("Download").clicked() {
                                download_clicked = true;
                            }
                        }
                        ActiveDownloadStatus::Pending => {
                            if ui.button("Cancel").clicked() {
                                self.v
                                    .pending_downloads
                                    .retain(|p| !(p.title_id == title_id && p.pkg.id() == pkg.id()));
                            }
                            status_pill(ui, "Queued", STATUS_COLOR_QUEUED);
                        }
                        ActiveDownloadStatus::Downloading(_) | ActiveDownloadStatus::Resuming(_, _) => {
                            abort_clicked = ui.button("Abort").clicked();
                            pause_clicked = ui.button("Pause").clicked();
                            let label = match download_status {
                                ActiveDownloadStatus::Resuming(_, offset) => format!("Resuming from {}", ByteSize::b(offset)),
                                _ => "Downloading".to_string(),
                            };
                            status_pill(ui, &label, STATUS_COLOR_ACTIVE);
                        }
                        ActiveDownloadStatus::Verifying => {
                            status_pill(ui, "Verifying", STATUS_COLOR_PAUSED);
                        }
                        ActiveDownloadStatus::Paused => {
                            if ui.button("Abort").clicked() {
                                self.v
                                    .paused_downloads
                                    .retain(|p| !(p.title_id == title_id && p.pkg_id == pkg.id()));
                                let path = crate::utils::create_pkg_path(
                                    &self.settings.pkg_download_path,
                                    title_id,
                                    &title,
                                    self.settings.directory_format,
                                );
                                if path.exists() {
                                    if let Err(e) = std::fs::remove_dir_all(&path) {
                                        warn!("Failed to clean up paused download dir {}: {e}", path.display());
                                    }
                                }
                            }
                            if ui.button("Resume").clicked() {
                                self.v
                                    .paused_downloads
                                    .retain(|p| !(p.title_id == title_id && p.pkg_id == pkg.id()));
                                self.queue_download(title_id.to_string(), title.clone(), pkg.clone(), ui);
                            }
                            status_pill(ui, "Paused", STATUS_COLOR_PAUSED);
                        }
                        ActiveDownloadStatus::Completed => {
                            let install_key = (title_id.to_string(), pkg.id());
                            let is_installing = self
                                .v
                                .rpcs3_install_promises
                                .iter()
                                .any(|(tid, pid, _)| tid == title_id && *pid == pkg.id());
                            let is_installed = self.v.rpcs3_installed_pkgs.contains(&install_key);

                            if !is_installing && !is_installed {
                                if let Some(rpcs3_exe) = &self.settings.rpcs3_executable {
                                    if ui.button("Install to RPCS3").clicked() {
                                        let folder = crate::utils::create_pkg_path(
                                            &self.settings.pkg_download_path,
                                            title_id,
                                            &title,
                                            self.settings.directory_format,
                                        );
                                        if let Some(pkg_file) = find_pkg_in_dir(&folder) {
                                            let exe = rpcs3_exe.clone();
                                            let tid = title_id.to_string();
                                            let pid = pkg.id();
                                            let promise =
                                                Promise::spawn_async(
                                                    async move { crate::rpcs3::install_pkg(&exe, &pkg_file).await },
                                                );
                                            self.v.rpcs3_install_promises.push((tid, pid, promise));
                                        }
                                    }
                                }
                            }

                            if ui.button("Open folder").clicked() {
                                let folder = crate::utils::create_pkg_path(
                                    &self.settings.pkg_download_path,
                                    title_id,
                                    &title,
                                    self.settings.directory_format,
                                );
                                open_folder(&folder);
                            }

                            if is_installed {
                                status_pill(ui, "Installed in RPCS3", STATUS_COLOR_DONE);
                            } else if is_installing {
                                status_pill(ui, "Installing…", STATUS_COLOR_PAUSED);
                            } else {
                                status_pill(ui, "Completed", STATUS_COLOR_DONE);
                            }
                        }
                        ActiveDownloadStatus::Failed => {
                            if ui.button("Retry").clicked() {
                                self.v
                                    .failed_downloads
                                    .retain(|f| !(f.title_id == title_id && f.pkg_id == pkg.id()));
                                let download = self.start_download(title_id.to_string(), title.clone(), pkg.clone(), ui);
                                self.add_download(download);
                            }
                            status_pill(ui, "Failed", STATUS_COLOR_FAILED);
                        }
                    }

                    // Per-package merge status (only meaningful for PS4 multipart entries).
                    match self.pkg_merge_status(title_id, pkg) {
                        ActiveMergeStatus::NotMergable | ActiveMergeStatus::NotStarted => {}
                        ActiveMergeStatus::Failed => status_pill(ui, "Merge failed", STATUS_COLOR_FAILED),
                        ActiveMergeStatus::Merged => status_pill(ui, "Merged", STATUS_COLOR_DONE),
                        ActiveMergeStatus::Merging(_) => status_pill(ui, "Merging…", STATUS_COLOR_PAUSED),
                    }
                });
            });

            // Row 2: progress bar + speed/ETA — shown only while a transfer is in flight.
            if is_active {
                ui.horizontal(|ui| {
                    let progress = match download_status {
                        ActiveDownloadStatus::Downloading(p) | ActiveDownloadStatus::Resuming(p, _) => p,
                        _ => 0.0,
                    };
                    let width = (ui.available_width() - 220.0).max(120.0);
                    ui.add(egui::ProgressBar::new(progress).desired_width(width).show_percentage());
                    self.draw_speed_eta(ui, &cached);
                });
            }

            // Optional details — hidden by default to keep SHA-1 / URL noise out of the way for
            // typical use, but one click away for power users.
            egui::CollapsingHeader::new(egui::RichText::new("Details").weak())
                .id_salt(format!("pkg_details_{title_id}_{}", pkg.id()))
                .default_open(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("SHA-1:").weak());
                        ui.label(egui::RichText::new(&pkg.sha1sum).monospace().small());
                        if ui.small_button("Copy").clicked() {
                            if let Some(ref mut clip) = self.v.clipboard {
                                let _ = clip.set_contents(pkg.sha1sum.clone());
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Source URL:").weak());
                        if ui.small_button("Copy URL").clicked() {
                            if let Some(ref mut clip) = self.v.clipboard {
                                let _ = clip.set_contents(pkg.url.clone());
                            }
                        }
                    });
                });

            // Apply queued click effects after the row is drawn so the borrow checker stays happy
            // and so any spawned downloads don't read self.v while we're mid-render.
            if pause_clicked {
                if let Some(d) = self.get_active_download(title_id, pkg) {
                    d.cancel_signal.store(CANCEL_PAUSE, Ordering::Relaxed);
                }
            }
            if abort_clicked {
                if let Some(d) = self.get_active_download(title_id, pkg) {
                    d.cancel_signal.store(CANCEL_ABORT, Ordering::Relaxed);
                }
            }
            if download_clicked {
                info!("Downloading update {} for serial {} (individual)", pkg.version, title_id);
                self.queue_download(title_id.to_string(), title, pkg.clone(), ui);
            }
        });
    }

    /// Render per-download speed and ETA inline.
    fn draw_speed_eta(&self, ui: &mut egui::Ui, cached: &CachedDownloadInfo) {
        if cached.speed_bps > 0.0 {
            let remaining = cached.total_bytes.saturating_sub(cached.progress_bytes) as f64;
            let eta_secs = (remaining / cached.speed_bps) as u64;
            let eta_str = if eta_secs < 60 {
                format!("{eta_secs}s")
            } else if eta_secs < 3600 {
                format!("{}:{:02}", eta_secs / 60, eta_secs % 60)
            } else {
                format!("{}:{:02}:{:02}", eta_secs / 3600, (eta_secs % 3600) / 60, eta_secs % 60)
            };
            ui.label(format!("{}/s - ETA {eta_str}", ByteSize::b(cached.speed_bps as u64)));
        }
    }

    fn draw_settings_window(&mut self, ctx: &egui::Context) {
        let mut show_window = self.v.show_settings_window;
        let mut current_download_path = self.v.modified_settings.pkg_download_path.to_string_lossy().to_string();

        // Fixed size avoids a bug that makes the window gradually stretch itself vertically for some reason.
        // See https://github.com/RainbowCookie32/rusty-psn/issues/138
        egui::Window::new("Settings")
            .id(egui::Id::new("cfg_win"))
            .open(&mut show_window)
            .fixed_size([420.0, 700.0])
            .show(ctx, |ui| {
                ui.heading("Downloads");
                ui.add_space(3.0);

                ui.label("Download path:");
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(false, |ui| {
                        ui.text_edit_singleline(&mut current_download_path);
                    });

                    if ui.button("Pick folder").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.v.settings_dirty = true;
                            self.v.modified_settings.pkg_download_path = path;
                        }
                    }

                    if ui.button("Reset").clicked() {
                        self.v.settings_dirty = true;
                        self.v.modified_settings.pkg_download_path = default_download_path();
                    }
                });

                ui.add_space(5.0);

                ui.label("Folder structure:");
                let current_format = self.v.modified_settings.directory_format;
                egui::ComboBox::from_id_salt("dir_format")
                    .selected_text(current_format.example())
                    .width(280.0)
                    .show_ui(ui, |ui| {
                        use crate::utils::DirectoryFormat;
                        for format in [
                            DirectoryFormat::SerialOnly,
                            DirectoryFormat::SerialTitle,
                            DirectoryFormat::TitleSerial,
                        ] {
                            if ui
                                .selectable_value(&mut self.v.modified_settings.directory_format, format, format.example())
                                .changed()
                            {
                                self.v.settings_dirty = true;
                            }
                        }
                    });

                ui.add_space(5.0);

                ui.label("Maximum concurrent downloads:");
                if ui
                    .add(
                        egui::Slider::new(&mut self.v.modified_settings.max_concurrent_downloads, 1..=10)
                            .clamping(egui::SliderClamping::Always),
                    )
                    .changed()
                {
                    self.v.settings_dirty = true;
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(3.0);
                ui.heading("Appearance");
                ui.add_space(3.0);

                ui.label("Theme:");
                let current_theme = self.v.modified_settings.theme;
                egui::ComboBox::from_id_salt("theme_selector")
                    .selected_text(current_theme.label())
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for theme in [AppTheme::Dark, AppTheme::Light, AppTheme::PlayStation] {
                            if ui
                                .selectable_value(&mut self.v.modified_settings.theme, theme, theme.label())
                                .changed()
                            {
                                self.v.settings_dirty = true;
                                // Apply theme immediately for live preview.
                                self.v.modified_settings.theme.apply(ui.ctx());
                            }
                        }
                    });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(3.0);
                ui.heading("Notifications");
                ui.add_space(3.0);

                if ui
                    .checkbox(&mut self.v.modified_settings.show_toasts, "Show in-app toasts")
                    .changed()
                {
                    self.v.settings_dirty = true;
                }

                if ui
                    .checkbox(&mut self.v.modified_settings.show_notifications, "Show system notifications")
                    .changed()
                {
                    self.v.settings_dirty = true;
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(3.0);
                ui.heading("RPCS3 Integration");
                ui.add_space(3.0);

                ui.label("RPCS3 config directory:");
                ui.horizontal(|ui| {
                    let rpcs3_display = self
                        .v
                        .modified_settings
                        .rpcs3_config_dir
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Not configured".to_string());
                    let mut display_text = rpcs3_display;
                    ui.add_enabled_ui(false, |ui| {
                        ui.text_edit_singleline(&mut display_text);
                    });

                    if ui.button("Browse").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.v.settings_dirty = true;
                            self.v.modified_settings.rpcs3_config_dir = Some(path);
                        }
                    }

                    if ui.button("Auto-detect").clicked() {
                        if let Some(installation) = crate::rpcs3::auto_detect::detect_rpcs3() {
                            self.v.settings_dirty = true;
                            self.v.modified_settings.rpcs3_config_dir = Some(installation.config_dir);
                            if self.v.modified_settings.rpcs3_executable.is_none() {
                                self.v.modified_settings.rpcs3_executable = installation.executable;
                            }
                        } else {
                            self.show_notifications("RPCS3 not found. Please configure the path manually.", ToastLevel::Warning);
                        }
                    }
                });

                // Show status.
                if let Some(dir) = &self.v.modified_settings.rpcs3_config_dir {
                    if dir.join("games.yml").exists() {
                        ui.label(egui::RichText::new("Status: RPCS3 config detected").color(egui::Rgba::from_rgb(0.0, 1.0, 0.0)));
                    } else {
                        ui.label(
                            egui::RichText::new("Status: games.yml not found in this directory")
                                .color(egui::Rgba::from_rgb(1.0, 0.6, 0.0)),
                        );
                    }
                } else {
                    ui.label("Status: Not configured");
                }

                ui.add_space(5.0);
                ui.label("RPCS3 executable:");
                ui.horizontal(|ui| {
                    let exe_display = self
                        .v
                        .modified_settings
                        .rpcs3_executable
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Not configured (auto-detected on scan)".to_string());
                    let mut display_text = exe_display;
                    ui.add_enabled_ui(false, |ui| {
                        ui.text_edit_singleline(&mut display_text);
                    });

                    if ui.button("Browse").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_file() {
                            self.v.settings_dirty = true;
                            self.v.modified_settings.rpcs3_executable = Some(path);
                        }
                    }

                    if ui.button("Clear").clicked() {
                        self.v.settings_dirty = true;
                        self.v.modified_settings.rpcs3_executable = None;
                    }
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(3.0);
                ui.heading("Content Library");
                ui.add_space(3.0);

                ui.label("PS3 content directory (patches + DLC):");
                ui.horizontal(|ui| {
                    let content_display = self
                        .v
                        .modified_settings
                        .content_directory
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Not configured".to_string());
                    let mut display_text = content_display;
                    ui.add_enabled_ui(false, |ui| {
                        ui.text_edit_singleline(&mut display_text);
                    });

                    if ui.button("Browse").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.v.settings_dirty = true;
                            self.v.modified_settings.content_directory = Some(path);
                        }
                    }

                    if ui.button("Clear").clicked() {
                        self.v.settings_dirty = true;
                        self.v.modified_settings.content_directory = None;
                    }
                });

                ui.with_layout(egui::Layout::bottom_up(egui::Align::TOP), |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Save settings").clicked() {
                            self.v.settings_dirty = false;
                            self.v.show_settings_window = false;

                            self.settings = self.v.modified_settings.clone();
                        }

                        if ui
                            .add_enabled(self.v.settings_dirty, egui::Button::new("Discard changes"))
                            .clicked()
                        {
                            self.v.settings_dirty = false;
                            self.v.show_settings_window = false;

                            // Revert theme if it was changed during preview.
                            self.settings.theme.apply(ui.ctx());
                            self.v.modified_settings = self.settings.clone();
                        }

                        if ui.button("Restore to defaults").clicked() {
                            self.v.settings_dirty = false;
                            self.v.show_settings_window = false;

                            self.settings = AppSettings::default();
                            self.v.modified_settings = AppSettings::default();
                            self.settings.theme.apply(ui.ctx());
                        }
                    });

                    ui.separator();
                });
            });

        if !show_window {
            self.v.show_settings_window = false;
        }
    }

    fn draw_hash_mismatch_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("File integrity check failed").collapsible(false).fixed_size([550.0, 100.0]).show(ctx, | ui | {
            ui.vertical_centered(| ui | {
                ui.label(egui::RichText::new("The integrity check for a downloaded file failed.").color(egui::Color32::YELLOW).heading());
                ui.label(egui::RichText::new("Considering the file is smaller than expected, it's likely that Sony's servers are being unreliable.").strong());
                ui.label(egui::RichText::new("You should try to download the file again, or wait for a few hours before retrying. Sony's servers should eventually be able handle a complete download.").strong());

                ui.small("fix your shit already sony, it's been years of unreliable downloads.");
            });

            ui.separator();

            ui.vertical_centered(| ui | {
                if ui.button("Close message").clicked() {
                    self.v.show_mismatch_warning_window = false;
                }
            });
        });
    }

    fn draw_menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Settings...").clicked() {
                        self.v.modified_settings = self.settings.clone();
                        self.v.show_settings_window = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("Tools", |ui| {
                    if ui.button("Import Batch File...").clicked() {
                        ui.close();
                        self.trigger_batch_import();
                    }
                    if ui.button("Scan RPCS3 Library...").clicked() {
                        ui.close();
                        if self.settings.rpcs3_config_dir.is_some() {
                            self.trigger_rpcs3_scan();
                        } else if let Some(installation) = crate::rpcs3::auto_detect::detect_rpcs3() {
                            self.settings.rpcs3_config_dir = Some(installation.config_dir);
                            self.trigger_rpcs3_scan();
                            self.show_notifications("RPCS3 auto-detected and scan started.", ToastLevel::Info);
                        } else {
                            self.show_notifications(
                                "RPCS3 not found. Go to Settings to configure the RPCS3 path.",
                                ToastLevel::Warning,
                            );
                        }
                    }
                    if ui.button("Download Firmware...").clicked() {
                        self.v.show_firmware_window = true;
                        ui.close();
                    }
                    if ui.button("Download History...").clicked() {
                        self.v.show_download_history_window = true;
                        ui.close();
                    }
                    if ui.button("Content Browser...").clicked() {
                        self.v.show_content_browser_window = true;
                        // Scan content directory on open
                        if let Some(ref dir) = self.settings.content_directory {
                            self.v.content_library = Some(crate::content::scan_content_directory(dir));
                        }
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Migrate Downloads...").clicked() {
                        let preview = crate::utils::detect_migratable_directories(
                            &self.settings.pkg_download_path,
                            self.settings.directory_format,
                        );
                        self.v.migration_preview = preview;
                        self.v.show_migration_dialog = true;
                        ui.close();
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("About rusty-psn").clicked() {
                        self.v.show_about_window = true;
                        ui.close();
                    }
                });
            });
        });
    }

    fn draw_about_window(&mut self, ctx: &egui::Context) {
        let mut show_window = self.v.show_about_window;
        egui::Window::new("About rusty-psn")
            .open(&mut show_window)
            .collapsible(false)
            .resizable(false)
            .fixed_size([300.0, 120.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("rusty-psn");
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.add_space(5.0);
                    ui.label("Download PlayStation game updates for use with emulators.");
                    ui.add_space(5.0);
                    ui.hyperlink_to("GitHub Repository", "https://github.com/RainbowCookie32/rusty-psn");
                });
            });

        if !show_window {
            self.v.show_about_window = false;
        }
    }

    fn draw_migration_dialog(&mut self, ctx: &egui::Context) {
        // Check if a migration operation completed.
        if let Some(promise) = &self.v.migration_promise {
            if let Some(count) = promise.ready() {
                let msg = format!("Migration complete: {count} folder(s) migrated.");
                self.show_notifications(&msg, ToastLevel::Success);
                self.v.migration_promise = None;
                self.v.show_migration_dialog = false;
                return;
            }
        }

        let mut show = self.v.show_migration_dialog;
        egui::Window::new("Migrate Downloads")
            .open(&mut show)
            .collapsible(false)
            .resizable(true)
            .default_size([500.0, 300.0])
            .show(ctx, |ui| {
                if self.v.migration_promise.is_some() {
                    ui.label("Migration in progress...");
                    ui.spinner();
                    return;
                }

                if self.v.migration_preview.is_empty() {
                    ui.label("No folders found that need migration.");
                    ui.label(format!("Current format: {}", self.settings.directory_format));
                    return;
                }

                ui.label(format!(
                    "Found {} folder(s) to migrate to \"{}\" format:",
                    self.v.migration_preview.len(),
                    self.settings.directory_format
                ));
                ui.add_space(5.0);

                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    for (old, new) in &self.v.migration_preview {
                        let old_name = old.file_name().unwrap_or_default().to_string_lossy();
                        let new_display = new.strip_prefix(&self.settings.pkg_download_path).unwrap_or(new);
                        ui.label(format!("  {} → {}", old_name, new_display.display()));
                    }
                });

                ui.add_space(5.0);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Migrate").clicked() {
                        let migrations = self.v.migration_preview.clone();
                        let ctx = ui.ctx().clone();
                        let _guard = self
                            .v
                            .rt_handle
                            .as_ref()
                            .expect("rt_handle set in UpdatesApp::new before any UI code runs")
                            .enter();
                        self.v.migration_promise = Some(Promise::spawn_async(async move {
                            let count = crate::utils::migrate_directories(&migrations).await;
                            ctx.request_repaint();
                            count
                        }));
                    }
                    if ui.button("Cancel").clicked() {
                        self.v.show_migration_dialog = false;
                    }
                });
            });

        if !show {
            self.v.show_migration_dialog = false;
        }
    }

    fn draw_status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Batch search progress.
                let batch_total = self.v.batch_search_total;
                let batch_done = self.v.batch_search_completed;
                let batch_active = self.v.batch_search_active.len();
                if batch_total > 0 && (batch_done < batch_total || batch_active > 0) {
                    ui.label(format!("Batch: {batch_done}/{batch_total} searched"));
                    ui.separator();
                }

                let active = self.v.download_queue.len();
                let completed = self.v.completed_downloads.len();
                let failed = self.v.failed_downloads.len();
                let pending = self.v.pending_downloads.len();
                let paused = self.v.paused_downloads.len();

                if active > 0 || completed > 0 || failed > 0 || pending > 0 || paused > 0 {
                    let mut status_parts = format!("{active} active, {completed} completed, {failed} failed");
                    if pending > 0 {
                        status_parts.push_str(&format!(", {pending} queued"));
                    }
                    if paused > 0 {
                        status_parts.push_str(&format!(", {paused} paused"));
                    }
                    ui.label(format!("Downloads: {status_parts}"));

                    if active > 0 {
                        // Calculate aggregate progress and speed across all active downloads.
                        let mut total_progress: u64 = 0;
                        let mut total_size: u64 = 0;
                        let mut total_speed: f64 = 0.0;

                        for download in &self.v.download_queue {
                            total_size += download.size;
                            let status = download.progress_status.lock().unwrap();
                            total_progress += status.progress;
                            total_speed += status.speed_bps;
                        }

                        if total_size > 0 {
                            let fraction = total_progress as f32 / total_size as f32;
                            let bar_width = ui.available_width().clamp(50.0, 150.0);
                            ui.separator();
                            ui.label(format!("{} / {}", ByteSize::b(total_progress), ByteSize::b(total_size)));

                            // Speed display.
                            if total_speed > 0.0 {
                                ui.label(format!("{}/s", ByteSize::b(total_speed as u64)));
                                // ETA calculation.
                                let remaining = total_size.saturating_sub(total_progress) as f64;
                                let eta_secs = (remaining / total_speed) as u64;
                                if eta_secs < 3600 {
                                    ui.label(format!("ETA: {}:{:02}", eta_secs / 60, eta_secs % 60));
                                } else {
                                    ui.label(format!(
                                        "ETA: {}:{:02}:{:02}",
                                        eta_secs / 3600,
                                        (eta_secs % 3600) / 60,
                                        eta_secs % 60
                                    ));
                                }
                            }

                            ui.add(
                                egui::ProgressBar::new(fraction)
                                    .desired_width(bar_width)
                                    .show_percentage()
                                    .animate(true),
                            );
                        }
                    }
                } else {
                    ui.label("Ready");
                }
            });
        });
    }

    fn add_download(&mut self, download: ActiveDownload) {
        self.v.download_queue.push(download);
    }

    fn get_active_download(&self, title_id: &str, pkg: &PackageInfo) -> Option<&ActiveDownload> {
        self.v
            .download_queue
            .iter()
            .find(|d| d.title_id == title_id && d.pkg_id == pkg.id())
    }

    fn get_active_merge(&self, title_id: &str) -> Option<&ActiveMerge> {
        self.v.merge_queue.iter().find(|d| d.title_id == title_id)
    }

    fn title_merge_status(&self, update: &UpdateInfo) -> ActiveMergeStatus {
        if let Some(active_merge) = self.get_active_merge(&update.title_id) {
            let merge = active_merge.progress_status.lock().unwrap();
            let progress = merge.progress as f32 / update.packages.len() as f32;
            return ActiveMergeStatus::Merging(progress);
        } else if self.v.completed_merges.contains(&update.title_id) {
            return ActiveMergeStatus::Merged;
        } else if self.v.failed_merges.contains(&update.title_id) {
            return ActiveMergeStatus::Failed;
        }

        ActiveMergeStatus::NotStarted
    }

    fn pkg_merge_status(&self, title_id: &str, pkg: &PackageInfo) -> ActiveMergeStatus {
        if pkg.part_number.is_none() {
            return ActiveMergeStatus::NotMergable;
        }

        let part_number = match pkg.part_number {
            Some(part_num) => part_num,
            None => return ActiveMergeStatus::NotMergable,
        };

        if let Some(active_merge) = self.get_active_merge(title_id) {
            let merge = active_merge.progress_status.lock().unwrap();
            if merge.progress < part_number {
                return ActiveMergeStatus::Merging(0.0);
            } else {
                return ActiveMergeStatus::Merged;
            }
        } else if self.v.completed_merges.iter().any(|id| id == title_id) {
            return ActiveMergeStatus::Merged;
        } else if self.v.failed_merges.iter().any(|id| id == title_id) {
            return ActiveMergeStatus::Failed;
        }

        ActiveMergeStatus::NotStarted
    }
}

// Status pill colors. Kept consistent across the result header and per-package rows so
// that "green/blue/yellow/red/gray" carries the same meaning everywhere in the results list.
const STATUS_COLOR_DONE: egui::Color32 = egui::Color32::from_rgb(80, 200, 120);
const STATUS_COLOR_ACTIVE: egui::Color32 = egui::Color32::from_rgb(100, 180, 255);
const STATUS_COLOR_PAUSED: egui::Color32 = egui::Color32::from_rgb(240, 200, 80);
const STATUS_COLOR_FAILED: egui::Color32 = egui::Color32::from_rgb(240, 100, 100);
const STATUS_COLOR_QUEUED: egui::Color32 = egui::Color32::from_rgb(170, 170, 170);
const LATEST_BADGE_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 215, 80);

/// Solid background color for the per-platform badge in the result header.
fn platform_color(variant: utils::PlatformVariant) -> egui::Color32 {
    match variant {
        utils::PlatformVariant::PS3 => egui::Color32::from_rgb(27, 102, 191),
        utils::PlatformVariant::PS4 => egui::Color32::from_rgb(0, 55, 145),
        utils::PlatformVariant::PSVita => egui::Color32::from_rgb(0, 144, 168),
    }
}

/// Render a small colored capsule with the platform name (e.g. " PS3 ").
fn platform_badge(ui: &mut egui::Ui, variant: utils::PlatformVariant) {
    ui.label(
        egui::RichText::new(format!("  {variant}  "))
            .color(egui::Color32::WHITE)
            .background_color(platform_color(variant))
            .strong(),
    );
}

/// Render a colored bullet followed by a label so a status reads at a glance.
fn status_pill(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(egui::RichText::new("\u{25CF}").color(color));
        ui.label(egui::RichText::new(text).color(color).strong());
    });
}

/// Platform-specific default download path.
#[cfg(target_os = "macos")]
fn default_download_path() -> PathBuf {
    let mut p = dirs::download_dir().unwrap_or_else(|| PathBuf::from("pkgs"));
    p.push("rusty-psn");
    p.push("pkgs");
    p
}

/// Platform-specific default download path.
#[cfg(not(target_os = "macos"))]
fn default_download_path() -> PathBuf {
    PathBuf::from("pkgs/")
}

/// Open a folder in the platform's file manager.
/// Find the first .pkg file in a directory (or its serial subdirectories).
fn find_pkg_in_dir(dir: &Path) -> Option<PathBuf> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pkg")) {
                return Some(path);
            }
        }
    }
    None
}

fn open_folder(path: &Path) {
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
}

#[derive(PartialEq, Debug, Clone, Copy)]
enum ActiveDownloadStatus {
    NotStarted,
    Pending,
    Resuming(f32, u64),
    Downloading(f32),
    Verifying,
    Paused,
    Completed,
    Failed,
}

#[derive(PartialEq, Debug, Clone, Copy)]
enum ActiveMergeStatus {
    NotMergable,
    NotStarted,
    Merging(f32),
    Merged,
    Failed,
}
