//! Small dialogs: hash mismatch, about, and folder migration.

use super::*;

impl UpdatesApp {
    pub(super) fn draw_hash_mismatch_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("File integrity check failed").collapsible(false).fixed_size([550.0, 100.0]).show(ctx, | ui | {
            ui.vertical_centered(| ui | {
                ui.label(egui::RichText::new("\u{26A0} The integrity check for a downloaded file failed.").color(StatusKind::Attention.color(ui.visuals())).heading());
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

    pub(super) fn draw_about_window(&mut self, ctx: &egui::Context) {
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

    /// Report a finished migration, even when its dialog was closed while it ran.
    pub(super) fn poll_migration(&mut self) {
        if let Some(promise) = &self.v.migration_promise {
            if let Some(&(migrated, skipped)) = promise.ready() {
                if skipped > 0 {
                    let msg = format!(
                        "Migration: {migrated} folder(s) migrated, {skipped} skipped. The log file says why each was skipped."
                    );
                    self.show_notifications(&msg, ToastLevel::Warning);
                } else {
                    let msg = format!("Migration complete: {migrated} folder(s) migrated.");
                    self.show_notifications(&msg, ToastLevel::Success);
                }
                self.v.migration_promise = None;
                self.v.show_migration_dialog = false;
            }
        }
    }

    pub(super) fn draw_migration_dialog(&mut self, ctx: &egui::Context) {
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
                        let new_display = new.strip_prefix(&self.v.migration_scan_root).unwrap_or(new);
                        ui.label(format!("  {} → {}", old_name, new_display.display()));
                    }
                });

                ui.add_space(5.0);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Migrate").clicked() {
                        let migrations = self.v.migration_preview.clone();
                        let download_path = self.v.migration_scan_root.clone();
                        let ctx = ui.ctx().clone();
                        let _guard = self
                            .v
                            .rt_handle
                            .as_ref()
                            .expect("rt_handle set in UpdatesApp::new before any UI code runs")
                            .enter();
                        self.v.migration_promise = Some(Promise::spawn_async(async move {
                            let outcome = crate::utils::migrate_directories(&migrations, &download_path).await;
                            ctx.request_repaint();
                            outcome
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
}
