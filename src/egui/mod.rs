use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::DerefMut;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::promise::Promise;
use bytesize::ByteSize;
use copypasta::{ClipboardContext, ClipboardProvider};
use eframe::egui;
use log::{error, info, warn};
use notify_rust::Notification;
use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;
use tokio::select;

use crate::psn::*;
use messages::{MessageLevel, MessageLog};

mod chrome;
mod content_browser;
mod dialogs;
mod downloads;
mod downloads_page;
mod firmware;
mod history;
mod messages;
mod placement;
mod results;
mod rpcs3_library;
mod search;
mod settings;

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

    promise: Promise<Result<PathBuf, DownloadError>>,
}

/// Maximum number of automatic retries when Sony's servers drop the connection mid-transfer.
const MAX_AUTO_RETRIES: u32 = 3;

struct FailedDownload {
    title_id: String,
    pkg_id: String,
    title: String,
    pkg: PackageInfo,
    /// The file arrived smaller than Sony said, which usually means its servers dropped the
    /// connection rather than that the file is bad.
    incomplete: bool,
}

/// A package downloaded this session, kept whole so the Downloads page can show its row.
struct FinishedDownload {
    title_id: String,
    title: String,
    pkg: PackageInfo,
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
    /// The partial file, so Abort removes only this package's file.
    file_path: Option<PathBuf>,
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

/// What a background RPCS3 search was started for.
#[derive(Clone, Copy)]
enum DetectFor {
    /// Use what it finds and scan the library.
    Scan,
    /// Fill in the Settings window's RPCS3 fields.
    Settings,
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
    HighContrast,
}

/// Every theme, in the order the settings window lists them.
const APP_THEMES: [AppTheme; 4] = [AppTheme::Dark, AppTheme::Light, AppTheme::PlayStation, AppTheme::HighContrast];

impl AppTheme {
    fn label(self) -> &'static str {
        match self {
            AppTheme::Dark => "Dark",
            AppTheme::Light => "Light",
            AppTheme::PlayStation => "PlayStation",
            AppTheme::HighContrast => "High contrast",
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
                visuals.selection.stroke = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);
                visuals.widgets.hovered.bg_fill = ps_blue_dim;
                visuals.widgets.active.bg_fill = ps_blue;
                visuals.hyperlink_color = ps_blue;

                // Non-interactive widgets (progress bars, labels).
                visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(28, 32, 45);
                visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(40, 45, 60));

                // Inactive widgets (buttons at rest).
                visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(30, 35, 50);
                visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, ps_blue_faint);
                visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(200, 210, 230));

                // Open widgets (combo boxes, menus).
                visuals.widgets.open.bg_fill = ps_blue_dim;

                // Backgrounds.
                visuals.panel_fill = egui::Color32::from_rgb(18, 20, 28);
                visuals.window_fill = egui::Color32::from_rgb(22, 24, 32);
                visuals.extreme_bg_color = egui::Color32::from_rgb(12, 14, 20);

                // Window border with subtle blue.
                visuals.window_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(40, 50, 75));

                ctx.set_visuals(visuals);
            }
            AppTheme::HighContrast => ctx.set_visuals(high_contrast_visuals()),
        }
    }
}

/// White text and outlines on black, with yellow marking what is selected, hovered or pressed.
fn high_contrast_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    let black = egui::Color32::BLACK;
    let white = egui::Color32::WHITE;
    let yellow = egui::Color32::from_rgb(255, 221, 0);
    let outline = egui::Stroke::new(2.0_f32, white);

    // Backgrounds.
    visuals.panel_fill = black;
    visuals.window_fill = black;
    visuals.extreme_bg_color = black;
    visuals.faint_bg_color = egui::Color32::from_gray(30);
    visuals.code_bg_color = black;
    visuals.window_stroke = outline;

    // Text and labels.
    visuals.weak_text_color = Some(egui::Color32::from_gray(210));
    visuals.hyperlink_color = egui::Color32::from_rgb(0, 230, 255);
    visuals.warn_fg_color = yellow;
    visuals.error_fg_color = egui::Color32::from_rgb(255, 110, 110);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, white);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, white);
    visuals.widgets.noninteractive.bg_fill = black;

    // Buttons at rest: outlined in white. A slider's track and a checkbox's box are grey, so they show on black.
    visuals.widgets.inactive.bg_fill = egui::Color32::from_gray(90);
    visuals.widgets.inactive.weak_bg_fill = black;
    visuals.slider_trailing_fill = true;
    visuals.widgets.inactive.bg_stroke = outline;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.5_f32, white);

    // Hovered, pressed and open widgets: outlined in yellow.
    for state in [
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        state.bg_fill = black;
        state.weak_bg_fill = black;
        state.bg_stroke = egui::Stroke::new(3.0_f32, yellow);
        state.fg_stroke = egui::Stroke::new(2.0_f32, yellow);
    }

    // Selected items and text: black on yellow.
    visuals.selection.bg_fill = yellow;
    visuals.selection.stroke = egui::Stroke::new(1.5_f32, black);
    visuals.text_cursor.stroke = egui::Stroke::new(2.0_f32, yellow);

    visuals
}

