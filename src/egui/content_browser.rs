//! The Patches and DLC page: local patches and DLC, and installing them.

use super::*;

impl UpdatesApp {
    pub(super) fn draw_content_browser_page(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            if self.settings.content_directory.is_none() {
                ui.label(tr!("content-browser-no-dir"));
                if ui.button(tr!("content-browser-open-settings")).clicked() {
                    self.go_to(chrome::Page::Settings, &ui.ctx().clone());
                }
                return;
            }

            let has_rpcs3 = self.settings.rpcs3_executable.is_some();
            // Without the RPCS3 program the Install buttons below are hidden; say why (RPSN-0179).
            if !has_rpcs3 {
                ui.horizontal(|ui| {
                    ui.label(tr!("content-browser-no-rpcs3"));
                    if ui.button(tr!("content-browser-open-settings")).clicked() {
                        self.go_to(chrome::Page::Settings, &ui.ctx().clone());
                    }
                });
                ui.separator();
            }

            // Toolbar
            ui.horizontal(|ui| {
                ui.label(tr!("common-filter"));
                ui.text_edit_singleline(&mut self.v.content_filter);
                if ui.button(tr!("content-browser-rescan")).clicked() {
                    self.start_content_scan();
                }
            });
            ui.separator();

            let Some(ref library) = self.v.content_library else {
                ui.label(tr!("content-browser-loading"));
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

            ui.label(tr!(
                "content-browser-shown",
                shown = filtered_games.len().to_string(),
                total = library.games.len().to_string()
            ));
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
                                ui.label(
                                    egui::RichText::new(tr!("content-browser-patches", count = game.patches.len().to_string()))
                                        .underline(),
                                );
                                for patch in &game.patches {
                                    ui.horizontal(|ui| {
                                        ui.label(format!("  {} / {}", patch.serial, patch.pkg_filename));
                                        let patch_key = patch.pkg_path.to_string_lossy().to_string();
                                        let is_installed = self.v.content_installed_items.contains(&patch_key);
                                        let is_installing =
                                            self.v.content_install_promises.iter().any(|(k, _, _)| *k == patch_key);

                                        if is_installed {
                                            status_pill(ui, &tr!("content-browser-installed"), StatusKind::Done);
                                        } else if is_installing {
                                            status_pill(ui, &tr!("common-installing"), StatusKind::Working);
                                        } else if has_rpcs3 && ui.small_button(tr!("content-browser-install")).clicked() {
                                            let exe =
                                                self.settings.rpcs3_executable.clone().expect("has_rpcs3 checked it is set");
                                            let pkg = patch.pkg_path.clone();
                                            let key = patch_key.clone();
                                            // ContentPatch carries no version, so any change of version confirms it.
                                            let check = Self::update_install_check(
                                                self.settings.rpcs3_config_dir.as_deref(),
                                                &patch.serial,
                                                None,
                                            );
                                            let rt = self
                                                .v
                                                .rt_handle
                                                .clone()
                                                .expect("rt_handle set in UpdatesApp::new before any UI code runs");
                                            let _guard = rt.enter();
                                            let promise = spawn_redrawing(Some(ui.ctx().clone()), async move {
                                                crate::rpcs3::install_pkg(&exe, &pkg, check)
                                                    .await
                                                    .map_err(|e| crate::content::ContentError::InstallFailed(e.to_string()))
                                            });
                                            self.v.content_install_promises.push((key, ContentKind::Patch, promise));
                                        }
                                    });
                                }
                                ui.add_space(3.0);
                            }

                            // DLC section
                            if !game.dlc.is_empty() {
                                ui.label(
                                    egui::RichText::new(tr!("content-browser-dlc", count = game.dlc.len().to_string()))
                                        .underline(),
                                );
                                for dlc_item in &game.dlc {
                                    ui.horizontal(|ui| {
                                        ui.label(format!("  {} ({})", dlc_item.zip_filename, ByteSize::b(dlc_item.size)));
                                        let dlc_key = dlc_item.zip_path.to_string_lossy().to_string();
                                        let is_installed = self.v.content_installed_items.contains(&dlc_key);
                                        let is_installing = self.v.content_install_promises.iter().any(|(k, _, _)| *k == dlc_key);

                                        if is_installed {
                                            status_pill(ui, &tr!("content-browser-installed"), StatusKind::Done);
                                        } else if is_installing {
                                            status_pill(ui, &tr!("common-installing"), StatusKind::Working);
                                        } else if has_rpcs3 && ui.small_button(tr!("content-browser-install")).clicked() {
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
                                            let promise = spawn_redrawing(Some(ui.ctx().clone()), async move {
                                                // rusty-psn cannot check a DLC install, so it is never shown as installed
                                                // (docs/specs/RPSN-0137-headless-confirmed-install.md § 4.4, RPSN-0182).
                                                crate::content::install_dlc_zip(&exe, &zip)
                                                    .await
                                                    .map(|()| crate::rpcs3::InstallOutcome::Unconfirmed)
                                            });
                                            self.v.content_install_promises.push((key, ContentKind::Dlc, promise));
                                        }
                                    });
                                }
                            }
                        });
                }
            });
        });
    }

    /// Scan the content folder in the background: it reads every file there, which would
    /// freeze the window. A second request while one runs is ignored.
    pub(super) fn start_content_scan(&mut self) {
        let Some(dir) = self.settings.content_directory.clone() else {
            return;
        };
        if self.v.content_scan_promise.is_some() {
            return;
        }
        let _guard = self
            .v
            .rt_handle
            .as_ref()
            .expect("rt_handle set in UpdatesApp::new before any UI code runs")
            .enter();
        let ctx = self.v.egui_ctx.clone();
        self.v.content_scan_promise = Some(Promise::spawn_async(async move {
            let library = tokio::task::spawn_blocking(move || crate::content::scan_content_directory(&dir))
                .await
                .ok();
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
            library
        }));
    }

    pub(super) fn handle_content_scan_promise(&mut self, messages: &mut Vec<(String, MessageLevel)>) {
        if !self.v.content_scan_promise.as_ref().is_some_and(|p| p.ready().is_some()) {
            return;
        }
        let promise = self.v.content_scan_promise.take().expect("checked above");
        match promise.block_and_take() {
            Some(library) => self.v.content_library = Some(library),
            None => messages.push((tr!("content-browser-scan-stopped"), MessageLevel::Error)),
        }
    }

    pub(super) fn handle_content_install_promises(&mut self, messages: &mut Vec<(String, MessageLevel)>) {
        let mut i = 0;
        while i < self.v.content_install_promises.len() {
            if self.v.content_install_promises[i].2.ready().is_some() {
                let (key, kind, promise) = self.v.content_install_promises.remove(i);
                // A short display name from the key (file path)
                let display = Path::new(&key)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| key.clone());
                let (message, level, installed) = install_report(kind, &display, promise.block_and_take());
                messages.push((message, level));
                if installed {
                    self.v.content_installed_items.insert(key);
                }
            } else {
                i += 1;
            }
        }
    }
}

