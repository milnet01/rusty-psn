//! Putting the window back where it was closed, on KDE Plasma under Wayland (RPSN-0156).
//!
//! eframe saves and restores the window's size everywhere, and its position on X11, Windows and
//! macOS. Wayland tells an app nothing of where its window is and ignores an app that moves
//! itself, so eframe stores no position there. KWin knows, and a one-shot KWin script is the one
//! way to reach it: the script's `print()` lands in the user journal, and a script may move a
//! window. The owner's OneUp app does the same (`oneup/gui/placement.py`). Other Wayland desktops
//! keep the size only.
//!
//! Off Wayland, a first start moves the window to the middle of its screen. Left to itself, Windows
//! places a new window down and to the right, which can push it off a small screen (RPSN-0164).

use super::*;

use std::io::Write;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The key the saved position is stored under, beside eframe's own window settings.
pub(super) const STORAGE_KEY: &str = "kwin_frame_position";

/// The name KWin loads each script under. One script runs at a time, and each is unloaded after.
const SCRIPT_NAME: &str = "rusty_psn_place";

/// How long to wait for KWin's answer to reach the journal. OneUp's figure.
const ANSWER_TIMEOUT: Duration = Duration::from_millis(1500);

/// KWin can move only a window it already lists, so the restore waits this long after the first frame.
const RESTORE_DELAY: Duration = Duration::from_millis(300);

impl UpdatesApp {
    /// On Wayland, restores the saved position once, after the first frame, and saves the position
    /// when the window is asked to close. Elsewhere, centres the window on a first start.
    pub(super) fn handle_window_placement(&mut self, ctx: &egui::Context) {
        if !on_wayland() {
            self.centre_on_first_start(ctx);
            return;
        }
        if !self.v.placement_restored {
            self.v.placement_restored = true;
            if let Some((x, y)) = self.v.frame_position {
                // Off the window thread: the window has to be shown before KWin can move it.
                std::thread::spawn(move || {
                    std::thread::sleep(RESTORE_DELAY);
                    if !run_kwin_script(&restore_script(std::process::id(), x, y)) {
                        info!("KWin could not be asked to restore the window position");
                    }
                });
            }
        }
        // Asked now, while the window is still shown: KWin no longer lists it once it closes.
        if ctx.input(|i| i.viewport().close_requested()) {
            if let Some(position) = frame_position() {
                self.v.frame_position = Some(position);
            }
        }
    }

    /// On a first start, moves the window to the middle of its screen as soon as egui knows both
    /// sizes. A later start keeps the position eframe saved.
    fn centre_on_first_start(&mut self, ctx: &egui::Context) {
        if !self.v.centre_pending {
            return;
        }
        let (outer, monitor) = ctx.input(|i| (i.viewport().outer_rect, i.viewport().monitor_size));
        if let (Some(outer), Some(monitor)) = (outer, monitor) {
            self.v.centre_pending = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(centred(outer.size(), monitor)));
        }
    }
}

/// The top-left corner that centres a window of `window` size on a screen of `monitor` size,
/// kept on screen when the window is the larger.
fn centred(window: egui::Vec2, monitor: egui::Vec2) -> egui::Pos2 {
    ((monitor - window) / 2.0).max(egui::Vec2::ZERO).to_pos2()
}

/// Whether winit chose Wayland, by winit's own rule: a non-empty `WAYLAND_DISPLAY` or `WAYLAND_SOCKET`
/// (`platform_impl/linux/mod.rs`, winit 0.30). `main` removes an unreachable `WAYLAND_DISPLAY` first.
fn on_wayland() -> bool {
    cfg!(target_os = "linux")
        && ["WAYLAND_DISPLAY", "WAYLAND_SOCKET"]
            .iter()
            .any(|name| std::env::var(name).is_ok_and(|value| !value.is_empty()))
}

