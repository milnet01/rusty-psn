//! The Messages panel: one foldable list at the foot of every page, holding everything the app
//! reports (RPSN-0157). Nothing pops up; a message only adds a row inside the panel's fixed box.

use super::*;

/// How many messages the panel keeps. The oldest goes when a new one would pass it.
const MESSAGE_CAP: usize = 200;

/// How tall the unfolded list is, in points. Fixed, so a new message never moves the page above it.
const LIST_HEIGHT: f32 = 120.0;

/// What a message reports. Each is drawn with a status icon, a word and a colour (RPSN-0144).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MessageLevel {
    Info,
    Success,
    Warning,
    Error,
}

impl MessageLevel {
    fn status(self) -> (StatusKind, String) {
        match self {
            MessageLevel::Info => (StatusKind::Queued, tr!("messages-level-note")),
            MessageLevel::Success => (StatusKind::Done, tr!("messages-level-done")),
            MessageLevel::Warning => (StatusKind::Attention, tr!("messages-level-warning")),
            MessageLevel::Error => (StatusKind::Failed, tr!("messages-level-error")),
        }
    }
}

pub(super) struct Message {
    level: MessageLevel,
    text: String,
}

/// The panel's contents and whether it is folded.
#[derive(Default)]
pub(super) struct MessageLog {
    messages: VecDeque<Message>,
    folded: bool,
}

impl MessageLog {
    pub(super) fn push(&mut self, level: MessageLevel, text: String) {
        if self.messages.len() == MESSAGE_CAP {
            self.messages.pop_front();
        }
        self.messages.push_back(Message { level, text });
    }

    pub(super) fn len(&self) -> usize {
        self.messages.len()
    }
}

impl UpdatesApp {
    /// Must come before CentralPanel, and after the status bar so it sits above it.
    pub(super) fn draw_messages_panel(&mut self, ui: &mut egui::Ui) {
        let log = &mut self.v.message_log;
        egui::Panel::bottom("messages").resizable(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                let arrow = if log.folded { "\u{25B6}" } else { "\u{25BC}" }; // ▶ or ▼
                if ui
                    .button(format!("{arrow} {}", tr!("messages-title", count = log.len().to_string())))
                    .clicked()
                {
                    log.folded = !log.folded;
                }
                if !log.folded
                    && ui
                        .add_enabled(log.len() > 0, egui::Button::new(tr!("common-clear")))
                        .clicked()
                {
                    log.messages.clear();
                }
            });
            if log.folded {
                return;
            }

            egui::ScrollArea::vertical()
                .min_scrolled_height(LIST_HEIGHT)
                .max_height(LIST_HEIGHT)
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if log.messages.is_empty() {
                        ui.weak(tr!("messages-empty"));
                    }
                    for message in &log.messages {
                        let (kind, word) = message.level.status();
                        ui.horizontal_wrapped(|ui| {
                            status_pill(ui, &word, kind);
                            ui.label(&message.text);
                        });
                    }
                });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_keeps_only_the_newest_messages() {
        let mut log = MessageLog::default();
        for i in 0..MESSAGE_CAP + 5 {
            log.push(MessageLevel::Info, format!("message {i}"));
        }
        assert_eq!(log.len(), MESSAGE_CAP);
        assert_eq!(log.messages.front().map(|m| m.text.as_str()), Some("message 5"));
        assert_eq!(
            log.messages.back().map(|m| m.text.clone()),
            Some(format!("message {}", MESSAGE_CAP + 4))
        );
    }
}
