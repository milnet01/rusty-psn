//! The RPCS3 library window: scanning the library and installing updates.

use super::*;

impl UpdatesApp {
    pub(super) fn trigger_rpcs3_scan(&mut self) {
        let config_dir = match &self.settings.rpcs3_config_dir {
            Some(dir) => dir.clone(),
            None => {
                self.show_notifications("RPCS3 not configured. Set the path in Settings.", ToastLevel::Error);
                return;
            }
        };

        // Resolve dev_hdd0 path from vfs.yml.
        let vfs_path = config_dir.join("vfs.yml");
        let dev_hdd0_path = if vfs_path.exists() {
            match crate::rpcs3::config::parse_vfs_yml(&vfs_path, &config_dir) {
                Ok(vfs) => vfs.dev_hdd0_path,
                Err(e) => {
                    warn!("Failed to parse vfs.yml: {e}, using default");
                    config_dir.join("dev_hdd0")
                }
            }
        } else {
            config_dir.join("dev_hdd0")
        };

        let installation = crate::rpcs3::Rpcs3Installation {
            config_dir,
            dev_hdd0_path,
            executable: None, // Manual config path — executable detected separately
        };

        info!("Starting RPCS3 library scan");
        let _guard = self
            .v
            .rt_handle
            .as_ref()
            .expect("rt_handle set in UpdatesApp::new before any UI code runs")
            .enter();
        self.v.rpcs3_scan_promise = Some(Promise::spawn_async(async move {
            // scan_library is sync but we wrap it in async for consistency with the promise pattern.
            crate::rpcs3::scan_library(&installation)
        }));
    }

    pub(super) fn handle_rpcs3_scan_promise(&mut self, toasts: &mut Vec<(String, ToastLevel)>) {
        let is_ready = self.v.rpcs3_scan_promise.as_ref().is_some_and(|p| p.ready().is_some());

        if !is_ready {
            return;
        }

        let promise = self.v.rpcs3_scan_promise.take().expect("checked above");
        match promise.block_and_take() {
            Ok(games) => {
                let count = games.len();
                info!("RPCS3 scan found {count} installed games");

                // Feed all serials into the batch search queue for PSN update checking.
                let serials: Vec<String> = games
                    .iter()
                    .map(|g| g.serial.clone())
                    .filter(|s| !self.v.update_results.iter().any(|u| u.title_id == *s))
                    .filter(|s| !self.v.batch_search_queue.iter().any(|q| q == s))
                    .filter(|s| !self.v.batch_search_active.iter().any(|(id, _)| id == s))
                    .collect();

                let search_count = serials.len();
                self.v.batch_search_total = search_count;
                self.v.batch_search_completed = 0;
                self.v.batch_search_errors.clear();
                self.v.batch_search_queue = serials.into();

                self.v.rpcs3_installed_games = games;
                self.v.show_rpcs3_library_window = true;

                toasts.push((
                    format!("RPCS3 scan: {count} game(s) found, checking {search_count} for updates..."),
                    ToastLevel::Info,
                ));
            }
            Err(e) => {
                toasts.push((format!("RPCS3 scan failed: {e}"), ToastLevel::Error));
            }
        }
    }