/// What the Patches and DLC page is installing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ContentKind {
    Patch,
    Dlc,
}

/// What the page says when an install ends, and whether it then shows the Installed pill.
fn install_report(
    kind: ContentKind,
    name: &str,
    result: Result<crate::rpcs3::InstallOutcome, crate::content::ContentError>,
) -> (String, MessageLevel, bool) {
    use crate::rpcs3::InstallOutcome;
    match (kind, result) {
        (_, Ok(InstallOutcome::Confirmed)) => (tr!("content-browser-install-done", name = name), MessageLevel::Success, true),
        (ContentKind::Dlc, Ok(InstallOutcome::Unconfirmed)) => (
            tr!("content-browser-dlc-unchecked", name = name),
            MessageLevel::Warning,
            false,
        ),
        (ContentKind::Patch, Ok(InstallOutcome::Unconfirmed)) => {
            (tr!("common-install-unchecked", name = name), MessageLevel::Warning, false)
        }
        (_, Err(e)) => (
            tr!("content-browser-install-failed", reason = e.to_string()),
            MessageLevel::Error,
            false,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpcs3::InstallOutcome;

    // RPSN-0182: a DLC install is never checked, so it must not show the green Installed pill,
    // and the warning must say why rusty-psn could not check it.
    #[test]
    fn an_unchecked_dlc_install_is_not_shown_as_installed() {
        let (message, level, installed) = install_report(ContentKind::Dlc, "ep2.zip", Ok(InstallOutcome::Unconfirmed));
        assert!(!installed, "marked installed: {message}");
        assert_eq!(level, MessageLevel::Warning, "{message}");
        assert!(message.contains("ep2.zip"), "{message}");
        assert!(
            message.contains("cannot check a DLC"),
            "says why it could not check: {message}"
        );
    }

    /// Draws the Patches and DLC page for one frame and returns every piece of text it drew.
    fn page_text(app: &mut UpdatesApp) -> String {
        fn collect(shape: &egui::Shape, out: &mut String) {
            match shape {
                egui::Shape::Text(text) => {
                    out.push_str(text.galley.text());
                    out.push('\n');
                }
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.draw_content_browser_page(ui));
        // Nothing draws these textures; epaint panics if they are dropped unapplied.
        output.textures_delta.clear();
        let mut text = String::new();
        output.shapes.iter().for_each(|clipped| collect(&clipped.shape, &mut text));
        text
    }

    // RPSN-0179: with no RPCS3 program set the Install buttons are hidden, so the page must say why.
    #[test]
    fn the_page_says_why_install_is_missing() {
        let mut app = UpdatesApp::default();
        app.settings.content_directory = Some(std::path::PathBuf::from("/content"));
        app.settings.rpcs3_executable = None;
        let text = page_text(&mut app);
        assert!(text.contains("RPCS3 executable"), "no hint drawn:\n{text}");
        assert!(text.contains("Open Settings"), "no way to Settings:\n{text}");

        app.settings.rpcs3_executable = Some(std::path::PathBuf::from("/rpcs3/rpcs3"));
        let text = page_text(&mut app);
        assert!(!text.contains("RPCS3 executable"), "hint drawn with RPCS3 set:\n{text}");
    }

    #[test]
    fn a_patch_install_is_shown_as_installed_only_when_checked() {
        assert!(install_report(ContentKind::Patch, "p.pkg", Ok(InstallOutcome::Confirmed)).2);
        let (message, level, installed) = install_report(ContentKind::Patch, "p.pkg", Ok(InstallOutcome::Unconfirmed));
        assert!(!installed, "{message}");
        assert_eq!(level, MessageLevel::Warning);
    }
}