/// A field missing from the stored settings takes its value from `AppSettings::default()`.
#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
struct AppSettings {
    pkg_download_path: PathBuf,
    show_notifications: bool,
    #[serde(deserialize_with = "default_if_unknown")]
    directory_format: crate::utils::DirectoryFormat,
    #[serde(deserialize_with = "download_limit")]
    max_concurrent_downloads: u32,
    rpcs3_config_dir: Option<PathBuf>,
    rpcs3_executable: Option<PathBuf>,
    content_directory: Option<PathBuf>,
    #[serde(deserialize_with = "default_if_unknown")]
    theme: AppTheme,
    /// How large all text and controls are drawn, as a percentage of normal size.
    #[serde(deserialize_with = "text_size")]
    text_size_percent: u32,
}

/// The text sizes the settings window offers, as percentages of normal size.
const TEXT_SIZES: [u32; 5] = [100, 125, 150, 175, 200];

/// Reads the stored text size into the range the settings window offers.
fn text_size<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    Ok(u32::deserialize(deserializer)?.clamp(TEXT_SIZES[0], TEXT_SIZES[TEXT_SIZES.len() - 1]))
}

/// The label the settings window shows for a text size.
fn text_size_label(percent: u32) -> String {
    if percent == 100 {
        "100% (normal)".to_string()
    } else {
        format!("{percent}%")
    }
}

/// Reads the stored download limit into the range the settings slider offers.
/// A stored 0 would leave every download waiting in the queue.
fn download_limit<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    Ok(u32::deserialize(deserializer)?.clamp(1, 10))
}

/// Reads a stored choice, falling back to its default when the value is one this version does not know,
/// such as a theme added by a newer version. Without this, one such value fails the whole settings file.
fn default_if_unknown<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Stored<T> {
        Known(T),
        Unknown(serde::de::IgnoredAny),
    }

    Ok(match Stored::<T>::deserialize(deserializer)? {
        Stored::Known(value) => value,
        Stored::Unknown(_) => {
            warn!("Stored setting has a value this version does not know; using the default");
            T::default()
        }
    })
}

impl Default for AppSettings {
    fn default() -> AppSettings {
        AppSettings {
            pkg_download_path: default_download_path(),
            show_notifications: false,
            directory_format: crate::utils::DirectoryFormat::default(),
            max_concurrent_downloads: default_max_concurrent(),
            rpcs3_config_dir: None,
            rpcs3_executable: None,
            content_directory: None,
            theme: AppTheme::default(),
            text_size_percent: 100,
        }
    }
}

impl AppSettings {
    /// Draws the window with this theme and text size.
    fn apply_appearance(&self, ctx: &egui::Context) {
        self.theme.apply(ctx);
        ctx.set_zoom_factor(self.text_size_percent as f32 / 100.0);
    }
}

// Values that shouldn't be persisted from run to run.
struct VolatileData {
    rt_handle: Option<Handle>,
    /// What the Messages panel lists.
    message_log: MessageLog,

    clipboard: Option<Box<dyn ClipboardProvider>>,

    serial_query: String,
    update_results: Vec<UpdateInfo>,

    /// The page the sidebar has chosen.
    page: chrome::Page,
    /// Whether the Settings page lists the folders a migration would move.
    migration_listed: bool,
    migration_preview: Vec<(PathBuf, PathBuf)>,
    /// The download folder the preview was scanned from; the migration checks against this one.
    migration_scan_root: PathBuf,
    migration_promise: Option<Promise<(usize, usize)>>,

    settings_dirty: bool,
    modified_settings: AppSettings,

