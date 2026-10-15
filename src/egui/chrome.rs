//! The sidebar and its pages, the status bar, and keyboard shortcuts.

use super::*;

/// The pages the sidebar offers (RPSN-0157). Each takes the main window's central area; nothing
/// floats above it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Search,
    Rpcs3Library,
    PatchesAndDlc,
    Firmware,
    History,
    Settings,
}

impl Page {
    /// The pages listed at the top of the sidebar, in order. Settings sits apart, at its foot.
    const TOP: [Page; 5] = [
        Page::Search,
        Page::Rpcs3Library,
        Page::PatchesAndDlc,
        Page::Firmware,
        Page::History,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Page::Search => "Search",
            Page::Rpcs3Library => "RPCS3 library",
            Page::PatchesAndDlc => "Patches and DLC",
            Page::Firmware => "Firmware",
            Page::History => "History",
            Page::Settings => "Settings",
        }
    }
}

impl UpdatesApp {
    /// Switch to `page`. Leaving Settings discards unsaved changes, as closing its window did;
    /// entering it starts the edit from the saved settings.
    pub(super) fn go_to(&mut self, page: Page, ctx: &egui::Context) {
        if page == self.v.page {
            return;
        }
        if self.v.page == Page::Settings {
            self.cancel_settings(ctx);
        }
        match page {
            Page::Settings => self.v.modified_settings = self.settings.clone(),
            Page::PatchesAndDlc => self.start_content_scan(),
            _ => {}
        }
        self.v.page = page;
    }

    pub(super) fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        let (find, escape, save) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(egui::Key::F),
                i.key_pressed(egui::Key::Escape),
                i.modifiers.command && i.key_pressed(egui::Key::S),
            )
        });
        if find {
            self.go_to(Page::Search, ctx);
            self.v.focus_search_bar = true;
        }
        if escape {
            self.go_to(Page::Search, ctx);
        }
        if save {
            if self.v.page == Page::Settings {
                self.save_settings();
            } else {
                self.go_to(Page::Settings, ctx);
            }
        }
    }

    pub(super) fn draw_sidebar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        egui::Panel::left("sidebar")
            .resizable(false)
            .exact_size(170.0)
            .show(ui, |ui| {
                let mut chosen = None;
                let mut entry = |ui: &mut egui::Ui, page: Page, current: Page| {
                    let button =
                        egui::Button::selectable(page == current, page.label()).min_size(egui::vec2(ui.available_width(), 0.0));
                    if ui.add(button).clicked() {
                        chosen = Some(page);
                    }
                };

                ui.add_space(4.0);
                for page in Page::TOP {
                    entry(ui, page, self.v.page);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.add_space(4.0);
                    entry(ui, Page::Settings, self.v.page);
                });

                if let Some(page) = chosen {
                    self.go_to(page, &ctx);
                }
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
