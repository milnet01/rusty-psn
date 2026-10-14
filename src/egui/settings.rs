//! The settings window.

use super::*;

impl UpdatesApp {
    pub(super) fn draw_settings_window(&mut self, ctx: &egui::Context) {
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
                        self.start_rpcs3_detect(ui.ctx(), DetectFor::Settings);
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
            self.cancel_settings_window(ctx);
        }
    }

    /// Close the settings window without saving, undoing a previewed theme.
    pub(super) fn cancel_settings_window(&mut self, ctx: &egui::Context) {
        self.v.show_settings_window = false;
        self.v.settings_dirty = false;
        self.settings.theme.apply(ctx);
    }
}
