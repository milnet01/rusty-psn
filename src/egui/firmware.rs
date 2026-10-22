//! The Firmware page: listing and downloading console firmware.

use super::*;

impl UpdatesApp {
    pub(super) fn draw_firmware_page(&mut self, ui: &mut egui::Ui) {
        // Collect download and install actions outside the closure to avoid borrow issues.
        let mut download_fw: Option<crate::psn::firmware::FirmwareInfo> = None;
        let mut install_fw: Option<crate::psn::firmware::FirmwareInfo> = None;

        ui.vertical(|ui| {
            let consoles = crate::psn::firmware::FirmwareConsole::all();
            let regions = crate::psn::firmware::FirmwareRegion::all();

            ui.horizontal(|ui| {
                ui.label(tr!("firmware-console"));
                egui::ComboBox::from_id_salt("fw_console")
                    .selected_text(consoles[self.v.firmware_console_idx].to_string())
                    .show_ui(ui, |ui| {
                        for (i, console) in consoles.iter().enumerate() {
                            ui.selectable_value(&mut self.v.firmware_console_idx, i, console.to_string());
                        }
                    });

                ui.label(tr!("firmware-region"));
                egui::ComboBox::from_id_salt("fw_region")
                    .selected_text(regions[self.v.firmware_region_idx].to_string())
                    .show_ui(ui, |ui| {
                        for (i, region) in regions.iter().enumerate() {
                            ui.selectable_value(&mut self.v.firmware_region_idx, i, region.to_string());
                        }
                    });

                let fetching = self.v.firmware_fetch_promise.is_some();
                ui.add_enabled_ui(!fetching, |ui| {
                    if ui.button(tr!("firmware-check")).clicked() {
                        let console = consoles[self.v.firmware_console_idx];
                        let region = regions[self.v.firmware_region_idx];
                        let client = self
                            .v
                            .psn_client
                            .clone()
                            .expect("psn_client set in UpdatesApp::new before any UI code runs");
                        let ctx_clone = ui.ctx().clone();
                        let _guard = self
                            .v
                            .rt_handle
                            .as_ref()
                            .expect("rt_handle set in UpdatesApp::new before any UI code runs")
                            .enter();
                        self.v.firmware_results.clear();
                        self.v.firmware_fetch_promise = Some(Promise::spawn_async(async move {
                            let result = crate::psn::firmware::fetch_firmware(&client, console, region).await;
                            ctx_clone.request_repaint();
                            result
                        }));
                    }
                });
                if fetching {
                    ui.spinner();
                }
            });

            if self.settings.rpcs3_config_dir.is_some() {
                let installed = self.v.rpcs3_firmware_version.clone().unwrap_or_else(|| tr!("firmware-none"));
                ui.label(tr!("firmware-installed-version", version = installed));
            }

            ui.separator();

            if self.v.firmware_results.is_empty() && self.v.firmware_fetch_promise.is_none() {
                ui.label(tr!("firmware-hint"));
            }

            if !self.v.firmware_results.is_empty() {
                egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                    egui::Grid::new("firmware_grid")
                        .striped(true)
                        .min_col_width(80.0)
                        .show(ui, |ui| {
                            ui.strong(tr!("firmware-type"));
                            ui.strong(tr!("common-version"));
                            ui.strong(tr!("common-size"));
                            ui.strong("");
                            ui.end_row();

                            for entry in &self.v.firmware_results {
                                ui.label(entry.fw_type.to_string());
                                ui.label(&entry.version);
                                ui.label(
                                    entry
                                        .size
                                        .map(|s| ByteSize::b(s).to_string())
                                        .unwrap_or_else(|| "--".to_string()),
                                );

                                ui.horizontal(|ui| {
                                    let downloading = self.v.firmware_download.is_some();
                                    ui.add_enabled_ui(!downloading, |ui| {
                                        if ui.button(tr!("common-download")).clicked() {
                                            download_fw = Some(entry.clone());
                                        }
                                    });
                                    if self.can_install_firmware(entry) {
                                        if self.v.firmware_install_promise.is_some() {
                                            status_pill(ui, &tr!("common-installing"), StatusKind::Working);
                                        } else if ui.button(tr!("common-install-to-rpcs3")).clicked() {
                                            install_fw = Some(entry.clone());
                                        }
                                    }
                                });
                                ui.end_row();
                            }
                        });
                });
            }

            // Active firmware download progress.
            if let Some(dl) = &self.v.firmware_download {
                ui.separator();
                ui.label(tr!("firmware-downloading", name = dl.name.as_str()));

                let status = dl.progress_status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let progress = status.progress;
                let speed = status.speed_bps;
                drop(status);

                if dl.size > 0 {
                    let fraction = progress as f32 / dl.size as f32;
                    ui.add(egui::ProgressBar::new(fraction).text(format!(
                        "{} / {} ({}/s)",
                        ByteSize::b(progress),
                        ByteSize::b(dl.size),
                        ByteSize::b(speed as u64),
                    )));
                } else {
                    ui.add(egui::ProgressBar::new(0.0).text(tr!(
                        "firmware-downloaded-unknown-size",
                        done = ByteSize::b(progress).to_string(),
                        speed = ByteSize::b(speed as u64).to_string()
                    )));
                }

                if ui.button(tr!("common-cancel")).clicked() {
                    dl.cancel_signal.store(CANCEL_ABORT, Ordering::Relaxed);
                }
            }
        });

        // Trigger firmware download or install if requested.
        if let Some(fw) = download_fw {
            self.start_firmware_download(fw);
        }
        if let Some(fw) = install_fw {
            self.start_firmware_install(&fw);
        }
    }

    pub(super) fn start_firmware_download(&mut self, fw: crate::psn::firmware::FirmwareInfo) {
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let name = fw.display_name();
        let size = fw.size.unwrap_or(0);
        let download_path = self.settings.pkg_download_path.clone();
        let cancel_signal = Arc::new(AtomicU8::new(CANCEL_NONE));
        let cancel_for_task = cancel_signal.clone();

        let status = Arc::new(std::sync::Mutex::new(DownloadProgressStatus {
            progress: 0,
            last_received_status: DownloadStatus::Progress(0),
            speed_sample_bytes: 0,
            speed_sample_time: std::time::Instant::now(),
            speed_bps: 0.0,
        }));
        let async_status = status.clone();
        let ctx = self.v.egui_ctx.clone().unwrap_or_default();

        let client = self
            .v
            .psn_client
            .clone()
            .expect("psn_client set in UpdatesApp::new before any UI code runs");
        let _guard = self
            .v
            .rt_handle
            .as_ref()
            .expect("rt_handle set in UpdatesApp::new before any UI code runs")
            .enter();

        let promise = Promise::spawn_async(async move {
            let download_fut = fw.start_download(tx.clone(), download_path, cancel_for_task, &client);

            let progress_fut = async {
                while let Some(status_msg) = rx.recv().await {
                    let mut locked = async_status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let DownloadStatus::Progress(bytes) = &status_msg {
                        locked.record_progress(*bytes, 1000);
                    }
                    locked.last_received_status = status_msg;
                    drop(locked);
                    ctx.request_repaint();
                }
            };

            tokio::select! {
                result = download_fut => result,
                _ = progress_fut => Ok(()),
            }
        });

        self.v.firmware_download = Some(ActiveFirmwareDownload {
            name,
            size,
            progress_status: status,
            cancel_signal,
            promise,
        });
    }

    /// A full PS3 firmware already downloaded, with RPCS3 set in Settings: RPCS3 can install it.
    fn can_install_firmware(&self, fw: &crate::psn::firmware::FirmwareInfo) -> bool {
        fw.console == crate::psn::firmware::FirmwareConsole::PS3
            && fw.fw_type == crate::psn::firmware::FirmwareType::Full
            && self.settings.rpcs3_executable.is_some()
            && fw.output_path(&self.settings.pkg_download_path).is_file()
    }

    pub(super) fn read_rpcs3_firmware_version(&mut self) {
        self.v.rpcs3_firmware_version = self
            .settings
            .rpcs3_config_dir
            .as_deref()
            .and_then(|config| crate::rpcs3::installed_firmware_version(&crate::rpcs3::config::resolve_dev_flash(config)));
    }

    /// Install a downloaded firmware into RPCS3 in the background (RPSN-0141).
    fn start_firmware_install(&mut self, fw: &crate::psn::firmware::FirmwareInfo) {
        let Some(exe) = self.settings.rpcs3_executable.clone() else {
            return;
        };
        let pup = fw.output_path(&self.settings.pkg_download_path);
        let check = self
            .settings
            .rpcs3_config_dir
            .as_deref()
            .map(|config| crate::rpcs3::FirmwareCheck {
                dev_flash: crate::rpcs3::config::resolve_dev_flash(config),
                version: fw.version.clone(),
            });
        let ctx = self.v.egui_ctx.clone();
        let _guard = self
            .v
            .rt_handle
            .as_ref()
            .expect("rt_handle set in UpdatesApp::new before any UI code runs")
            .enter();
        self.v.firmware_install_promise = Some(Promise::spawn_async(async move {
            let outcome = crate::rpcs3::install_firmware(&exe, &pup, check).await;
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
            outcome
        }));
    }

    pub(super) fn handle_firmware_promises(&mut self, messages: &mut Vec<(String, MessageLevel)>) {
        if self.v.firmware_install_promise.as_ref().is_some_and(|p| p.ready().is_some()) {
            let promise = self.v.firmware_install_promise.take().expect("checked is_some above");
            match promise.block_and_take() {
                Ok(crate::rpcs3::InstallOutcome::Confirmed) => messages.push((tr!("firmware-installed"), MessageLevel::Success)),
                Ok(crate::rpcs3::InstallOutcome::Unconfirmed) => {
                    messages.push((tr!("firmware-install-unchecked"), MessageLevel::Warning))
                }
                Err(e) => {
                    error!("Firmware install failed: {e}");
                    messages.push((tr!("firmware-install-failed", reason = e.to_string()), MessageLevel::Error));
                }
            }
            self.read_rpcs3_firmware_version();
        }

        // Handle firmware fetch promise.
        if let Some(ref promise) = self.v.firmware_fetch_promise {
            if promise.ready().is_some() {
                let promise = self.v.firmware_fetch_promise.take().expect("checked is_some above");
                match promise.block_and_take() {
                    Ok(entries) => {
                        info!("Firmware fetch returned {} entries", entries.len());
                        self.v.firmware_results = entries;
                    }
                    Err(e) => {
                        messages.push((tr!("firmware-check-failed", reason = e.to_string()), MessageLevel::Error));
                    }
                }
            }
        }

        // Handle firmware download promise.
        if let Some(ref dl) = self.v.firmware_download {
            if dl.promise.ready().is_some() {
                let dl = self.v.firmware_download.take().expect("checked is_some above");
                match dl.promise.block_and_take() {
                    Ok(()) => {
                        messages.push((tr!("firmware-download-done", name = dl.name), MessageLevel::Success));
                    }
                    Err(DownloadError::Cancelled { .. }) => {
                        messages.push((tr!("firmware-download-cancelled", name = dl.name), MessageLevel::Info));
                    }
                    Err(e) => {
                        error!("Firmware download failed: {e:?}");
                        messages.push((
                            tr!("firmware-download-failed", reason = format!("{e:?}")),
                            MessageLevel::Error,
                        ));
                    }
                }
            }
        }
    }
}
