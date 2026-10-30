# Versioning overrides — rusty-psn

Answers the questions `~/.claude/standards/versioning.md` leaves to each
project: which surfaces count as breaking (§ 3), and what makes it `1.0`
(§ 4). Everything else follows that standard unchanged.

**Status:** agreed by the owner (2026-10-22).

## What makes it 1.0

The fork's first release is `1.0.0` (owner, 2026-10-22, RPSN-0162). It
starts past `0.x`, so a breaking change bumps the MAJOR.

## Breaking surfaces

A release that makes any of these stop working for an existing user is
breaking. "Stop working" includes the app refusing or resetting a saved
file, losing anything stored in it, or changing a layout other tools rely
on.

- **Command-line options and exit code** — the options `--help` lists
  (`Args` in `src/main.rs`) and the exit code `STANDARDS.md` § 7.3 states.
  Scripts and AI sessions call the command line.
- **Saved window-app state** — the settings and their keys and the download
  history (`UpdatesApp::save` in `src/egui/mod.rs`), and the window's size
  and position (eframe's, and `src/egui/placement.rs` on KDE Wayland). eframe
  keeps them in a folder named from the viewport's `app_id`, or, with none
  set, from the app name `src/main.rs` passes to `eframe::run_native`; a
  `persistence_path` overrides both. Older saved state must still load from
  there.
- **Download folders** — the three folder layouts (`DirectoryFormat` in
  `src/utils.rs`), where a download lands under each, and the layout and
  folder each build uses when the user chose none. A changed layout strands
  earlier downloads and breaks resuming them.
- **Batch files** — what `parse_batch_file` in `src/batch.rs` accepts: one
  serial per line, `#` starting a comment, any letter case, with or without
  dashes.
- **Results export** — the CSV columns and the JSON fields
  (`src/egui/export.rs`).
- **Keyboard shortcuts** — Ctrl+F and Ctrl+S (Cmd on macOS), and Escape
  (`src/egui/chrome.rs`).
- **The Flatpak app id** — `io.github.milnet01.RustyPsn`
  (`packaging/flatpak/`). A new id starts with no settings and no history.

A surface missing from this list is still a surface: if users rely on it and
it breaks, the release was breaking (`versioning.md` § 3).

**Not a surface:** the text the command line prints (`STANDARDS.md` § 7.2).
Its wording and layout may change in any release; scripts rely on the options
and the exit code (owner, 2026-10-30, RPSN-0096).

## Cold-eyes loop log

Kept in [`docs/reviews/versioning-overrides-loop-log.md`](../reviews/versioning-overrides-loop-log.md).