    download_queue: Vec<ActiveDownload>,
    failed_downloads: Vec<FailedDownload>,
    completed_downloads: Vec<FinishedDownload>,
    /// The file each completed download wrote, keyed by (title id, package id).
    completed_paths: HashMap<(String, String), PathBuf>,
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
    rpcs3_detect_promise: Option<(DetectFor, Promise<Option<crate::rpcs3::Rpcs3Installation>>)>,
    rpcs3_installed_games: Vec<crate::rpcs3::InstalledGame>,

    // Firmware download state
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

    // RPCS3 install state. Key is (title_id, pkg_id).
    #[allow(clippy::type_complexity)]
    rpcs3_install_promises: Vec<(
        String,
        String,
        Promise<Result<crate::rpcs3::InstallOutcome, crate::rpcs3::Rpcs3Error>>,
    )>,
    rpcs3_installed_pkgs: HashSet<(String, String)>,

    // Content browser state.
    content_library: Option<crate::content::ContentLibrary>,
    /// A content folder scan running in the background; `None` inside means it panicked.
    content_scan_promise: Option<Promise<Option<crate::content::ContentLibrary>>>,
    content_filter: String,
    /// Tracks DLC installs in progress: zip filename → promise.
    content_install_promises: Vec<(
        String,
        Promise<Result<crate::rpcs3::InstallOutcome, crate::content::ContentError>>,
    )>,
    content_installed_items: HashSet<String>,

    /// Cached egui context for use in code paths without direct UI access (e.g., auto-retry).
    egui_ctx: Option<egui::Context>,

    /// Shared HTTP client for all PSN requests. Constructed once, reused across all
    /// searches and downloads. reqwest::Client is cheaply cloneable (Arc internally).
    psn_client: Option<reqwest::Client>,

    /// Where KWin last had the window frame, on Wayland only; see `placement`.
    frame_position: Option<(i32, i32)>,
    /// Whether the saved position has been handed to KWin this run.
    placement_restored: bool,
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
            message_log: MessageLog::default(),

            clipboard,

            serial_query: String::new(),
            update_results: Vec::new(),

            page: chrome::Page::Search,
            migration_listed: false,
            migration_preview: Vec::new(),
            migration_scan_root: PathBuf::new(),
            migration_promise: None,

            settings_dirty: false,
            modified_settings: AppSettings::default(),

            download_queue: Vec::new(),
            failed_downloads: Vec::new(),
            completed_downloads: Vec::new(),
            completed_paths: HashMap::new(),
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
            rpcs3_detect_promise: None,
            rpcs3_installed_games: Vec::new(),

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

            rpcs3_install_promises: Vec::new(),
            rpcs3_installed_pkgs: HashSet::new(),

            content_library: None,
            content_scan_promise: None,
            content_filter: String::new(),
            content_install_promises: Vec::new(),
            content_installed_items: HashSet::new(),

            egui_ctx: None,

            psn_client: None,

            frame_position: None,
            placement_restored: false,
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
        if let Some(position) = self.v.frame_position {
            eframe::set_value(storage, placement::STORAGE_KEY, &position);
        }
    }

    // Runs before every `ui` call, and also while the window is hidden, so downloads keep moving when minimised.
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut messages = Vec::new();

        // Check the status of the search promise.
        self.handle_search_promise(&mut messages);
        // Process batch search queue (parallel searches with concurrency limit).
        self.handle_batch_search_queue(&mut messages);
        // Check in on active downloads.
        self.handle_download_promises(&mut messages);
        self.handle_merge_promises(&mut messages);
        self.handle_rpcs3_detect_promise(&mut messages);
        self.handle_rpcs3_scan_promise(&mut messages);
        self.handle_firmware_promises(&mut messages);
        self.handle_rpcs3_install_promises(&mut messages);
        self.handle_content_install_promises(&mut messages);
        self.handle_content_scan_promise(&mut messages);

        for (msg, level) in messages {
            self.show_notifications(msg, level);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();

        // Build per-frame download status cache (avoids repeated linear scans).
        self.v.download_status_cache = self.build_download_status_cache();

        // Handle global keyboard shortcuts.
        self.handle_keyboard_shortcuts(ctx);
        self.handle_window_placement(ctx);

        // The sidebar, the status bar and the Messages panel must come before CentralPanel.
        self.draw_sidebar(ui);
        self.draw_status_bar(ui);
        self.draw_messages_panel(ui);

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading(self.v.page.label());
            ui.separator();
            match self.v.page {
                chrome::Page::Search => {
                    self.draw_search_bar(ui);
                    ui.separator();
                    self.draw_results_list(ctx, ui);
                }
                chrome::Page::Downloads => self.draw_downloads_page(ui),
                chrome::Page::Rpcs3Library => self.draw_rpcs3_library_page(ui),
                chrome::Page::PatchesAndDlc => self.draw_content_browser_page(ui),
                chrome::Page::Firmware => self.draw_firmware_page(ui),
                chrome::Page::History => self.draw_history_page(ui),
                chrome::Page::Settings => self.draw_settings_page(ui),
            }
        });

        self.poll_migration();
    }
}

