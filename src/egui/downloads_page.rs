//! The Downloads page: every package downloaded or downloading this session, in three foldable
//! sections (RPSN-0157 step 5). Each row is the package row the Search page draws, with the same
//! buttons, so a download can be paused, resumed, retried or installed from either page.

use super::*;

/// One package row on the page: the title id, the game's title and the package.
type Row = (String, String, PackageInfo);

impl UpdatesApp {
    /// The page's three sections, in the order shown: needs attention, in progress, finished.
    fn download_sections(&self) -> [Vec<Row>; 3] {
        let attention = self
            .v
            .failed_downloads
            .iter()
            .map(|f| (f.title_id.clone(), f.title.clone(), f.pkg.clone()))
            .collect();

        let mut in_progress: Vec<Row> = self
            .v
            .download_queue
            .iter()
            .map(|d| (d.title_id.clone(), d.title.clone(), d.pkg.clone()))
            .collect();
        in_progress.extend(
            self.v
                .paused_downloads
                .iter()
                .map(|p| (p.title_id.clone(), p.title.clone(), p.pkg.clone())),
        );
        in_progress.extend(
            self.v
                .pending_downloads
                .iter()
                .map(|p| (p.title_id.clone(), p.title.clone(), p.pkg.clone())),
        );

        let finished = self
            .v
            .completed_downloads
            .iter()
            .rev()
            .map(|f| (f.title_id.clone(), f.title.clone(), f.pkg.clone()))
            .collect();

        [attention, in_progress, finished]
    }

    /// Queue every failed download again, in the order they failed (RPSN-0029).
    pub(super) fn retry_all_failed(&mut self) {
        for failed in std::mem::take(&mut self.v.failed_downloads) {
            self.queue_download_no_ui(failed.title_id, failed.title, failed.pkg);
        }
    }

    pub(super) fn draw_downloads_page(&mut self, ui: &mut egui::Ui) {
        let [attention, in_progress, finished] = self.download_sections();
        if attention.is_empty() && in_progress.is_empty() && finished.is_empty() {
            ui.label(tr!("downloads-page-empty"));
            return;
        }

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (salt, name, rows) in [
                ("Needs attention", tr!("downloads-page-attention"), attention),
                ("In progress", tr!("downloads-page-in-progress"), in_progress),
                ("Finished", tr!("downloads-page-finished"), finished),
            ] {
                egui::CollapsingHeader::new(tr!("downloads-page-section", name = name, count = rows.len().to_string()))
                    .id_salt(salt)
                    .default_open(true)
                    .show(ui, |ui| {
                        if rows.is_empty() {
                            ui.weak(tr!("downloads-page-none"));
                        } else if salt == "Needs attention" && ui.button(tr!("downloads-page-retry-all")).clicked() {
                            self.retry_all_failed();
                        }
                        for (title_id, title, pkg) in rows {
                            ui.label(egui::RichText::new(format!("{title_id} · {title}")).strong());
                            self.draw_entry_pkg(ui, &pkg, &title_id, title, false);
                        }
                    });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(version: &str) -> PackageInfo {
        PackageInfo {
            url: String::new(),
            size: 0,
            version: version.to_string(),
            sha1sum: String::new(),
            hash_whole_file: false,
            manifest_url: String::new(),
            offset: 0,
            part_number: None,
            min_system_version: None,
        }
    }

    #[test]
    fn each_download_lands_in_its_section() {
        let mut app = UpdatesApp::default();
        let title = || "Game".to_string();
        app.v.failed_downloads.push(FailedDownload {
            title_id: "FAILED".into(),
            pkg_id: "01.01".into(),
            title: title(),
            pkg: package("01.01"),
            incomplete: false,
        });
        app.v.paused_downloads.push(PausedDownload {
            title_id: "PAUSED".into(),
            pkg_id: "01.02".into(),
            title: title(),
            pkg: package("01.02"),
            file_path: None,
        });
        app.v.pending_downloads.push_back(PendingDownload {
            title_id: "QUEUED".into(),
            title: title(),
            pkg: package("01.03"),
        });
        for (title_id, version) in [("DONE1", "01.04"), ("DONE2", "01.05")] {
            app.v.completed_downloads.push(FinishedDownload {
                title_id: title_id.into(),
                title: title(),
                pkg: package(version),
            });
        }

        let ids = |rows: &Vec<Row>| rows.iter().map(|(id, _, _)| id.clone()).collect::<Vec<_>>();
        let [attention, in_progress, finished] = app.download_sections();
        assert_eq!(ids(&attention), ["FAILED"]);
        assert_eq!(ids(&in_progress), ["PAUSED", "QUEUED"]);
        assert_eq!(ids(&finished), ["DONE2", "DONE1"], "newest finished first");
    }

    #[test]
    fn retry_all_failed_queues_every_failed_download() {
        let mut app = UpdatesApp::default();
        app.settings.max_concurrent_downloads = 0; // Everything waits, so no download starts in the test.
        for (title_id, version) in [("ONE", "01.01"), ("TWO", "01.02")] {
            app.v.failed_downloads.push(FailedDownload {
                title_id: title_id.into(),
                pkg_id: version.into(),
                title: "Game".into(),
                pkg: package(version),
                incomplete: false,
            });
        }

        app.retry_all_failed();
        assert!(app.v.failed_downloads.is_empty());
        let queued: Vec<_> = app.v.pending_downloads.iter().map(|p| p.title_id.as_str()).collect();
        assert_eq!(queued, ["ONE", "TWO"]);
    }
}
