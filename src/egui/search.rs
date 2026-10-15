//! Searching: the search bar, single and batch searches, and batch import.

use super::*;

impl VolatileData {
    /// Queues serials for a batch search, skipping any already found, queued or being searched.
    /// A batch already running grows by the new serials rather than being replaced. Returns how many were queued.
    pub(super) fn enqueue_batch_search(&mut self, serials: impl IntoIterator<Item = String>) -> usize {
        let new_serials: Vec<String> = serials
            .into_iter()
            .filter(|s| !self.update_results.iter().any(|u| u.title_id == *s))
            .filter(|s| !self.batch_search_queue.iter().any(|q| q == s))
            .filter(|s| !self.batch_search_active.iter().any(|(id, _)| id == s))
            .collect();

        // A finished batch leaves its total at 0 (see handle_batch_search_queue), so start counting afresh.
        if self.batch_search_total == 0 {
            self.batch_search_completed = 0;
            self.batch_search_errors.clear();
        }
        let queued = new_serials.len();
        self.batch_search_total += queued;
        self.batch_search_queue.extend(new_serials);
        queued
    }
}

impl UpdatesApp {
    /// Collect every finished search typed into the search bar. Several may run at once
    /// (RPSN-0069), so each message names its serial.
    pub(super) fn handle_search_promises(&mut self, messages: &mut Vec<(String, MessageLevel)>) {
        let mut i = 0;
        while i < self.v.search_promises.len() {
            if self.v.search_promises[i].1.ready().is_none() {
                i += 1;
                continue;
            }
            let (serial, promise) = self.v.search_promises.remove(i);
            match promise.block_and_take() {
                Ok(update_info) => {
                    info!("Received search results for serial {}", update_info.title_id);
                    if !self.v.update_results.iter().any(|u| u.title_id == update_info.title_id) {
                        self.v.update_results.push(update_info);
                    }
                }
                Err(e) => {
                    let text = match &e {
                        UpdateError::UnhandledErrorResponse(e) => {
                            format!("Unexpected error received in a response from PSN ({e}).")
                        }
                        UpdateError::InvalidSerial => {
                            String::from("The provided serial didn't give any results, double-check your input.")
                        }
                        UpdateError::NoUpdatesAvailable => {
                            String::from("The provided serial doesn't have any available updates.")
                        }
                        UpdateError::Reqwest(e) => format!("There was an error completing the request ({e})."),
                        UpdateError::XmlParsing(e) => format!("Error parsing response from Sony, try again later ({e})."),
                        UpdateError::ManifestParsing(e) => {
                            format!("Error parsing manifest response from Sony, try again later ({e}).")
                        }
                        UpdateError::Security(e) => format!("Security check failed: {e}"),
                        UpdateError::ResponseTooLarge(size) => {
                            format!("Response too large ({size} bytes), possible server error.")
                        }
                    };
                    messages.push((format!("{serial}: {text}"), MessageLevel::Error));
                    error!("Error received from updates query for {serial}: {e:?}");
                }
            }
        }
    }