impl UpdatesApp {
    pub fn new(cc: &eframe::CreationContext<'_>, rt_handle: Handle, psn_client: reqwest::Client) -> Self {
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

        let mut app: Self = cc
            .storage
            .and_then(|storage| {
                let app = eframe::get_value(storage, eframe::APP_KEY);
                if app.is_none() && storage.get_string(eframe::APP_KEY).is_some() {
                    // The next save overwrites the stored copy, so this log line is the only trace of it.
                    error!("Stored settings could not be read; starting with the defaults");
                }
                app
            })
            .unwrap_or_default();

        app.v.rt_handle = Some(rt_handle);
        app.v.psn_client = Some(psn_client);
        app.v.egui_ctx = Some(cc.egui_ctx.clone());
        app.v.frame_position = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, placement::STORAGE_KEY));

        // Apply the persisted theme and text size on startup.
        app.settings.apply_appearance(&cc.egui_ctx);

        app
    }

    fn show_notifications<S: Into<String>>(&mut self, msg: S, level: MessageLevel) {
        let msg = msg.into();

        self.v.message_log.push(level, msg.clone());

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
}

/// What a status label means. Each pairs a colour with an icon, and every label carries words too,
/// so no status is told by colour alone (RPSN-0144). The same kind looks the same everywhere.
#[derive(Clone, Copy, Debug, PartialEq)]
enum StatusKind {
    /// Finished, installed or up to date.
    Done,
    /// Downloading.
    Active,
    /// Verifying, merging, installing or checking: busy, nothing to do yet.
    Working,
    Paused,
    Failed,
    /// Waiting its turn.
    Queued,
    /// Needs a look, such as an update that is available or a folder that is wrong.
    Attention,
}

impl StatusKind {
    fn icon(self) -> &'static str {
        match self {
            StatusKind::Done => "\u{2714}",      // ✔
            StatusKind::Active => "\u{2B07}",    // ⬇
            StatusKind::Working => "\u{231B}",   // ⌛
            StatusKind::Paused => "\u{23F8}",    // ⏸
            StatusKind::Failed => "\u{2716}",    // ✖
            StatusKind::Queued => "\u{2022}",    // •
            StatusKind::Attention => "\u{26A0}", // ⚠
        }
    }

    /// The colour for this status, darker on a light theme so it stays readable there.
    fn color(self, visuals: &egui::Visuals) -> egui::Color32 {
        let (dark, light) = match self {
            StatusKind::Done => ((80, 200, 120), (0, 120, 50)),
            StatusKind::Active => ((100, 180, 255), (0, 80, 180)),
            StatusKind::Working | StatusKind::Paused => ((240, 200, 80), (140, 90, 0)),
            StatusKind::Failed => ((240, 100, 100), (180, 20, 20)),
            StatusKind::Queued => ((170, 170, 170), (90, 90, 90)),
            StatusKind::Attention => ((255, 170, 60), (160, 70, 0)),
        };
        let (r, g, b) = if visuals.dark_mode { dark } else { light };
        egui::Color32::from_rgb(r, g, b)
    }
}

/// The colour of the "Latest" badge, from the same palette as the status labels.
fn latest_badge_color(visuals: &egui::Visuals) -> egui::Color32 {
    StatusKind::Attention.color(visuals)
}

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

/// Render a status as an icon and words in the status's colour, so it reads at a glance.
fn status_pill(ui: &mut egui::Ui, text: &str, kind: StatusKind) {
    let color = kind.color(ui.visuals());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(egui::RichText::new(kind.icon()).color(color));
        ui.label(egui::RichText::new(text).color(color).strong());
    });
}

/// The default download path: `rusty-psn/pkgs` in the user's Downloads folder, or in
/// their home folder where there is none. Relative only when neither is known.
fn default_download_path() -> PathBuf {
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .map(|dir| dir.join("rusty-psn").join("pkgs"))
        .unwrap_or_else(|| PathBuf::from("pkgs"))
}