/// Where KWin has this app's main window frame, or None if it cannot say.
fn frame_position() -> Option<(i32, i32)> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let marker = format!("rusty_psn_frame_{}_{:x}", std::process::id(), now.as_nanos());
    let since = format!("@{}", now.as_secs().saturating_sub(2));
    if !run_kwin_script(&position_script(std::process::id(), &marker)) {
        return None;
    }
    let deadline = Instant::now() + ANSWER_TIMEOUT;
    loop {
        let output = crate::sandbox::host_command("journalctl")
            .args(["--user", "-b", "--since", &since, "-o", "cat"])
            .output()
            .ok()?;
        let position = frame_from_journal(&String::from_utf8_lossy(&output.stdout), &marker);
        if position.is_some() || Instant::now() >= deadline {
            return position;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The x, y KWin printed after `marker`, or None if no line carries two integers after it.
fn frame_from_journal(text: &str, marker: &str) -> Option<(i32, i32)> {
    let line = text.lines().find(|line| line.contains(marker))?;
    let mut numbers = line.split(marker).nth(1)?.split_whitespace().map(str::parse::<i32>);
    match (numbers.next(), numbers.next()) {
        (Some(Ok(x)), Some(Ok(y))) => Some((x, y)),
        _ => None,
    }
}

/// The script that prints this app's main window frame position after `marker`.
/// Rounded, because KWin's geometry is fractional under display scaling.
fn position_script(pid: u32, marker: &str) -> String {
    format!(
        r#"var wins = workspace.windowList();
for (var i = 0; i < wins.length; i++) {{
    var c = wins[i];
    if (c.pid !== {pid} || c.transientFor) continue;
    print("{marker}", Math.round(c.frameGeometry.x), Math.round(c.frameGeometry.y));
    break;
}}
"#
    )
}

/// The script that moves this app's main window frame to x, y, keeping its size. Clamped to the usable
/// area of the screen holding that point, or of the window's own screen when that monitor is gone.
/// `KWin.PlacementArea`, not `workspace.PlacementArea`, which Plasma 6 leaves undefined (OneUp, 2026-10-10).
fn restore_script(pid: u32, x: i32, y: i32) -> String {
    format!(
        r#"var x = {x};
var y = {y};
var wins = workspace.windowList();
for (var i = 0; i < wins.length; i++) {{
    var c = wins[i];
    if (c.pid !== {pid} || c.transientFor) continue;
    var g = c.frameGeometry, area = null;
    for (var j = 0; j < workspace.screens.length; j++) {{
        var s = workspace.screens[j].geometry;
        if (x >= s.x && x < s.x + s.width && y >= s.y && y < s.y + s.height)
            area = workspace.clientArea(KWin.PlacementArea, workspace.screens[j], workspace.currentDesktop);
    }}
    if (area === null) area = workspace.clientArea(KWin.PlacementArea, c);
    x = Math.max(area.x, Math.min(x, area.x + area.width - g.width));
    y = Math.max(area.y, Math.min(y, area.y + area.height - g.height));
    c.frameGeometry = {{ x: x, y: y, width: g.width, height: g.height }};
    break;
}}
"#
    )
}

/// Loads, runs and unloads a one-shot KWin script over D-Bus. True means KWin accepted it, never
/// that it did anything: `dbus-send` fails only when the call did not land, such as with no KWin.
fn run_kwin_script(js: &str) -> bool {
    let mut file = match tempfile::Builder::new()
        .prefix("rusty_psn_place_")
        .suffix(".js")
        .tempfile_in(crate::sandbox::host_visible_temp_dir())
    {
        Ok(file) => file,
        Err(e) => {
            warn!("Could not create a KWin script file: {e}");
            return false;
        }
    };
    if let Err(e) = file.write_all(js.as_bytes()).and_then(|()| file.flush()) {
        warn!("Could not write a KWin script file: {e}");
        return false;
    }
    let Some(path) = file.path().to_str() else {
        return false;
    };
    let dbus = |args: &[&str]| {
        crate::sandbox::host_command("dbus-send")
            .args([
                "--session",
                "--dest=org.kde.KWin",
                "--print-reply",
                "--reply-timeout=3000",
                "/Scripting",
            ])
            .args(args)
            .output()
    };
    let loaded = dbus(&[
        "org.kde.kwin.Scripting.loadScript",
        &format!("string:{path}"),
        &format!("string:{SCRIPT_NAME}"),
    ]);
    let _ = dbus(&["org.kde.kwin.Scripting.start"]);
    let _ = dbus(&["org.kde.kwin.Scripting.unloadScript", &format!("string:{SCRIPT_NAME}")]);
    // The temporary file is deleted when `file` drops, after KWin has read it.
    loaded.is_ok_and(|output| output.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs one frame in which egui reports the window's outer rectangle and the screen size, and
    /// returns the window moves the app asked for.
    fn moves_after_frame(app: &mut UpdatesApp, ctx: &egui::Context) -> Vec<egui::ViewportCommand> {
        let info = egui::ViewportInfo {
            // The Windows test PC's first start, before RPSN-0164: off the right and bottom.
            outer_rect: Some(egui::Rect::from_min_size(egui::pos2(104.0, 104.0), egui::vec2(1256.0, 899.0))),
            monitor_size: Some(egui::vec2(1280.0, 1024.0)),
            ..Default::default()
        };
        let mut input = egui::RawInput::default();
        input.viewports.insert(egui::ViewportId::ROOT, info);
        let mut output = ctx.run_ui(input, |ui| app.centre_on_first_start(&ui.ctx().clone()));
        // Nothing draws these textures; epaint panics if they are dropped unapplied.
        output.textures_delta.clear();
        output
            .viewport_output
            .remove(&egui::ViewportId::ROOT)
            .map(|viewport| viewport.commands)
            .unwrap_or_default()
            .into_iter()
            .filter(|command| matches!(command, egui::ViewportCommand::OuterPosition(_)))
            .collect()
    }

    #[test]
    fn a_first_start_centres_the_window_once() {
        let ctx = egui::Context::default();
        let mut app = UpdatesApp::default();
        app.v.centre_pending = true;
        assert_eq!(
            moves_after_frame(&mut app, &ctx),
            vec![egui::ViewportCommand::OuterPosition(egui::pos2(12.0, 62.5))]
        );
        assert!(moves_after_frame(&mut app, &ctx).is_empty());
    }

    #[test]
    fn a_later_start_leaves_the_saved_position_alone() {
        let ctx = egui::Context::default();
        let mut app = UpdatesApp::default();
        assert!(moves_after_frame(&mut app, &ctx).is_empty());
    }

    #[test]
    fn a_window_larger_than_the_screen_starts_at_its_corner() {
        assert_eq!(
            centred(egui::vec2(1400.0, 900.0), egui::vec2(1280.0, 1024.0)),
            egui::pos2(0.0, 62.0)
        );
    }

    #[test]
    fn frame_from_journal_reads_the_marked_line() {
        let journal = "kwin_wayland: something else\njs: rusty_psn_frame_7_ab 120 -40\njs: rusty_psn_frame_9_cd 1 2\n";
        assert_eq!(frame_from_journal(journal, "rusty_psn_frame_7_ab"), Some((120, -40)));
    }

    #[test]
    fn frame_from_journal_rejects_a_line_without_two_integers() {
        assert_eq!(frame_from_journal("js: m 120\n", "m"), None);
        assert_eq!(frame_from_journal("js: m 120.5 40\n", "m"), None);
        assert_eq!(frame_from_journal("js: other 1 2\n", "m"), None);
    }

    #[test]
    fn scripts_name_only_this_process_and_the_given_values() {
        let position = position_script(4321, "rusty_psn_frame_4321_ff");
        assert!(position.contains("c.pid !== 4321"));
        assert!(position.contains(r#"print("rusty_psn_frame_4321_ff","#));

        let restore = restore_script(4321, -1920, 35);
        assert!(restore.contains("var x = -1920;"));
        assert!(restore.contains("var y = 35;"));
        assert!(restore.contains("c.pid !== 4321"));
    }
}