    pub(super) fn handle_batch_search_queue(&mut self, messages: &mut Vec<(String, MessageLevel)>) {
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
                    if matches!(e, UpdateError::NoUpdatesAvailable) {
                        self.v.no_update_serials.insert(serial.clone());
                    }
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
            let c = self
                .v
                .psn_client
                .clone()
                .expect("psn_client set in UpdatesApp::new before any UI code runs");
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
            let found = self.v.batch_search_total.saturating_sub(errors);
            let msg = if errors > 0 {
                format!("Batch search complete: {found} found, {errors} failed.")
            } else {
                format!("Batch search complete: {found} title(s) found.")
            };
            messages.push((msg, MessageLevel::Info));
            self.v.batch_search_total = 0; // Reset so the toast doesn't fire again.
        }
    }

    pub(super) fn trigger_batch_import(&mut self) {
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
                        MessageLevel::Warning,
                    );
                }

                let queued = self.v.enqueue_batch_search(result.serials);

                if queued == 0 {
                    self.show_notifications("All serials from the batch file are already loaded.", MessageLevel::Info);
                    return;
                }

                self.show_notifications(
                    format!("Batch import: {queued} serial(s) queued for search."),
                    MessageLevel::Info,
                );
            }
            Err(e) => {
                self.show_notifications(format!("Failed to import batch file: {e}"), MessageLevel::Error);
            }
        }
    }

    pub(super) fn draw_search_bar(&mut self, ui: &mut egui::Ui) {
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

            let in_flight = self
                .v
                .search_promises
                .iter()
                .any(|(s, _)| *s == parse_title_id(&self.v.serial_query));
            ui.add_enabled_ui(!self.v.serial_query.is_empty() && !in_flight, |ui| {
                if !input_submitted && !ui.button("Search for updates").clicked() {
                    return;
                }

                let already_searched = self
                    .v
                    .update_results
                    .iter()
                    .any(|e| e.title_id == parse_title_id(&self.v.serial_query));
                if already_searched {
                    self.show_notifications("Provided title id results already shown", MessageLevel::Info);
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
                let c = self
                    .v
                    .psn_client
                    .clone()
                    .expect("psn_client set in UpdatesApp::new before any UI code runs");
                let promise = Promise::spawn_async(async move {
                    let result = UpdateInfo::get_info(serial_query, &c).await;
                    ctx.request_repaint();
                    result
                });

                self.v.search_promises.push((parse_title_id(&self.v.serial_query), promise));
            });

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

        // Row 2: Action buttons — always visible, wrapping onto a second line at a large text size.
        ui.horizontal_wrapped(|ui| {
            if ui.button("Import Batch...").clicked() {
                self.trigger_batch_import();
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
                    self.v.no_update_serials.clear();
                }
            });

            if ui.button("Open Downloads").clicked() {
                let folder = self.settings.pkg_download_path.clone();
                self.open_folder(&folder);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn several_typed_searches_run_at_once() {
        // Only one search could run at a time (RPSN-0069).
        let rt = tokio::runtime::Runtime::new().expect("a test runtime starts");
        let _guard = rt.enter();
        let (_still_waiting, wait) = tokio::sync::oneshot::channel::<()>();

        let mut app = UpdatesApp::default();
        app.v.search_promises = vec![
            (
                "BLES00001".into(),
                Promise::spawn_async(async { Err(UpdateError::NoUpdatesAvailable) }),
            ),
            (
                "BLES00002".into(),
                Promise::spawn_async(async { Err(UpdateError::InvalidSerial) }),
            ),
            (
                "BLES00003".into(),
                Promise::spawn_async(async move {
                    let _ = wait.await;
                    Err(UpdateError::InvalidSerial)
                }),
            ),
        ];
        while app.v.search_promises[..2].iter().any(|(_, p)| p.ready().is_none()) {
            std::thread::yield_now();
        }

        let mut messages = Vec::new();
        app.handle_search_promises(&mut messages);
        let texts: Vec<_> = messages.iter().map(|(text, _)| text.as_str()).collect();
        assert!(texts[0].starts_with("BLES00001: "), "{texts:?}");
        assert!(texts[1].starts_with("BLES00002: "), "{texts:?}");
        assert_eq!(app.v.search_promises.len(), 1, "the unfinished search keeps running");
        assert_eq!(app.v.search_promises[0].0, "BLES00003");
    }

    #[test]
    fn second_batch_joins_a_running_one() {
        // A second batch used to replace the first, dropping its queued serials (RPSN-0099).
        let mut v = VolatileData::default();
        assert_eq!(v.enqueue_batch_search(["BLES00001".to_string(), "BLES00002".to_string()]), 2);

        // The first serial finishes with an error while the second is still queued.
        let first = v.batch_search_queue.pop_front().expect("two serials were queued");
        v.batch_search_completed += 1;
        v.batch_search_errors.push((first, "no updates available".to_string()));

        assert_eq!(v.enqueue_batch_search(["BLES00003".to_string()]), 1);
        assert_eq!(v.batch_search_queue, ["BLES00002", "BLES00003"]);
        assert_eq!(v.batch_search_total, 3);
        assert_eq!(v.batch_search_completed, 1);
        assert_eq!(v.batch_search_errors.len(), 1);
    }
}
