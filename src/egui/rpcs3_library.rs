//! The RPCS3 library page: scanning the library and installing updates.

use super::*;

impl UpdatesApp {
    /// Use an auto-detected RPCS3: its config folder, and its executable when none is set.
    pub(super) fn adopt_detected_rpcs3(&mut self, installation: crate::rpcs3::Rpcs3Installation) {
        self.settings.rpcs3_config_dir = Some(installation.config_dir);
        if self.settings.rpcs3_executable.is_none() {
            self.settings.rpcs3_executable = installation.executable;
        }
    }

    /// What "Scan RPCS3 library" does, on the RPCS3 library page: scan the
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

    pub(super) fn handle_rpcs3_detect_promise(&mut self, messages: &mut Vec<(String, MessageLevel)>) {
        if !self.v.rpcs3_detect_promise.as_ref().is_some_and(|(_, p)| p.ready().is_some()) {
            return;
        }
        let (purpose, promise) = self.v.rpcs3_detect_promise.take().expect("checked above");
        match (purpose, promise.block_and_take()) {
            (DetectFor::Scan, Some(installation)) => {
                self.adopt_detected_rpcs3(installation);
                self.trigger_rpcs3_scan();
                messages.push(("RPCS3 auto-detected and scan started.".to_string(), MessageLevel::Info));
            }
            (DetectFor::Scan, None) => messages.push((
                "RPCS3 not found. Go to Settings to configure the RPCS3 path.".to_string(),
                MessageLevel::Warning,
            )),
            // The Settings page was left while the search ran: nothing is waiting for it.
            (DetectFor::Settings, _) if self.v.page != chrome::Page::Settings => {}
            (DetectFor::Settings, Some(installation)) => {
                self.v.settings_dirty = true;
                self.v.modified_settings.rpcs3_config_dir = Some(installation.config_dir);
                if self.v.modified_settings.rpcs3_executable.is_none() {
                    self.v.modified_settings.rpcs3_executable = installation.executable;
                }
            }
            (DetectFor::Settings, None) => messages.push((
                "RPCS3 not found. Please configure the path manually.".to_string(),
                MessageLevel::Warning,
            )),
        }
    }

    pub(super) fn trigger_rpcs3_scan(&mut self) {
        let config_dir = match &self.settings.rpcs3_config_dir {
            Some(dir) => dir.clone(),
            None => {
                self.show_notifications("RPCS3 not configured. Set the path in Settings.", MessageLevel::Error);
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
        let ctx = self.v.egui_ctx.clone();
        self.v.rpcs3_scan_promise = Some(Promise::spawn_async(async move {
            // scan_library reads many files, so it runs on a blocking worker, not an async one.
            let result = tokio::task::spawn_blocking(move || crate::rpcs3::scan_library(&installation))
                .await
                .unwrap_or_else(|e| Err(crate::rpcs3::Rpcs3Error::Io(std::io::Error::other(e))));
            // Show the result now, not at the next mouse move.
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
            result
        }));
    }

    pub(super) fn handle_rpcs3_scan_promise(&mut self, messages: &mut Vec<(String, MessageLevel)>) {
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

                messages.push((
                    format!("RPCS3 scan: {count} game(s) found, checking {search_count} for updates..."),
                    MessageLevel::Info,
                ));
            }
            Err(e) => {
                messages.push((format!("RPCS3 scan failed: {e}"), MessageLevel::Error));
            }
        }
    }