    pub(super) fn draw_rpcs3_library_window(&mut self, ctx: &egui::Context) {
        let mut show = self.v.show_rpcs3_library_window;

        // Pre-compute outdated games list outside the window closure to minimize cloning.
        // Only the title_id/title/pkg tuples needed for downloading are cloned, not the full lists.
        let game_count = self.v.rpcs3_installed_games.len();
        let games_to_update: Vec<_> = self
            .v
            .rpcs3_installed_games
            .iter()
            .filter_map(|game| {
                let installed_ver = game.installed_version.as_ref()?;
                let update = self.v.update_results.iter().find(|u| u.title_id == game.serial)?;
                let latest_pkg = update.packages.last()?;
                if crate::rpcs3::compare_versions(installed_ver, &latest_pkg.version) == std::cmp::Ordering::Less {
                    Some((update.title_id.clone(), update.title(), latest_pkg.clone()))
                } else {
                    None
                }
            })
            .collect();
        let outdated_count = games_to_update.len();

        // Collect download actions from the window, then apply them after the closure.
        let mut download_actions: Vec<(String, String, PackageInfo)> = Vec::new();

        egui::Window::new("RPCS3 Library")
            .open(&mut show)
            .resizable(true)
            .default_size([700.0, 400.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{game_count} game(s) installed, {outdated_count} with updates available"
                    ));

                    if outdated_count > 0 && ui.button("Update All Outdated").clicked() {
                        download_actions = games_to_update;
                    }
                });

                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Grid::new("rpcs3_library_grid")
                        .striped(true)
                        .min_col_width(60.0)
                        .show(ui, |ui| {
                            // Header row.
                            ui.strong("Serial");
                            ui.strong("Title");
                            ui.strong("Installed");
                            ui.strong("Available");
                            ui.strong("Status");
                            ui.end_row();

                            for i in 0..self.v.rpcs3_installed_games.len() {
                                let game = &self.v.rpcs3_installed_games[i];
                                ui.label(&game.serial);
                                ui.label(&game.title);

                                let installed_ver = game.installed_version.as_deref().unwrap_or("--");
                                ui.label(installed_ver);

                                // Look up the PSN update info for this serial.
                                let update_info = self.v.update_results.iter().find(|u| u.title_id == game.serial);
                                let available_ver = update_info
                                    .and_then(|u| u.packages.last())
                                    .map(|p| p.version.as_str())
                                    .unwrap_or("--");
                                ui.label(available_ver);

                                // Status column.
                                if let (Some(inst), Some(update)) = (&game.installed_version, update_info) {
                                    if let Some(latest) = update.packages.last() {
                                        match crate::rpcs3::compare_versions(inst, &latest.version) {
                                            std::cmp::Ordering::Less => {
                                                ui.label(
                                                    egui::RichText::new("Update available")
                                                        .color(egui::Rgba::from_rgb(1.0, 1.0, 0.6)),
                                                );
                                            }
                                            _ => {
                                                ui.label(
                                                    egui::RichText::new("Up to date").color(egui::Rgba::from_rgb(0.0, 1.0, 0.0)),
                                                );
                                            }
                                        }
                                    } else {
                                        ui.label("No updates");
                                    }
                                } else if update_info.is_none()
                                    && (self.v.batch_search_active.iter().any(|(s, _)| s == &game.serial)
                                        || self.v.batch_search_queue.iter().any(|s| s == &game.serial))
                                {
                                    ui.label(egui::RichText::new("Checking...").color(egui::Rgba::from_rgb(0.6, 0.6, 0.6)));
                                } else {
                                    ui.label("--");
                                }

                                ui.end_row();
                            }
                        });
                });
            });

        // Apply queued download actions after the window closure.
        for (title_id, title, pkg) in download_actions {
            self.queue_download_no_ui(title_id, title, pkg);
        }

        if !show {
            self.v.show_rpcs3_library_window = false;
        }
    }

    pub(super) fn handle_rpcs3_install_promises(&mut self, toasts: &mut Vec<(String, ToastLevel)>) {
        let mut i = 0;
        while i < self.v.rpcs3_install_promises.len() {
            if self.v.rpcs3_install_promises[i].2.ready().is_some() {
                let (tid, pid, promise) = self.v.rpcs3_install_promises.remove(i);
                match promise.block_and_take() {
                    Ok(()) => {
                        toasts.push((format!("Installed {tid} to RPCS3"), ToastLevel::Success));
                        self.v.rpcs3_installed_pkgs.insert((tid, pid));
                    }
                    Err(e) => {
                        toasts.push((format!("RPCS3 install failed: {e}"), ToastLevel::Error));
                    }
                }
            } else {
                i += 1;
            }
        }
    }
}
