//! The results list: one entry per game and one row per package.

use super::*;

impl UpdatesApp {
    pub(super) fn draw_results_list(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        // Build filtered/sorted index list up front so the toolbar can show "showing X of Y".
        let filter_lower = self.v.results_filter.to_lowercase();
        let mut indices: Vec<usize> = (0..self.v.update_results.len())
            .filter(|&i| {
                if filter_lower.is_empty() {
                    return true;
                }
                let u = &self.v.update_results[i];
                u.title_id.to_lowercase().contains(&filter_lower) || u.title().to_lowercase().contains(&filter_lower)
            })
            .collect();

        let sort_by = self.v.results_sort_by;
        let sort_dir = self.v.results_sort_direction;
        indices.sort_by(|&a, &b| {
            let ua = &self.v.update_results[a];
            let ub = &self.v.update_results[b];
            let ord = match sort_by {
                ResultSortBy::SearchOrder => a.cmp(&b),
                ResultSortBy::Serial => ua.title_id.cmp(&ub.title_id),
                ResultSortBy::Title => ua.title().cmp(&ub.title()),
                ResultSortBy::UpdateCount => ua.packages.len().cmp(&ub.packages.len()),
                ResultSortBy::TotalSize => {
                    let sa: u64 = ua.packages.iter().map(|p| p.size).sum();
                    let sb: u64 = ub.packages.iter().map(|p| p.size).sum();
                    sa.cmp(&sb)
                }
            };
            if sort_dir == SortDirection::Descending {
                ord.reverse()
            } else {
                ord
            }
        });

        // Sort + filter toolbar with a clearer visual hierarchy than a flat row of labels.
        if !self.v.update_results.is_empty() {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;

                ui.label(egui::RichText::new("Filter:").strong());
                let filter_resp = ui.add(
                    egui::TextEdit::singleline(&mut self.v.results_filter)
                        .desired_width(200.0)
                        .hint_text("Serial or title…"),
                );
                filter_resp.on_hover_text("Show only results whose serial or title contains this text");
                if !self.v.results_filter.is_empty() && ui.small_button("\u{2715}").on_hover_text("Clear filter").clicked() {
                    self.v.results_filter.clear();
                }

                ui.separator();

                ui.label(egui::RichText::new("Sort by:").strong());
                egui::ComboBox::from_id_salt("results_sort")
                    .selected_text(self.v.results_sort_by.label())
                    .width(140.0)
                    .show_ui(ui, |ui| {
                        for opt in [
                            ResultSortBy::SearchOrder,
                            ResultSortBy::Serial,
                            ResultSortBy::Title,
                            ResultSortBy::UpdateCount,
                            ResultSortBy::TotalSize,
                        ] {
                            ui.selectable_value(&mut self.v.results_sort_by, opt, opt.label());
                        }
                    });

                let (arrow, hover) = match self.v.results_sort_direction {
                    SortDirection::Ascending => ("\u{2191}", "Ascending — click to flip to descending"),
                    SortDirection::Descending => ("\u{2193}", "Descending — click to flip to ascending"),
                };
                if ui
                    .add(egui::Button::new(egui::RichText::new(arrow).strong().size(15.0)))
                    .on_hover_text(hover)
                    .clicked()
                {
                    self.v.results_sort_direction = match self.v.results_sort_direction {
                        SortDirection::Ascending => SortDirection::Descending,
                        SortDirection::Descending => SortDirection::Ascending,
                    };
                }

                // Result count, right-aligned.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let total = self.v.update_results.len();
                    let visible = indices.len();
                    let text = if visible == total {
                        format!("{total} result{}", if total == 1 { "" } else { "s" })
                    } else {
                        format!("Showing {visible} of {total}")
                    };
                    ui.label(egui::RichText::new(text).weak());
                });
            });
            ui.add_space(2.0);
            ui.separator();
        }

        let result_count = self.v.update_results.len();
        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            if indices.is_empty() && !self.v.update_results.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("No results match the current filter.").weak());
                });
                return;
            }
            for &i in &indices {
                let update = self.v.update_results[i].clone();
                self.draw_result_entry(ctx, ui, &update, result_count);
            }
        });
    }

    pub(super) fn draw_result_entry(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, update: &UpdateInfo, result_count: usize) {
        let total_updates_size = update.packages.iter().map(|pkg| pkg.size).sum::<u64>();

        let title_id = &update.title_id;
        let update_count = update.packages.len();
        let platform_variant = update.platform_variant;

        // Aggregate per-package status counts for the header status pill.
        let completed_count = update
            .packages
            .iter()
            .filter(|pkg| self.cached_download_info(title_id, pkg).status == ActiveDownloadStatus::Completed)
            .count();
        let failed_count = update
            .packages
            .iter()
            .filter(|pkg| self.cached_download_info(title_id, pkg).status == ActiveDownloadStatus::Failed)
            .count();
        let paused_count = update
            .packages
            .iter()
            .filter(|pkg| self.cached_download_info(title_id, pkg).status == ActiveDownloadStatus::Paused)
            .count();
        let in_progress_count = update
            .packages
            .iter()
            .filter(|pkg| {
                matches!(
                    self.cached_download_info(title_id, pkg).status,
                    ActiveDownloadStatus::Downloading(_) | ActiveDownloadStatus::Resuming(_, _) | ActiveDownloadStatus::Verifying
                )
            })
            .count();

        let id = egui::Id::new(format!("pkg_header_{title_id}"));

        egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, result_count == 1)
            .show_header(ui, |ui| {
                let title = update.title();

                // Left side: platform badge + title (or serial fallback) + metadata strip.
                platform_badge(ui, platform_variant);
                ui.add_space(4.0);

                if !title.is_empty() {
                    ui.label(egui::RichText::new(&title).strong().size(14.0));
                    ui.label(egui::RichText::new("\u{00B7}").weak());
                }
                ui.label(egui::RichText::new(title_id).monospace().weak());
                ui.label(egui::RichText::new("\u{00B7}").weak());
                ui.label(
                    egui::RichText::new(format!("{update_count} update{}", if update_count == 1 { "" } else { "s" })).weak(),
                );
                ui.label(egui::RichText::new("\u{00B7}").weak());
                ui.label(egui::RichText::new(format!("{}", ByteSize::b(total_updates_size))).weak());

                // Right side: actions and aggregate status pill, packed right-to-left so the
                // primary action sits at the rightmost edge in the conventional position.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // PS4 multipart merge controls (rightmost when applicable).
                    if platform_variant == utils::PlatformVariant::PS4 {
                        let is_multipart = update.packages.len() > 1;
                        let all_pkgs_completed = update
                            .packages
                            .iter()
                            .all(|pkg| self.cached_download_info(title_id, pkg).status == ActiveDownloadStatus::Completed);
                        let is_mergeable = is_multipart && all_pkgs_completed;
                        let hover_text = if is_multipart {
                            "All parts need to be completed for merge to be available"
                        } else {
                            "This PS4 update is not a multipart update"
                        };

                        let merge_status = self.title_merge_status(update);
                        match merge_status {
                            ActiveMergeStatus::Merging(progress) => {
                                let width = ui.available_width().min(150.0);
                                ui.add(egui::ProgressBar::new(progress).desired_width(width).show_percentage());
                                status_pill(ui, "Merging parts…", StatusKind::Working);
                            }
                            ActiveMergeStatus::Merged => {
                                status_pill(ui, "Parts merged", StatusKind::Done);
                            }
                            ActiveMergeStatus::Failed => {
                                status_pill(ui, "Parts merge failed", StatusKind::Failed);
                            }
                            _ => {}
                        }

                        let merge_btn = ui
                            .add_enabled(is_mergeable, egui::Button::new("Merge parts"))
                            .on_disabled_hover_text(hover_text);
                        if merge_btn.clicked() {
                            self.v.merge_queue.push(self.start_merge_parts(update.clone(), ui));
                        }
                    }

                    // Primary action: download just the latest package.
                    if let Some(latest_pkg) = update.packages.last() {
                        let resp = ui
                            .button("Download latest")
                            .on_hover_text("Queue only the most recent update");
                        if resp.clicked() && self.get_active_download(title_id, latest_pkg).is_none() {
                            info!("Downloading latest update {} for serial {title_id}", latest_pkg.id());
                            self.queue_download(title_id.to_string(), title.clone(), latest_pkg.clone(), ui);
                        }
                    }

                    // Secondary action: download every update.
                    let resp = ui.button("Download all").on_hover_text("Queue every update for this title");
                    if resp.clicked() {
                        info!("Downloading all updates for serial {title_id} ({update_count})");
                        for pkg in update.packages.iter() {
                            if self.get_active_download(title_id, pkg).is_none() {
                                info!("Downloading update {} for serial {title_id} (group)", pkg.id());
                                self.queue_download(title_id.to_string(), title.clone(), pkg.clone(), ui);
                            }
                        }
                    }

                    ui.add_space(6.0);

                    // Aggregate status pill (rendered to the left of the action buttons).
                    if completed_count == update_count {
                        status_pill(ui, "All downloaded", StatusKind::Done);
                    } else if failed_count > 0 {
                        let text = if completed_count > 0 {
                            format!("{completed_count}/{update_count} done · {failed_count} failed")
                        } else {
                            format!("{failed_count} failed")
                        };
                        status_pill(ui, &text, StatusKind::Failed);
                    } else if in_progress_count > 0 {
                        let text = if completed_count > 0 {
                            format!("{completed_count}/{update_count} done · {in_progress_count} active")
                        } else {
                            format!("{in_progress_count} downloading")
                        };
                        status_pill(ui, &text, StatusKind::Active);
                    } else if paused_count > 0 {
                        status_pill(ui, &format!("{paused_count} paused"), StatusKind::Paused);
                    } else if completed_count > 0 {
                        status_pill(ui, &format!("{completed_count}/{update_count} done"), StatusKind::Done);
                    }
                });
            })
            .body(|ui| {
                ui.add_space(4.0);

                let pkg_count = update.packages.len();
                for (pkg_idx, pkg) in update.packages.iter().enumerate() {
                    let is_latest = pkg_idx == pkg_count - 1;
                    self.draw_entry_pkg(ui, pkg, title_id, update.title(), is_latest);
                    ui.add_space(4.0);
                }
            });

        ui.add_space(6.0);
    }

    pub(super) fn draw_entry_pkg(
        &mut self,
        ui: &mut egui::Ui,
        pkg: &PackageInfo,
        title_id: &str,
        title: String,
        is_latest: bool,
    ) {
        ui.group(|ui| {
            let cached = self.cached_download_info(title_id, pkg);
            let download_status = cached.status;
            let is_active = matches!(
                download_status,
                ActiveDownloadStatus::Downloading(_) | ActiveDownloadStatus::Resuming(_, _) | ActiveDownloadStatus::Verifying
            );

            let mut pause_clicked = false;
            let mut abort_clicked = false;
            let mut download_clicked = false;

            // Row 1: identity (version + Latest badge) on the left; status + actions on the right.
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("v{}", pkg.id())).strong().size(14.0));
                if is_latest {
                    ui.label(
                        egui::RichText::new("\u{2605} Latest")
                            .color(latest_badge_color(ui.visuals()))
                            .strong()
                            .small(),
                    );
                }
                ui.label(egui::RichText::new(format!("{}", ByteSize::b(pkg.size))).weak());
                if pkg.offset > 0 {
                    ui.label(egui::RichText::new(format!("\u{00B7} offset {}", pkg.offset)).weak().small());
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    match download_status {
                        ActiveDownloadStatus::NotStarted => {
                            if ui.button("Download").clicked() {
                                download_clicked = true;
                            }
                        }
                        ActiveDownloadStatus::Pending => {
                            if ui.button("Cancel").clicked() {
                                self.v
                                    .pending_downloads
                                    .retain(|p| !(p.title_id == title_id && p.pkg.id() == pkg.id()));
                            }
                            status_pill(ui, "Queued", StatusKind::Queued);
                        }
                        ActiveDownloadStatus::Downloading(_) | ActiveDownloadStatus::Resuming(_, _) => {
                            abort_clicked = ui.button("Abort").clicked();
                            pause_clicked = ui.button("Pause").clicked();
                            let label = match download_status {
                                ActiveDownloadStatus::Resuming(_, offset) => format!("Resuming from {}", ByteSize::b(offset)),
                                _ => "Downloading".to_string(),
                            };
                            status_pill(ui, &label, StatusKind::Active);
                        }
                        ActiveDownloadStatus::Verifying => {
                            status_pill(ui, "Verifying", StatusKind::Working);
                        }
                        ActiveDownloadStatus::Paused => {
                            if ui.button("Abort").clicked() {
                                let file_path = self
                                    .v
                                    .paused_downloads
                                    .iter()
                                    .find(|p| p.title_id == title_id && p.pkg_id == pkg.id())
                                    .and_then(|p| p.file_path.clone());
                                self.v
                                    .paused_downloads
                                    .retain(|p| !(p.title_id == title_id && p.pkg_id == pkg.id()));
                                // Remove only this package's partial file, and only inside the download folder.
                                if let Some(path) = file_path {
                                    match crate::security::validate_path_containment(&path, &self.settings.pkg_download_path) {
                                        Ok(()) => {
                                            if let Err(e) = std::fs::remove_file(&path) {
                                                warn!("Failed to clean up paused download {}: {e}", path.display());
                                            }
                                        }
                                        Err(e) => warn!("Not removing paused download: {e}"),
                                    }
                                }
                            }
                            if ui.button("Resume").clicked() {
                                self.v
                                    .paused_downloads
                                    .retain(|p| !(p.title_id == title_id && p.pkg_id == pkg.id()));
                                self.queue_download(title_id.to_string(), title.clone(), pkg.clone(), ui);
                            }
                            status_pill(ui, "Paused", StatusKind::Paused);
                        }
                        ActiveDownloadStatus::Completed => {
                            let install_key = (title_id.to_string(), pkg.id());
                            let is_installing = self
                                .v
                                .rpcs3_install_promises
                                .iter()
                                .any(|(tid, pid, _)| tid == title_id && *pid == pkg.id());
                            let is_installed = self.v.rpcs3_installed_pkgs.contains(&install_key);

                            if !is_installing && !is_installed {
                                if let Some(rpcs3_exe) = &self.settings.rpcs3_executable {
                                    if ui.button("Install to RPCS3").clicked() {
                                        // Install exactly the file this download wrote.
                                        let pkg_file = self
                                            .v
                                            .completed_paths
                                            .get(&(title_id.to_string(), pkg.id()))
                                            .filter(|p| p.is_file())
                                            .cloned();
                                        if pkg_file.is_none() {
                                            warn!("No downloaded file recorded for {title_id} {}", pkg.id());
                                        }
                                        if let Some(pkg_file) = pkg_file {
                                            let exe = rpcs3_exe.clone();
                                            let tid = title_id.to_string();
                                            let pid = pkg.id();
                                            let check = Self::update_install_check(
                                                self.settings.rpcs3_config_dir.as_deref(),
                                                title_id,
                                                Some(pkg.version.clone()),
                                            );
                                            let rt = self
                                                .v
                                                .rt_handle
                                                .clone()
                                                .expect("rt_handle set in UpdatesApp::new before any UI code runs");
                                            let _guard = rt.enter();
                                            let promise = Promise::spawn_async(async move {
                                                crate::rpcs3::install_pkg(&exe, &pkg_file, check).await
                                            });
                                            self.v.rpcs3_install_promises.push((tid, pid, promise));
                                        }
                                    }
                                }
                            }

                            if ui.button("Open folder").clicked() {
                                let folder = crate::utils::create_pkg_path(
                                    &self.settings.pkg_download_path,
                                    title_id,
                                    &title,
                                    self.settings.directory_format,
                                );
                                self.open_folder(&folder);
                            }

                            if is_installed {
                                status_pill(ui, "Installed in RPCS3", StatusKind::Done);
                            } else if is_installing {
                                status_pill(ui, "Installing…", StatusKind::Working);
                            } else {
                                status_pill(ui, "Completed", StatusKind::Done);
                            }
                        }
                        ActiveDownloadStatus::Failed => {
                            if ui.button("Retry").clicked() {
                                self.v
                                    .failed_downloads
                                    .retain(|f| !(f.title_id == title_id && f.pkg_id == pkg.id()));
                                self.queue_download(title_id.to_string(), title.clone(), pkg.clone(), ui);
                            }
                            status_pill(ui, "Failed", StatusKind::Failed);
                        }
                    }

                    // Per-package merge status (only meaningful for PS4 multipart entries).
                    match self.pkg_merge_status(title_id, pkg) {
                        ActiveMergeStatus::NotMergeable | ActiveMergeStatus::NotStarted => {}
                        ActiveMergeStatus::Failed => status_pill(ui, "Merge failed", StatusKind::Failed),
                        ActiveMergeStatus::Merged => status_pill(ui, "Merged", StatusKind::Done),
                        ActiveMergeStatus::Merging(_) => status_pill(ui, "Merging…", StatusKind::Working),
                    }
                });
            });

            // Row 2: progress bar + speed/ETA — shown only while a transfer is in flight.
            if is_active {
                ui.horizontal(|ui| {
                    let progress = match download_status {
                        ActiveDownloadStatus::Downloading(p) | ActiveDownloadStatus::Resuming(p, _) => p,
                        _ => 0.0,
                    };
                    let width = (ui.available_width() - 220.0).max(120.0);
                    ui.add(egui::ProgressBar::new(progress).desired_width(width).show_percentage());
                    self.draw_speed_eta(ui, &cached);
                });
            }

            // Optional details — hidden by default to keep SHA-1 / URL noise out of the way for
            // typical use, but one click away for power users.
            egui::CollapsingHeader::new(egui::RichText::new("Details").weak())
                .id_salt(format!("pkg_details_{title_id}_{}", pkg.id()))
                .default_open(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("SHA-1:").weak());
                        ui.label(egui::RichText::new(&pkg.sha1sum).monospace().small());
                        if ui.small_button("Copy").clicked() {
                            self.copy_to_clipboard(pkg.sha1sum.clone());
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Source URL:").weak());
                        if ui.small_button("Copy URL").clicked() {
                            self.copy_to_clipboard(pkg.url.clone());
                        }
                    });
                });

            // Apply queued click effects after the row is drawn so the borrow checker stays happy
            // and so any spawned downloads don't read self.v while we're mid-render.
            if pause_clicked {
                if let Some(d) = self.get_active_download(title_id, pkg) {
                    d.cancel_signal.store(CANCEL_PAUSE, Ordering::Relaxed);
                }
            }
            if abort_clicked {
                if let Some(d) = self.get_active_download(title_id, pkg) {
                    d.cancel_signal.store(CANCEL_ABORT, Ordering::Relaxed);
                }
            }
            if download_clicked {
                info!("Downloading update {} for serial {} (individual)", pkg.version, title_id);
                self.queue_download(title_id.to_string(), title, pkg.clone(), ui);
            }
        });
    }

    /// Render per-download speed and ETA inline.
    pub(super) fn draw_speed_eta(&self, ui: &mut egui::Ui, cached: &CachedDownloadInfo) {
        if cached.speed_bps > 0.0 {
            let remaining = cached.total_bytes.saturating_sub(cached.progress_bytes) as f64;
            let eta_secs = (remaining / cached.speed_bps) as u64;
            let eta_str = if eta_secs < 60 {
                format!("{eta_secs}s")
            } else if eta_secs < 3600 {
                format!("{}:{:02}", eta_secs / 60, eta_secs % 60)
            } else {
                format!("{}:{:02}:{:02}", eta_secs / 3600, (eta_secs % 3600) / 60, eta_secs % 60)
            };
            ui.label(format!("{}/s - ETA {eta_str}", ByteSize::b(cached.speed_bps as u64)));
        }
    }
}
