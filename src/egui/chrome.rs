//! The menu bar, the status bar, and keyboard shortcuts.

use super::*;

impl UpdatesApp {
    pub(super) fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        let mut cancel_settings = false;
        ctx.input(|i| {
            if i.modifiers.command && i.key_pressed(egui::Key::F) {
                self.v.focus_search_bar = true;
            }
            if i.key_pressed(egui::Key::Escape) {
                // Applying the theme needs the context, which this closure holds.
                cancel_settings = self.v.show_settings_window;
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
        if cancel_settings {
            self.cancel_settings_window(ctx);
        }
    }

    pub(super) fn draw_menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("menu_bar").show(ui, |ui| {
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
                            self.adopt_detected_rpcs3(installation);
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
                        self.v.migration_scan_root = self.settings.pkg_download_path.clone();
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

    pub(super) fn draw_status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status_bar").show(ui, |ui| {
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
                            let status = download
                                .progress_status
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
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
}