    pub(super) fn draw_rpcs3_library_page(&mut self, ui: &mut egui::Ui) {
        // Pre-compute outdated games list outside the page closure to minimize cloning.
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
                    Some((
                        update.title_id.clone(),
                        update.title(),
                        installed_ver.clone(),
                        update.packages.clone(),
                    ))
                } else {
                    None
                }
            })
            .collect();
        let outdated_count = games_to_update.len();

        // Collect download actions from the page, then apply them after the closure.
        let mut update_all = None;
        let mut scan = false;

        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                scan = ui.button("Scan RPCS3 library").clicked();
                ui.label(format!(
                    "{game_count} game(s) in library, {outdated_count} with updates available"
                ));

                if outdated_count > 0 && ui.button("Update All Outdated").clicked() {
                    update_all = Some(games_to_update);
                }
            });

            ui.horizontal(|ui| {
                ui.label("Show:");
                for filter in LibraryFilter::ALL {
                    ui.selectable_value(&mut self.v.library_filter, filter, filter.label());
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
                            if !self.v.library_filter.shows(self.library_status(game)) {
                                continue;
                            }
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

                            match self.library_status(game) {
                                LibraryStatus::NeedsUpdate => status_pill(ui, "Update available", StatusKind::Attention),
                                LibraryStatus::UpToDate => status_pill(ui, "Up to date", StatusKind::Done),
                                LibraryStatus::NoUpdates => {
                                    ui.label("No updates");
                                }
                                LibraryStatus::Checking => status_pill(ui, "Checking…", StatusKind::Working),
                                LibraryStatus::VersionUnknown => {
                                    ui.label("version unknown");
                                }
                                LibraryStatus::Unknown => {
                                    ui.label("--");
                                }
                            }

                            ui.end_row();
                        }
                    });
            });
        });

        if scan {
            self.scan_or_detect_rpcs3(&ui.ctx().clone());
        }
        // Download, and install where RPCS3 is set, every outdated game's updates (RPSN-0058).
        if let Some(games) = update_all {
            self.start_update_all(games);
        }
    }

    /// Where one installed game stands against what PSN offers.
    pub(super) fn library_status(&self, game: &crate::rpcs3::InstalledGame) -> LibraryStatus {
        let update_info = self.v.update_results.iter().find(|u| u.title_id == game.serial);
        let checking = self.v.batch_search_active.iter().any(|(s, _)| s == &game.serial)
            || self.v.batch_search_queue.iter().any(|s| s == &game.serial);
        match (update_info, update_info.and_then(|u| u.packages.last())) {
            (None, _) if checking => LibraryStatus::Checking,
            (None, _) if self.v.no_update_serials.contains(&game.serial) => LibraryStatus::NoUpdates,
            (None, _) => LibraryStatus::Unknown,
            (Some(_), None) => LibraryStatus::NoUpdates,
            (Some(_), Some(latest)) => match &game.installed_version {
                None => LibraryStatus::VersionUnknown,
                Some(installed) => match crate::rpcs3::compare_versions(installed, &latest.version) {
                    std::cmp::Ordering::Less => LibraryStatus::NeedsUpdate,
                    _ => LibraryStatus::UpToDate,
                },
            },
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

    pub(super) fn handle_rpcs3_install_promises(&mut self, messages: &mut Vec<(String, MessageLevel)>) {
        let mut i = 0;
        while i < self.v.rpcs3_install_promises.len() {
            if self.v.rpcs3_install_promises[i].2.ready().is_some() {
                let (tid, pid, promise) = self.v.rpcs3_install_promises.remove(i);
                match promise.block_and_take() {
                    Ok(crate::rpcs3::InstallOutcome::Confirmed) => {
                        messages.push((format!("Installed {tid} to RPCS3"), MessageLevel::Success));
                        self.v.rpcs3_installed_pkgs.insert((tid, pid));
                    }
                    // No pill, so the Install button stays for a retry once the folder is set.
                    Ok(crate::rpcs3::InstallOutcome::Unconfirmed) => messages.push((
                        format!(
                            "RPCS3 finished installing {tid}, but rusty-psn could not check the result. Set the RPCS3 folder in Settings to check it."
                        ),
                        MessageLevel::Warning,
                    )),
                    Err(e) => {
                        messages.push((format!("RPCS3 install failed: {e}"), MessageLevel::Error));
                    }
                }
            } else {
                i += 1;
            }
        }
    }
}

/// Where an installed game stands; see `UpdatesApp::library_status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LibraryStatus {
    NeedsUpdate,
    UpToDate,
    /// PSN lists no update for it.
    NoUpdates,
    /// Its search is still queued or running.
    Checking,
    /// PSN has an update, but no installed version is on record to compare it with (RPSN-0131).
    VersionUnknown,
    /// Not searched yet, or its search failed.
    Unknown,
}