impl UpdatesApp {
    /// Open a folder in the platform's file manager, saying so when it cannot be started.
    fn open_folder(&mut self, path: &Path) {
        #[cfg(target_os = "linux")]
        let opener = "xdg-open";
        #[cfg(target_os = "macos")]
        let opener = "open";
        #[cfg(target_os = "windows")]
        let opener = "explorer";

        if let Err(e) = std::process::Command::new(opener).arg(path).spawn() {
            warn!("Could not open {} with {opener}: {e}", path.display());
            self.show_notifications(format!("Could not open the folder: {e}"), MessageLevel::Warning);
        }
    }

    /// Copy text to the clipboard, saying so when that fails.
    fn copy_to_clipboard(&mut self, text: String) {
        let Some(clip) = self.v.clipboard.as_mut() else {
            self.show_notifications("No clipboard is available.", MessageLevel::Warning);
            return;
        };
        if let Err(e) = clip.set_contents(text) {
            warn!("Could not copy to the clipboard: {e}");
            self.show_notifications(format!("Could not copy: {e}"), MessageLevel::Warning);
        }
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
    NotMergeable,
    NotStarted,
    Merging(f32),
    Merged,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(stored: &str) -> Option<AppSettings> {
        ron::from_str::<UpdatesApp>(stored).ok().map(|app| app.settings)
    }

    #[test]
    fn unknown_theme_keeps_the_other_settings() {
        // One value the app cannot read used to reset every setting (RPSN-0101).
        let settings =
            load(r#"(settings:(pkg_download_path:"/games/pkgs",show_toasts:false,show_notifications:true,theme:Sepia))"#);
        assert!(settings.is_some(), "settings with an unknown theme did not load");
        let settings = settings.unwrap(); // Checked just above.
        assert_eq!(settings.pkg_download_path, PathBuf::from("/games/pkgs"));
        assert!(settings.show_notifications);
        assert_eq!(settings.theme, AppTheme::default());
    }

    /// Runs one frame that presses `key` with `modifiers`, then the app's shortcut handling.
    fn press(app: &mut UpdatesApp, ctx: &egui::Context, key: egui::Key, modifiers: egui::Modifiers) {
        let event = egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        let input = egui::RawInput {
            events: vec![egui::Event::ModifiersChanged(modifiers), event],
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| app.handle_keyboard_shortcuts(&ui.ctx().clone()));
        // Nothing draws these textures; epaint panics if they are dropped unapplied.
        output.textures_delta.clear();
    }

    #[test]
    fn shortcuts_switch_pages() {
        let ctx = egui::Context::default();
        let mut app = UpdatesApp::default();

        press(&mut app, &ctx, egui::Key::S, egui::Modifiers::COMMAND);
        assert_eq!(app.v.page, chrome::Page::Settings, "Ctrl+S opens Settings");

        app.v.modified_settings.max_concurrent_downloads = 9;
        app.v.settings_dirty = true;
        press(&mut app, &ctx, egui::Key::S, egui::Modifiers::COMMAND);
        assert_eq!(app.settings.max_concurrent_downloads, 9, "Ctrl+S on Settings saves");

        press(&mut app, &ctx, egui::Key::Escape, egui::Modifiers::NONE);
        assert_eq!(app.v.page, chrome::Page::Search, "Escape returns to Search");
    }

    #[test]
    fn restoring_defaults_saves_nothing_until_save() {
        let ctx = egui::Context::default();
        let mut app = UpdatesApp::default();
        app.settings.max_concurrent_downloads = 7;
        app.go_to(chrome::Page::Settings, &ctx);

        app.restore_default_settings(&ctx);
        assert!(app.v.settings_dirty);
        assert_eq!(app.settings.max_concurrent_downloads, 7, "the saved settings are untouched");

        app.save_settings();
        assert_eq!(
            app.settings.max_concurrent_downloads,
            AppSettings::default().max_concurrent_downloads
        );
    }

    #[test]
    fn leaving_settings_drops_unsaved_changes() {
        // Closing the Settings window discarded its edits; leaving the Settings page must too (RPSN-0157).
        let ctx = egui::Context::default();
        let mut app = UpdatesApp::default();
        app.go_to(chrome::Page::Settings, &ctx);
        app.v.modified_settings.max_concurrent_downloads = 9;
        app.v.settings_dirty = true;

        app.go_to(chrome::Page::Search, &ctx);
        assert!(!app.v.settings_dirty);
        app.go_to(chrome::Page::Settings, &ctx);
        assert_eq!(
            app.v.modified_settings.max_concurrent_downloads,
            app.settings.max_concurrent_downloads
        );
    }

    #[test]
    fn saved_settings_load_unchanged() {
        let mut app = UpdatesApp::default();
        app.settings.theme = AppTheme::PlayStation;
        app.settings.directory_format = crate::utils::DirectoryFormat::TitleSerial;
        app.settings.max_concurrent_downloads = 7;
        app.settings.text_size_percent = 150;
        let stored = ron::ser::to_string(&app).expect("settings serialize");

        let settings = load(&stored);
        assert!(settings.is_some(), "saved settings did not load: {stored}");
        let settings = settings.unwrap(); // Checked just above.
        assert_eq!(settings.theme, AppTheme::PlayStation);
        assert_eq!(settings.directory_format, crate::utils::DirectoryFormat::TitleSerial);
        assert_eq!(settings.max_concurrent_downloads, 7);
        assert_eq!(settings.text_size_percent, 150);
    }

    #[test]
    fn no_two_statuses_look_the_same() {
        // A status told apart by colour alone fails a reader who cannot see the colour (RPSN-0144).
        let kinds = [
            StatusKind::Done,
            StatusKind::Active,
            StatusKind::Working,
            StatusKind::Paused,
            StatusKind::Failed,
            StatusKind::Queued,
            StatusKind::Attention,
        ];
        for (i, a) in kinds.iter().enumerate() {
            for b in &kinds[i + 1..] {
                assert_ne!(a.icon(), b.icon(), "{a:?} and {b:?} share an icon");
            }
        }
    }

    #[test]
    fn status_colours_follow_the_theme() {
        // A pale colour that reads on a dark background washes out on a light one (RPSN-0062).
        let (dark, light) = (egui::Visuals::dark(), egui::Visuals::light());
        for kind in [StatusKind::Done, StatusKind::Failed, StatusKind::Attention] {
            assert_ne!(kind.color(&dark), kind.color(&light), "{kind:?}");
        }
    }

    #[test]
    fn every_theme_is_offered_and_loads_after_saving() {
        // A theme missing from the list cannot be chosen; one that does not load back resets on restart.
        for theme in APP_THEMES {
            let mut app = UpdatesApp::default();
            app.settings.theme = theme;
            let stored = ron::ser::to_string(&app).expect("settings serialize");
            let settings = load(&stored);
            assert!(settings.is_some(), "saved theme {} did not load", theme.label());
            let settings = settings.unwrap(); // Checked just above.
            assert_eq!(settings.theme, theme);
        }
        assert!(APP_THEMES.contains(&AppTheme::HighContrast));
    }

    #[test]
    fn a_stored_text_size_loads_inside_the_offered_range() {
        // A hand-edited or corrupted size could make the window unreadably small or large (RPSN-0142).
        for (stored, expected) in [(0, 100), (40, 100), (130, 130), (900, 200)] {
            let settings = load(&format!("(settings:(text_size_percent:{stored}))"));
            assert!(settings.is_some(), "settings with text size {stored} did not load");
            let settings = settings.unwrap(); // Checked just above.
            assert_eq!(settings.text_size_percent, expected, "stored {stored}");
        }
    }

    #[test]
    fn a_stored_download_limit_of_zero_loads_as_one() {
        // With 0 every download waited in the queue for ever (RPSN-0117).
        let settings = load(r#"(settings:(max_concurrent_downloads:0))"#);
        assert!(settings.is_some(), "settings with a zero limit did not load");
        assert_eq!(settings.unwrap().max_concurrent_downloads, 1); // Checked just above.
    }

    #[test]
    fn the_default_download_folder_does_not_depend_on_where_the_app_starts() {
        // A relative default landed wherever the app was launched from (RPSN-0117).
        assert!(default_download_path().is_absolute(), "{:?}", default_download_path());
    }

    #[test]
    fn missing_setting_takes_its_default() {
        let settings = load(r#"(settings:(pkg_download_path:"/games/pkgs",directory_format:Sideways))"#);
        assert!(settings.is_some(), "settings missing a field did not load");
        let settings = settings.unwrap(); // Checked just above.
        assert_eq!(settings.pkg_download_path, PathBuf::from("/games/pkgs"));
        assert!(!settings.show_notifications);
        assert_eq!(settings.directory_format, crate::utils::DirectoryFormat::default());
        assert_eq!(settings.text_size_percent, 100);
    }
}
