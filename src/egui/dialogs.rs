//! The About and folder-migration sections of the Settings page.

use super::*;

impl UpdatesApp {
    pub(super) fn draw_about_section(&mut self, ui: &mut egui::Ui) {
        ui.label(tr!("dialogs-version", version = env!("CARGO_PKG_VERSION")));
        ui.label(tr!("dialogs-tagline"));
        ui.hyperlink_to(tr!("dialogs-github"), "https://github.com/milnet01/rusty-psn");
        ui.add_space(6.0);
        // The README's Disclaimer section says the same (RPSN-0166).
        ui.label(tr!("dialogs-disclaimer"));
    }

    /// Report a finished migration, even when its dialog was closed while it ran.
    pub(super) fn poll_migration(&mut self) {
        if let Some(promise) = &self.v.migration_promise {
            if let Some(&(migrated, skipped)) = promise.ready() {
                if skipped > 0 {
                    let msg = tr!(
                        "dialogs-migration-done-skipped",
                        migrated = migrated.to_string(),
                        skipped = skipped.to_string()
                    );
                    self.show_notifications(&msg, MessageLevel::Warning);
                } else {
                    let msg = tr!("dialogs-migration-done", migrated = migrated.to_string());
                    self.show_notifications(&msg, MessageLevel::Success);
                }
                self.v.migration_promise = None;
                self.v.migration_listed = false;
            }
        }
    }

    /// Moving download folders made under an older layout into the current one. The folders
    /// are listed first, and nothing moves until the user presses Migrate.
    pub(super) fn draw_migration_section(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            if self.v.migration_promise.is_none() && ui.button(tr!("dialogs-migration-look")).clicked() {
                self.v.migration_preview =
                    crate::utils::detect_migratable_directories(&self.settings.pkg_download_path, self.settings.directory_format);
                self.v.migration_scan_root = self.settings.pkg_download_path.clone();
                self.v.migration_listed = true;
            }

            if self.v.migration_promise.is_some() {
                ui.label(tr!("dialogs-migration-running"));
                ui.spinner();
                return;
            }

            if !self.v.migration_listed {
                return;
            }

            if self.v.migration_preview.is_empty() {
                ui.label(tr!("dialogs-migration-none"));
                ui.label(tr!(
                    "dialogs-migration-current-format",
                    format = self.settings.directory_format.to_string()
                ));
                return;
            }

            ui.label(tr!(
                "dialogs-migration-found",
                count = self.v.migration_preview.len().to_string(),
                format = self.settings.directory_format.to_string()
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
                if ui.button(tr!("dialogs-migrate")).clicked() {
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
                if ui.button(tr!("common-cancel")).clicked() {
                    self.v.migration_listed = false;
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_readme_carries_the_about_disclaimer() {
        let words = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
        let readme = words(include_str!("../../README.md"));
        assert!(
            readme.contains(&words(&tr!("dialogs-disclaimer"))),
            "README.md's Disclaimer section differs from dialogs-disclaimer in i18n/en/rusty-psn.ftl"
        );
    }
}
