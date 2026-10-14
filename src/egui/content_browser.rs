//! The content browser window: local patches and DLC, and installing them.

use super::*;

impl UpdatesApp {
    pub(super) fn draw_content_browser_window(&mut self, ctx: &egui::Context) {
        let mut show = self.v.show_content_browser_window;
        egui::Window::new("Content Browser")
            .open(&mut show)
            .resizable(true)
            .default_size([650.0, 500.0])
            .show(ctx, |ui| {
                if self.settings.content_directory.is_none() {
                    ui.label("No content directory configured. Set it in Settings > Content Library.");
                    if ui.button("Open Settings").clicked() {
                        self.v.modified_settings = self.settings.clone();
                        self.v.show_settings_window = true;
                    }
                    return;
                }

                let has_rpcs3 = self.settings.rpcs3_executable.is_some();

                // Toolbar
                ui.horizontal(|ui| {
                    ui.label("Filter:");
                    ui.text_edit_singleline(&mut self.v.content_filter);
                    if ui.button("Rescan").clicked() {
                        if let Some(ref dir) = self.settings.content_directory {
                            self.v.content_library = Some(crate::content::scan_content_directory(dir));
                        }
                    }
                });
                ui.separator();

                let Some(ref library) = self.v.content_library else {
                    ui.label("Loading...");
                    return;
                };

                let filter_lower = self.v.content_filter.to_lowercase();
                let filtered_games: Vec<usize> = library
                    .games
                    .iter()
                    .enumerate()
                    .filter(|(_, g)| filter_lower.is_empty() || g.name.to_lowercase().contains(&filter_lower))
                    .map(|(i, _)| i)
                    .collect();

                ui.label(format!("{} game(s) shown (of {})", filtered_games.len(), library.games.len()));
                ui.add_space(3.0);

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for &game_idx in &filtered_games {
                        let game = &library.games[game_idx];
                        let id = egui::Id::new(("content_game", game_idx));
                        egui::CollapsingHeader::new(egui::RichText::new(&game.name).strong())
                            .id_salt(id)
                            .show(ui, |ui| {
                                // Patches section
                                if !game.patches.is_empty() {
                                    ui.label(egui::RichText::new(format!("Patches ({})", game.patches.len())).underline());
                                    for patch in &game.patches {
                                        ui.horizontal(|ui| {
                                            ui.label(format!("  {} / {}", patch.serial, patch.pkg_filename));
                                            let patch_key = patch.pkg_path.to_string_lossy().to_string();
                                            let is_installed = self.v.content_installed_items.contains(&patch_key);
                                            let is_installing =
                                                self.v.content_install_promises.iter().any(|(k, _)| *k == patch_key);

                                            if is_installed {
                                                ui.label(
                                                    egui::RichText::new("Installed").color(egui::Rgba::from_rgb(0.0, 1.0, 0.0)),
                                                );
                                            } else if is_installing {
                                                ui.label(
                                                    egui::RichText::new("Installing...")
                                                        .color(egui::Rgba::from_rgb(1.0, 0.8, 0.0)),
                                                );
                                            } else if has_rpcs3 && ui.small_button("Install").clicked() {
                                                let exe =
                                                    self.settings.rpcs3_executable.clone().expect("has_rpcs3 checked it is set");
                                                let pkg = patch.pkg_path.clone();
                                                let key = patch_key.clone();
                                                let rt = self
                                                    .v
                                                    .rt_handle
                                                    .clone()
                                                    .expect("rt_handle set in UpdatesApp::new before any UI code runs");
                                                let _guard = rt.enter();
                                                let promise = Promise::spawn_async(async move {
                                                    crate::rpcs3::install_pkg(&exe, &pkg)
                                                        .await
                                                        .map_err(|e| crate::content::ContentError::InstallFailed(e.to_string()))
                                                });
                                                self.v.content_install_promises.push((key, promise));
                                            }
                                        });
                                    }
                                    ui.add_space(3.0);
                                }

                                // DLC section
                                if !game.dlc.is_empty() {
                                    ui.label(egui::RichText::new(format!("DLC ({})", game.dlc.len())).underline());
                                    for dlc_item in &game.dlc {
                                        ui.horizontal(|ui| {
                                            ui.label(format!("  {} ({})", dlc_item.zip_filename, ByteSize::b(dlc_item.size)));
                                            let dlc_key = dlc_item.zip_path.to_string_lossy().to_string();
                                            let is_installed = self.v.content_installed_items.contains(&dlc_key);
                                            let is_installing =
                                                self.v.content_install_promises.iter().any(|(k, _)| *k == dlc_key);

                                            if is_installed {
                                                ui.label(
                                                    egui::RichText::new("Installed").color(egui::Rgba::from_rgb(0.0, 1.0, 0.0)),
                                                );
                                            } else if is_installing {
                                                ui.label(
                                                    egui::RichText::new("Installing...")
                                                        .color(egui::Rgba::from_rgb(1.0, 0.8, 0.0)),
                                                );
                                            } else if has_rpcs3 && ui.small_button("Install").clicked() {
                                                let exe =
                                                    self.settings.rpcs3_executable.clone().expect("has_rpcs3 checked it is set");
                                                let zip = dlc_item.zip_path.clone();
                                                let key = dlc_key.clone();
                                                let rt = self
                                                    .v
                                                    .rt_handle
                                                    .clone()
                                                    .expect("rt_handle set in UpdatesApp::new before any UI code runs");
                                                let _guard = rt.enter();
                                                let promise = Promise::spawn_async(async move {
                                                    crate::content::install_dlc_zip(&exe, &zip).await
                                                });
                                                self.v.content_install_promises.push((key, promise));
                                            }
                                        });
                                    }
                                }
                            });
                    }
                });
            });
        if !show {
            self.v.show_content_browser_window = false;
        }
    }

    pub(super) fn handle_content_install_promises(&mut self, toasts: &mut Vec<(String, ToastLevel)>) {
        let mut i = 0;
        while i < self.v.content_install_promises.len() {
            if self.v.content_install_promises[i].1.ready().is_some() {
                let (key, promise) = self.v.content_install_promises.remove(i);
                match promise.block_and_take() {
                    Ok(()) => {
                        // Extract a short display name from the key (file path)
                        let display = Path::new(&key)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| key.clone());
                        toasts.push((format!("Installed: {display}"), ToastLevel::Success));
                        self.v.content_installed_items.insert(key);
                    }
                    Err(e) => {
                        toasts.push((format!("Install failed: {e}"), ToastLevel::Error));
                    }
                }
            } else {
                i += 1;
            }
        }
    }
}
