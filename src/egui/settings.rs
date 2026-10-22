//! The Settings page.

use super::*;

impl UpdatesApp {
    pub(super) fn draw_settings_page(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            // The fixed bar: Save and Undo work only while something is unsaved, and the words
            // after them say which it is (RPSN-0157 step 4).
            ui.horizontal(|ui| {
                let dirty = self.v.settings_dirty;
                if ui.add_enabled(dirty, egui::Button::new(tr!("settings-save"))).clicked() {
                    self.save_settings();
                }
                if ui.add_enabled(dirty, egui::Button::new(tr!("settings-undo"))).clicked() {
                    self.cancel_settings(ui.ctx());
                    self.v.modified_settings = self.settings.clone();
                }
                if ui.button(tr!("settings-restore-defaults")).clicked() {
                    self.restore_default_settings(ui.ctx());
                }
                ui.separator();
                if dirty {
                    status_pill(ui, &tr!("settings-unsaved"), StatusKind::Attention);
                } else {
                    status_pill(ui, &tr!("settings-all-saved"), StatusKind::Done);
                }
            });

            ui.separator();

            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.draw_settings_fields(ui);
                    ui.add_space(10.0);
                    egui::CollapsingHeader::new(tr!("settings-migrate-heading")).show(ui, |ui| self.draw_migration_section(ui));
                    egui::CollapsingHeader::new(tr!("settings-about-heading")).show(ui, |ui| self.draw_about_section(ui));
                });
            });
        });
    }

    /// Put the defaults in front of the user as unsaved changes, previewing their theme and text
    /// size. Nothing is saved until Save changes; Undo changes brings the saved settings back.
    pub(super) fn restore_default_settings(&mut self, ctx: &egui::Context) {
        self.v.modified_settings = AppSettings::default();
        self.v.modified_settings.apply_appearance(ctx);
        self.v.settings_dirty = true;
    }

    pub(super) fn save_settings(&mut self) {
        self.v.settings_dirty = false;
        self.settings = self.v.modified_settings.clone();
    }

    /// Drop unsaved changes, undoing a previewed theme or text size.
    pub(super) fn cancel_settings(&mut self, ctx: &egui::Context) {
        self.v.settings_dirty = false;
        self.settings.apply_appearance(ctx);
    }

    /// The settings themselves, above the Save and Discard buttons.
    fn draw_settings_fields(&mut self, ui: &mut egui::Ui) {
        let mut current_download_path = self.v.modified_settings.pkg_download_path.to_string_lossy().to_string();

        ui.heading(tr!("settings-downloads-heading"));
        ui.add_space(3.0);

        ui.label(tr!("settings-download-path"));
        ui.horizontal(|ui| {
            ui.add_enabled_ui(false, |ui| {
                ui.text_edit_singleline(&mut current_download_path);
            });

            if ui.button(tr!("settings-pick-folder")).clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    self.v.settings_dirty = true;
                    self.v.modified_settings.pkg_download_path = path;
                }
            }

            if ui.button(tr!("settings-reset")).clicked() {
                self.v.settings_dirty = true;
                self.v.modified_settings.pkg_download_path = default_download_path();
            }
        });

        ui.add_space(5.0);

        ui.label(tr!("settings-folder-structure"));
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

        ui.label(tr!("settings-max-downloads"));
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
        ui.heading(tr!("settings-appearance-heading"));
        ui.add_space(3.0);

        ui.label(tr!("settings-theme"));
        let current_theme = self.v.modified_settings.theme;
        egui::ComboBox::from_id_salt("theme_selector")
            .selected_text(current_theme.label())
            .width(200.0)
            .show_ui(ui, |ui| {
                for theme in APP_THEMES {
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

        ui.add_space(5.0);

        ui.label(tr!("settings-text-size"));
        let current_size = self.v.modified_settings.text_size_percent;
        egui::ComboBox::from_id_salt("text_size_selector")
            .selected_text(text_size_label(current_size))
            .width(200.0)
            .show_ui(ui, |ui| {
                for size in TEXT_SIZES {
                    if ui
                        .selectable_value(&mut self.v.modified_settings.text_size_percent, size, text_size_label(size))
                        .changed()
                    {
                        self.v.settings_dirty = true;
                        // Apply the size immediately for live preview.
                        ui.ctx().set_zoom_factor(size as f32 / 100.0);
                    }
                }
            });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(3.0);
        ui.heading(tr!("settings-notifications-heading"));
        ui.add_space(3.0);

        if ui
            .checkbox(
                &mut self.v.modified_settings.show_notifications,
                tr!("settings-show-notifications"),
            )
            .changed()
        {
            self.v.settings_dirty = true;
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(3.0);
        ui.heading(tr!("settings-rpcs3-heading"));
        ui.add_space(3.0);

        ui.label(tr!("settings-rpcs3-config-dir"));
        ui.horizontal(|ui| {
            let rpcs3_display = self
                .v
                .modified_settings
                .rpcs3_config_dir
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| tr!("settings-not-configured"));
            let mut display_text = rpcs3_display;
            ui.add_enabled_ui(false, |ui| {
                ui.text_edit_singleline(&mut display_text);
            });

            if ui.button(tr!("settings-browse")).clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    self.v.settings_dirty = true;
                    self.v.modified_settings.rpcs3_config_dir = Some(path);
                }
            }

            if ui.button(tr!("settings-auto-detect")).clicked() {
                self.start_rpcs3_detect(ui.ctx(), DetectFor::Settings);
            }
        });

        // Show status.
        if let Some(dir) = &self.v.modified_settings.rpcs3_config_dir {
            if dir.join("games.yml").exists() {
                status_pill(ui, &tr!("settings-rpcs3-config-found"), StatusKind::Done);
            } else {
                status_pill(ui, &tr!("settings-games-yml-missing"), StatusKind::Attention);
            }
        } else {
            ui.label(tr!("settings-status-not-configured"));
        }

        ui.add_space(5.0);
        ui.label(tr!("settings-rpcs3-executable"));
        ui.horizontal(|ui| {
            let exe_display = self
                .v
                .modified_settings
                .rpcs3_executable
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| tr!("settings-rpcs3-executable-unset"));
            let mut display_text = exe_display;
            ui.add_enabled_ui(false, |ui| {
                ui.text_edit_singleline(&mut display_text);
            });

            if ui.button(tr!("settings-browse")).clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    self.v.settings_dirty = true;
                    self.v.modified_settings.rpcs3_executable = Some(path);
                }
            }

            if ui.button(tr!("common-clear")).clicked() {
                self.v.settings_dirty = true;
                self.v.modified_settings.rpcs3_executable = None;
            }
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(3.0);
        ui.heading(tr!("settings-content-heading"));
        ui.add_space(3.0);

        ui.label(tr!("settings-content-dir"));
        ui.horizontal(|ui| {
            let content_display = self
                .v
                .modified_settings
                .content_directory
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| tr!("settings-not-configured"));
            let mut display_text = content_display;
            ui.add_enabled_ui(false, |ui| {
                ui.text_edit_singleline(&mut display_text);
            });

            if ui.button(tr!("settings-browse")).clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    self.v.settings_dirty = true;
                    self.v.modified_settings.content_directory = Some(path);
                }
            }

            if ui.button(tr!("common-clear")).clicked() {
                self.v.settings_dirty = true;
                self.v.modified_settings.content_directory = None;
            }
        });
    }
}
