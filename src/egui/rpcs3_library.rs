//! The RPCS3 library window: scanning the library and installing updates.

use super::*;

impl UpdatesApp {
    /// Use an auto-detected RPCS3: its config folder, and its executable when none is set.
    pub(super) fn adopt_detected_rpcs3(&mut self, installation: crate::rpcs3::Rpcs3Installation) {
        self.settings.rpcs3_config_dir = Some(installation.config_dir);
        if self.settings.rpcs3_executable.is_none() {
            self.settings.rpcs3_executable = installation.executable;
        }
    }

    /// What "Scan RPCS3 Library" does, from the toolbar or the Tools menu: scan the
    /// configured RPCS3, else look for one and scan it when found.
    pub(super) fn scan_or_detect_rpcs3(&mut self, ctx: &egui::Context) {
        if self.settings.rpcs3_config_dir.is_some() {
            self.trigger_rpcs3_scan();
        } else {
            self.start_rpcs3_detect(ctx, DetectFor::Scan);
        }
    }

    /// Look for RPCS3 in the background: the search reads folders and runs programs,
    /// which would freeze the window. A second request while one runs is ignored.
    pub(super) fn start_rpcs3_detect(&mut self, ctx: &egui::Context, purpose: DetectFor) {
        if self.v.rpcs3_detect_promise.is_some() {
            return;
        }
        let _guard = self
            .v
            .rt_handle
            .as_ref()
            .expect("rt_handle set in UpdatesApp::new before any UI code runs")
            .enter();
        let ctx = ctx.clone();
        let promise = Promise::spawn_async(async move {
            // A panicked search found nothing.
            let found = tokio::task::spawn_blocking(crate::rpcs3::auto_detect::detect_rpcs3)
                .await
                .ok()
                .flatten();
            ctx.request_repaint();
            found
        });
        self.v.rpcs3_detect_promise = Some((purpose, promise));
    }

    pub(super) fn handle_rpcs3_detect_promise(&mut self, toasts: &mut Vec<(String, ToastLevel)>) {
        if !self.v.rpcs3_detect_promise.as_ref().is_some_and(|(_, p)| p.ready().is_some()) {
            return;
        }
        let (purpose, promise) = self.v.rpcs3_detect_promise.take().expect("checked above");
        match (purpose, promise.block_and_take()) {
            (DetectFor::Scan, Some(installation)) => {
                self.adopt_detected_rpcs3(installation);
                self.trigger_rpcs3_scan();
                toasts.push(("RPCS3 auto-detected and scan started.".to_string(), ToastLevel::Info));
            }
            (DetectFor::Scan, None) => toasts.push((
                "RPCS3 not found. Go to Settings to configure the RPCS3 path.".to_string(),
                ToastLevel::Warning,
            )),
            // The Settings window was shut while the search ran: nothing is waiting for it.
            (DetectFor::Settings, _) if !self.v.show_settings_window => {}
            (DetectFor::Settings, Some(installation)) => {
                self.v.settings_dirty = true;
                self.v.modified_settings.rpcs3_config_dir = Some(installation.config_dir);
                if self.v.modified_settings.rpcs3_executable.is_none() {
                    self.v.modified_settings.rpcs3_executable = installation.executable;
                }
            }
            (DetectFor::Settings, None) => toasts.push((
                "RPCS3 not found. Please configure the path manually.".to_string(),
                ToastLevel::Warning,
            )),
        }
    }

    pub(super) fn trigger_rpcs3_scan(&mut self) {
        let config_dir = match &self.settings.rpcs3_config_dir {
            Some(dir) => dir.clone(),
            None => {
                self.show_notifications("RPCS3 not configured. Set the path in Settings.", ToastLevel::Error);
                return;
            }
        };

        let dev_hdd0_path = crate::rpcs3::config::resolve_dev_hdd0(&config_dir);

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
                let search_count = self.v.enqueue_batch_search(games.iter().map(|g| g.serial.clone()));

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

    /// The check an update install asks for: the expected version read back from dev_hdd0,
    /// or nothing when the RPCS3 folder is not set (docs/specs/RPSN-0137-headless-confirmed-install.md § 4.3).
    pub(super) fn update_install_check(
        rpcs3_config_dir: Option<&std::path::Path>,
        serial: &str,
        version: Option<String>,
    ) -> crate::rpcs3::InstallCheck {
        match rpcs3_config_dir {
            Some(dir) => crate::rpcs3::InstallCheck::AppVersion {
                dev_hdd0: crate::rpcs3::config::resolve_dev_hdd0(dir),
                serial: serial.to_string(),
                version,
            },
            None => crate::rpcs3::InstallCheck::Unconfirmed,
        }
    }

    pub(super) fn handle_rpcs3_install_promises(&mut self, toasts: &mut Vec<(String, ToastLevel)>) {
        let mut i = 0;
        while i < self.v.rpcs3_install_promises.len() {
            if self.v.rpcs3_install_promises[i].2.ready().is_some() {
                let (tid, pid, promise) = self.v.rpcs3_install_promises.remove(i);
                match promise.block_and_take() {
                    Ok(crate::rpcs3::InstallOutcome::Confirmed) => {
                        toasts.push((format!("Installed {tid} to RPCS3"), ToastLevel::Success));
                        self.v.rpcs3_installed_pkgs.insert((tid, pid));
                    }
                    // No pill, so the Install button stays for a retry once the folder is set.
                    Ok(crate::rpcs3::InstallOutcome::Unconfirmed) => toasts.push((
                        format!(
                            "RPCS3 finished installing {tid}, but rusty-psn could not check the result. Set the RPCS3 folder in Settings to check it."
                        ),
                        ToastLevel::Warning,
                    )),
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
