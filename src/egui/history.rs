//! The download history window.

use super::*;

impl UpdatesApp {
    pub(super) fn draw_download_history_window(&mut self, ctx: &egui::Context) {
        let mut show = self.v.show_download_history_window;
        egui::Window::new("Download History")
            .open(&mut show)
            .resizable(true)
            .default_size([550.0, 350.0])
            .show(ctx, |ui| {
                if self.v.download_history.is_empty() {
                    ui.label("No downloads completed this session.");
                    return;
                }
                ui.horizontal(|ui| {
                    ui.label(format!("{} download(s) completed", self.v.download_history.len()));
                    if ui.button("Clear History").clicked() {
                        self.v.download_history.clear();
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Grid::new("history_grid")
                        .striped(true)
                        .min_col_width(60.0)
                        .show(ui, |ui| {
                            ui.strong("Serial");
                            ui.strong("Version");
                            ui.strong("Title");
                            ui.strong("Size");
                            ui.strong("Time");
                            ui.end_row();

                            for entry in self.v.download_history.iter().rev() {
                                ui.label(&entry.title_id);
                                ui.label(&entry.pkg_id);
                                ui.label(&entry.title);
                                ui.label(ByteSize::b(entry.size).to_string());
                                let secs = entry.completed_at.elapsed().as_secs();
                                let time_str = if secs < 60 {
                                    format!("{secs}s ago")
                                } else if secs < 3600 {
                                    format!("{}m ago", secs / 60)
                                } else {
                                    format!("{}h {}m ago", secs / 3600, (secs % 3600) / 60)
                                };
                                ui.label(time_str);
                                ui.end_row();
                            }
                        });
                });
            });
        if !show {
            self.v.show_download_history_window = false;
        }
    }
}
