//! Downloads and merges: queueing, starting, tracking and finishing them.

use super::*;

impl UpdatesApp {
    pub(super) fn handle_download_promises(&mut self, messages: &mut Vec<(String, MessageLevel)>) {
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
                    Ok(path) => {
                        info!("Download completed! ({} {})", download.title_id, download.pkg_id);
                        self.v
                            .completed_paths
                            .insert((download.title_id.clone(), download.pkg_id.clone()), path.clone());

                        messages.push((
                            tr!(
                                "downloads-done",
                                serial = download.title_id.as_str(),
                                version = download.pkg_id.as_str()
                            ),
                            MessageLevel::Success,
                        ));
                        self.v.completed_downloads.push(FinishedDownload {
                            title_id: download.title_id.clone(),
                            title: download.title.clone(),
                            pkg: download.pkg.clone(),
                        });
                        history::record(
                            &mut self.v.download_history,
                            DownloadHistoryEntry {
                                title_id: download.title_id.clone(),
                                pkg_id: download.pkg_id.clone(),
                                title: download.title.clone(),
                                size: download.size,
                                completed_at: history::now(),
                            },
                        );
                    }
                    Err(e) => {
                        match e {
                            DownloadError::HashMismatch { short_on_data, .. } => {
                                if *short_on_data && download.retry_count < MAX_AUTO_RETRIES {
                                    // Sony dropped the connection. Auto-retry with resume.
                                    info!(
                                        "Short transfer for {} {}, auto-retrying ({}/{})",
                                        download.title_id,
                                        download.pkg_id,
                                        download.retry_count + 1,
                                        MAX_AUTO_RETRIES
                                    );
                                    messages.push((
                                        tr!(
                                            "downloads-retrying",
                                            serial = download.title_id.as_str(),
                                            version = download.pkg_id.as_str(),
                                            attempt = (download.retry_count + 1).to_string(),
                                            max = MAX_AUTO_RETRIES.to_string()
                                        ),
                                        MessageLevel::Warning,
                                    ));
                                    auto_retry_queue.push((
                                        download.pkg.clone(),
                                        download.title_id.clone(),
                                        download.title.clone(),
                                        download.retry_count + 1,
                                    ));
                                } else {
                                    // True hash mismatch or max retries exceeded.
                                    e.discard_kept_file();
                                    self.v.failed_downloads.push(FailedDownload {
                                        title_id: download.title_id.clone(),
                                        pkg_id: download.pkg_id.clone(),
                                        title: download.title.clone(),
                                        pkg: download.pkg.clone(),
                                        incomplete: false,
                                    });
                                    messages.push((
                                        tr!(
                                            "downloads-hash-mismatch",
                                            serial = download.title_id.as_str(),
                                            version = download.pkg_id.as_str()
                                        ),
                                        MessageLevel::Error,
                                    ));
                                    if *short_on_data {
                                        if let Some(failed) = self.v.failed_downloads.last_mut() {
                                            failed.incomplete = true;
                                        }
                                        messages.push((tr!("downloads-short-hint"), MessageLevel::Warning));
                                    }
                                }
                            }
                            DownloadError::Tokio(err) => {
                                self.v.failed_downloads.push(FailedDownload {
                                    title_id: download.title_id.clone(),
                                    pkg_id: download.pkg_id.clone(),
                                    title: download.title.clone(),
                                    pkg: download.pkg.clone(),
                                    incomplete: false,
                                });
                                messages.push((
                                    tr!(
                                        "downloads-failed",
                                        serial = download.title_id.as_str(),
                                        version = download.pkg_id.as_str(),
                                        reason = err.to_string()
                                    ),
                                    MessageLevel::Error,
                                ));
                            }
                            DownloadError::Reqwest(err) => {
                                self.v.failed_downloads.push(FailedDownload {
                                    title_id: download.title_id.clone(),
                                    pkg_id: download.pkg_id.clone(),
                                    title: download.title.clone(),
                                    pkg: download.pkg.clone(),
                                    incomplete: false,
                                });
                                messages.push((
                                    tr!(
                                        "downloads-failed",
                                        serial = download.title_id.as_str(),
                                        version = download.pkg_id.as_str(),
                                        reason = err.to_string()
                                    ),
                                    MessageLevel::Error,
                                ));
                            }
                            DownloadError::Security(err) => {
                                self.v.failed_downloads.push(FailedDownload {
                                    title_id: download.title_id.clone(),
                                    pkg_id: download.pkg_id.clone(),
                                    title: download.title.clone(),
                                    pkg: download.pkg.clone(),
                                    incomplete: false,
                                });
                                messages.push((
                                    tr!(
                                        "downloads-security-failed",
                                        serial = download.title_id.as_str(),
                                        version = download.pkg_id.as_str(),
                                        reason = err.to_string()
                                    ),
                                    MessageLevel::Error,
                                ));
                            }
                            // Packages are checked by hash instead; only firmware reports this.
                            DownloadError::Incomplete { received, expected } => {
                                self.v.failed_downloads.push(FailedDownload {
                                    title_id: download.title_id.clone(),
                                    pkg_id: download.pkg_id.clone(),
                                    title: download.title.clone(),
                                    pkg: download.pkg.clone(),
                                    incomplete: false,
                                });
                                messages.push((
                                    tr!(
                                        "downloads-incomplete",
                                        serial = download.title_id.as_str(),
                                        version = download.pkg_id.as_str(),
                                        received = received.to_string(),
                                        expected = expected.to_string()
                                    ),
                                    MessageLevel::Error,
                                ));
                            }
                            DownloadError::Cancelled { file_path, abort } => {
                                if *abort {
                                    info!("Aborted download {} {}, cleaning up", download.title_id, download.pkg_id);
                                    if let Some(path) = file_path {
                                        abort_file_cleanup.push(path.clone());
                                    }
                                    messages.push((
                                        tr!(
                                            "downloads-aborted",
                                            serial = download.title_id.as_str(),
                                            version = download.pkg_id.as_str()
                                        ),
                                        MessageLevel::Warning,
                                    ));
                                } else {
                                    info!("Paused download {} {}", download.title_id, download.pkg_id);
                                    pause_queue.push(PausedDownload {
                                        title_id: download.title_id.clone(),
                                        pkg_id: download.pkg_id.clone(),
                                        title: download.title.clone(),
                                        pkg: download.pkg.clone(),
                                        file_path: file_path.clone(),
                                    });
                                    messages.push((
                                        tr!(
                                            "downloads-paused",
                                            serial = download.title_id.as_str(),
                                            version = download.pkg_id.as_str()
                                        ),
                                        MessageLevel::Info,
                                    ));
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

    pub(super) fn handle_merge_promises(&mut self, messages: &mut Vec<(String, MessageLevel)>) {
        let mut finished_merge_indexes: Vec<usize> = Vec::new();
        for i in 0..self.v.merge_queue.len() {
            let merge = &mut self.v.merge_queue[i];
            if let Some(result) = merge.promise.ready() {
                match result {
                    Ok(_) => {
                        info!("Merge completed for {}", merge.title_id);

                        messages.push((
                            tr!("downloads-merged", serial = merge.title_id.as_str()),
                            MessageLevel::Success,
                        ));
                        self.v.completed_merges.push(merge.title_id.clone());
                    }
                    Err(e) => {
                        self.v.failed_merges.push(merge.title_id.clone());

                        messages.push((
                            tr!(
                                "downloads-merge-failed",
                                serial = merge.title_id.as_str(),
                                reason = e.to_string()
                            ),
                            MessageLevel::Error,
                        ));

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

    /// Build a cache of all download statuses. Called once per frame to avoid
    /// repeated linear scans of the download/completed/failed/paused/pending lists.
    pub(super) fn build_download_status_cache(&self) -> HashMap<(String, String), CachedDownloadInfo> {
        let capacity = self.v.download_queue.len()
            + self.v.completed_downloads.len()
            + self.v.failed_downloads.len()
            + self.v.paused_downloads.len()
            + self.v.pending_downloads.len();
        let mut cache = HashMap::with_capacity(capacity);

        // Active downloads (with speed/progress data).
        for dl in &self.v.download_queue {
            let status_guard = dl.progress_status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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

        for finished in &self.v.completed_downloads {
            cache.insert(
                (finished.title_id.clone(), finished.pkg.id()),
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
    pub(super) fn cached_download_info(&self, title_id: &str, pkg: &PackageInfo) -> CachedDownloadInfo {
        self.v
            .download_status_cache
            .get(&(title_id.to_string(), pkg.id()))
            .copied()
            .unwrap_or_default()
    }

    /// Queue a download, respecting the max concurrent downloads setting.
    /// If all slots are full, the download is added to the pending queue.
    pub(super) fn queue_download(&mut self, title_id: String, title: String, pkg: PackageInfo, ui: &egui::Ui) {
        if self.is_download_known(&title_id, &pkg) {
            return;
        }
        if self.v.download_queue.len() < self.settings.max_concurrent_downloads as usize {
            let active = self.start_download(title_id, title, pkg, ui);
            self.v.download_queue.push(active);
        } else {
            self.v.pending_downloads.push_back(PendingDownload { title_id, title, pkg });
        }
    }

    /// Queue a download without a UI reference (for "Download All" and pending promotion).
    pub(super) fn queue_download_no_ui(&mut self, title_id: String, title: String, pkg: PackageInfo) {
        if self.is_download_known(&title_id, &pkg) {
            return;
        }
        if self.v.download_queue.len() < self.settings.max_concurrent_downloads as usize {
            let active = self.start_download_with_retry(title_id, title, pkg, 0);
            self.v.download_queue.push(active);
        } else {
            self.v.pending_downloads.push_back(PendingDownload { title_id, title, pkg });
        }
    }

    pub(super) fn start_download(&self, serial: String, title: String, pkg: PackageInfo, ui: &egui::Ui) -> ActiveDownload {
        self.start_download_inner(serial, title, pkg, 0, ui.ctx().clone())
    }

    /// Re-queue a download with an incremented retry count (used for auto-retry on short transfers).
    /// Uses the cached egui context instead of requiring a UI reference.
    pub(super) fn start_download_with_retry(
        &self,
        serial: String,
        title: String,
        pkg: PackageInfo,
        retry_count: u32,
    ) -> ActiveDownload {
        // Use the cached real context so that request_repaint() actually triggers UI updates.
        // Falls back to a default context only if the cache was somehow never initialized.
        let ctx = self.v.egui_ctx.clone().unwrap_or_default();
        self.start_download_inner(serial, title, pkg, retry_count, ctx)
    }

    pub(super) fn start_download_inner(
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

        let dl_client = self
            .v
            .psn_client
            .clone()
            .expect("psn_client set in UpdatesApp::new before any UI code runs");
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
                            let mut guard = async_status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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

    pub(super) fn start_merge_parts(&self, update_info: UpdateInfo, ui: &egui::Ui) -> ActiveMerge {
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
                        let mut guard = async_status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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

    pub(super) fn add_download(&mut self, download: ActiveDownload) {
        self.v.download_queue.push(download);
    }

    /// True when this package is already downloading, waiting for a slot, or finished this session.
    pub(super) fn is_download_known(&self, title_id: &str, pkg: &PackageInfo) -> bool {
        let pkg_id = pkg.id();
        self.get_active_download(title_id, pkg).is_some()
            || self
                .v
                .pending_downloads
                .iter()
                .any(|p| p.title_id == title_id && p.pkg.id() == pkg_id)
            || self
                .v
                .completed_downloads
                .iter()
                .any(|f| f.title_id == title_id && f.pkg.id() == pkg_id)
    }

    pub(super) fn get_active_download(&self, title_id: &str, pkg: &PackageInfo) -> Option<&ActiveDownload> {
        self.v
            .download_queue
            .iter()
            .find(|d| d.title_id == title_id && d.pkg_id == pkg.id())
    }

    pub(super) fn get_active_merge(&self, title_id: &str) -> Option<&ActiveMerge> {
        self.v.merge_queue.iter().find(|d| d.title_id == title_id)
    }

    pub(super) fn title_merge_status(&self, update: &UpdateInfo) -> ActiveMergeStatus {
        if let Some(active_merge) = self.get_active_merge(&update.title_id) {
            let merge = active_merge
                .progress_status
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let progress = merge.progress as f32 / update.packages.len() as f32;
            return ActiveMergeStatus::Merging(progress);
        } else if self.v.completed_merges.contains(&update.title_id) {
            return ActiveMergeStatus::Merged;
        } else if self.v.failed_merges.contains(&update.title_id) {
            return ActiveMergeStatus::Failed;
        }

        ActiveMergeStatus::NotStarted
    }

    pub(super) fn pkg_merge_status(&self, title_id: &str, pkg: &PackageInfo) -> ActiveMergeStatus {
        if pkg.part_number.is_none() {
            return ActiveMergeStatus::NotMergeable;
        }

        let part_number = match pkg.part_number {
            Some(part_num) => part_num,
            None => return ActiveMergeStatus::NotMergeable,
        };

        if let Some(active_merge) = self.get_active_merge(title_id) {
            let merge = active_merge
                .progress_status
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
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