/// The RPCS3 library page's filter buttons (RPSN-0157 step 6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum LibraryFilter {
    #[default]
    All,
    NeedsUpdate,
    UpToDate,
    NoUpdates,
}

impl LibraryFilter {
    const ALL: [LibraryFilter; 4] = [
        LibraryFilter::All,
        LibraryFilter::NeedsUpdate,
        LibraryFilter::UpToDate,
        LibraryFilter::NoUpdates,
    ];

    fn label(self) -> &'static str {
        match self {
            LibraryFilter::All => "All",
            LibraryFilter::NeedsUpdate => "Needs update",
            LibraryFilter::UpToDate => "Up to date",
            LibraryFilter::NoUpdates => "No updates exist",
        }
    }

    pub(super) fn shows(self, status: LibraryStatus) -> bool {
        match self {
            LibraryFilter::All => true,
            LibraryFilter::NeedsUpdate => status == LibraryStatus::NeedsUpdate,
            LibraryFilter::UpToDate => status == LibraryStatus::UpToDate,
            LibraryFilter::NoUpdates => status == LibraryStatus::NoUpdates,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(serial: &str, installed: Option<&str>) -> crate::rpcs3::InstalledGame {
        crate::rpcs3::InstalledGame {
            serial: serial.to_string(),
            title: String::new(),
            installed_version: installed.map(str::to_string),
            game_path: PathBuf::new(),
        }
    }

    fn update(serial: &str, versions: &[&str]) -> UpdateInfo {
        UpdateInfo {
            title_id: serial.to_string(),
            tag_name: String::new(),
            titles: Vec::new(),
            packages: versions
                .iter()
                .map(|version| PackageInfo {
                    url: String::new(),
                    size: 0,
                    version: version.to_string(),
                    sha1sum: String::new(),
                    hash_whole_file: false,
                    manifest_url: String::new(),
                    offset: 0,
                    part_number: None,
                    min_system_version: None,
                })
                .collect(),
            platform_variant: crate::psn::utils::PlatformVariant::PS3,
        }
    }

    #[test]
    fn each_filter_shows_only_its_games() {
        let mut app = UpdatesApp::default();
        let games = [
            game("OLD", Some("01.00")),
            game("NEW", Some("01.02")),
            game("NONE", Some("01.00")),
            game("FAILED", Some("01.00")),
            game("WAITING", Some("01.00")),
        ];
        app.v.update_results = vec![update("OLD", &["01.02"]), update("NEW", &["01.02"])];
        app.v.no_update_serials.insert("NONE".to_string());
        app.v.batch_search_queue.push_back("WAITING".to_string());

        let shown = |app: &UpdatesApp, filter: LibraryFilter| {
            games
                .iter()
                .filter(|g| filter.shows(app.library_status(g)))
                .map(|g| g.serial.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(shown(&app, LibraryFilter::All).len(), games.len());
        assert_eq!(shown(&app, LibraryFilter::NeedsUpdate), ["OLD"]);
        assert_eq!(shown(&app, LibraryFilter::UpToDate), ["NEW"]);
        assert_eq!(
            shown(&app, LibraryFilter::NoUpdates),
            ["NONE"],
            "a failed search is not 'no updates'"
        );
        assert_eq!(app.library_status(&games[4]), LibraryStatus::Checking);
        assert_eq!(app.library_status(&game("OLD", None)), LibraryStatus::VersionUnknown);
    }
}
