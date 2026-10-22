//! The History page: finished downloads, kept between runs (RPSN-0068).

use super::*;

/// The key the history is stored under, beside eframe's own settings.
pub(super) const STORAGE_KEY: &str = "download_history";

/// How many entries the history keeps; the oldest goes first.
const HISTORY_CAP: usize = 500;

/// Seconds since 1970, or 0 on a clock set before it.
pub(super) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Add a finished download, dropping the oldest entries past the cap.
pub(super) fn record(history: &mut Vec<DownloadHistoryEntry>, entry: DownloadHistoryEntry) {
    history.push(entry);
    let excess = history.len().saturating_sub(HISTORY_CAP);
    history.drain(..excess);
}

/// "12s ago", "5m ago", "2h 3m ago" or "4d ago".
fn ago(secs: u64) -> String {
    if secs < 60 {
        tr!("history-ago-seconds", seconds = secs.to_string())
    } else if secs < 3600 {
        tr!("history-ago-minutes", minutes = (secs / 60).to_string())
    } else if secs < 86_400 {
        tr!(
            "history-ago-hours",
            hours = (secs / 3600).to_string(),
            minutes = ((secs % 3600) / 60).to_string()
        )
    } else {
        tr!("history-ago-days", days = (secs / 86_400).to_string())
    }
}

impl UpdatesApp {
    pub(super) fn draw_history_page(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            if self.v.download_history.is_empty() {
                ui.label(tr!("history-empty"));
                return;
            }
            ui.horizontal(|ui| {
                ui.label(tr!("history-count", count = self.v.download_history.len().to_string()));
                if ui.button(tr!("history-clear")).clicked() {
                    self.v.download_history.clear();
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("history_grid")
                    .striped(true)
                    .min_col_width(60.0)
                    .show(ui, |ui| {
                        ui.strong(tr!("common-serial"));
                        ui.strong(tr!("common-version"));
                        ui.strong(tr!("common-title"));
                        ui.strong(tr!("common-size"));
                        ui.strong(tr!("history-time"));
                        ui.end_row();

                        for entry in self.v.download_history.iter().rev() {
                            ui.label(&entry.title_id);
                            ui.label(&entry.pkg_id);
                            ui.label(&entry.title);
                            ui.label(ByteSize::b(entry.size).to_string());
                            let time_str = ago(now().saturating_sub(entry.completed_at));
                            ui.label(time_str);
                            ui.end_row();
                        }
                    });
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title_id: &str) -> DownloadHistoryEntry {
        DownloadHistoryEntry {
            title_id: title_id.to_string(),
            pkg_id: "01.01".to_string(),
            title: "Game".to_string(),
            size: 1,
            completed_at: 1_700_000_000,
        }
    }

    #[test]
    fn the_history_survives_a_save_and_load() {
        // It lasted one session only (RPSN-0068).
        let saved = ron::ser::to_string(&vec![entry("BLES00001")]).expect("the history serializes");
        let loaded: Vec<DownloadHistoryEntry> = ron::from_str(&saved).expect("the history loads");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].title_id, "BLES00001");
        assert_eq!(loaded[0].completed_at, 1_700_000_000);
    }

    #[test]
    fn the_history_keeps_only_the_newest_entries() {
        let mut history = Vec::new();
        for i in 0..HISTORY_CAP + 3 {
            record(&mut history, entry(&format!("SERIAL{i}")));
        }
        assert_eq!(history.len(), HISTORY_CAP);
        assert_eq!(history[0].title_id, "SERIAL3");
    }

    #[test]
    fn ages_read_in_the_largest_whole_unit() {
        assert_eq!(ago(59), "59s ago");
        assert_eq!(ago(3_599), "59m ago");
        assert_eq!(ago(3_660), "1h 1m ago");
        assert_eq!(ago(172_800), "2d ago");
    }
}
