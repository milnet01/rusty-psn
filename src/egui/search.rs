//! Searching: the search bar, single and batch searches, and batch import.

use super::*;

impl UpdatesApp {
    pub(super) fn handle_search_promise(&mut self, toasts: &mut Vec<(String, ToastLevel)>) -> Option<()> {
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

    pub(super) fn handle_batch_search_queue(&mut self, toasts: &mut Vec<(String, ToastLevel)>) {
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
}
