//! Download progress on the app's taskbar icon (RPSN-0073).
//!
//! The owner chose this over a tray icon on 2026-10-15: under Wayland a window cannot hide itself
//! or come back from minimised, so the window minimises as usual and its taskbar icon shows how
//! far the downloads are. KDE Plasma reads the `com.canonical.Unity.LauncherEntry` D-Bus signal
//! (`applets/taskmanager/smartlauncherbackend.cpp`); Windows has `ITaskbarList3`. Elsewhere this
//! does nothing.

use super::*;

/// What the taskbar icon last showed, and the platform's way of telling it.
#[derive(Default)]
pub(super) struct TaskbarProgress {
    /// Percent shown, or None for no progress bar.
    shown: Option<u8>,
    #[cfg(target_os = "linux")]
    launcher: Option<std::sync::mpsc::Sender<Option<u8>>>,
    #[cfg(windows)]
    windows: Option<windows_taskbar::Taskbar>,
}

impl UpdatesApp {
    /// Shows the downloads' overall progress on the taskbar icon. Called from `logic`, which runs
    /// while the window is minimised too; tells the taskbar only when the whole percent changes.
    pub(super) fn update_taskbar_progress(&mut self, frame: &eframe::Frame) {
        let (progress, size, _) = self.download_totals();
        let percent = progress_percent(progress, size);
        if percent == self.v.taskbar.shown {
            return;
        }
        self.v.taskbar.shown = percent;

        #[cfg(target_os = "linux")]
        {
            let _ = frame;
            let launcher = self.v.taskbar.launcher.get_or_insert_with(start_launcher_entry);
            // A send fails only when the D-Bus thread has stopped; it has said why in the log.
            let _ = launcher.send(percent);
        }
        #[cfg(windows)]
        {
            if self.v.taskbar.windows.is_none() {
                self.v.taskbar.windows = windows_taskbar::Taskbar::new(frame);
            }
            if let Some(taskbar) = &self.v.taskbar.windows {
                taskbar.show(percent);
            }
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        let _ = frame;
    }
}

/// The whole percent of `size` that `progress` is, or None while nothing is downloading.
fn progress_percent(progress: u64, size: u64) -> Option<u8> {
    (size > 0).then(|| (progress.min(size) as u128 * 100 / size as u128) as u8)
}

/// The desktop entry KDE matches the signal to: the Flatpak's when running as one, else the one
/// `resources/rusty-psn.desktop` installs.
#[cfg(target_os = "linux")]
fn desktop_entry_uri(flatpak_id: Option<&str>) -> String {
    format!("application://{}.desktop", flatpak_id.unwrap_or("rusty-psn"))
}

/// The properties of one LauncherEntry `Update` signal: a progress bar at `percent`, or none.
#[cfg(target_os = "linux")]
fn launcher_entry_properties(percent: Option<u8>) -> HashMap<&'static str, zbus::zvariant::Value<'static>> {
    HashMap::from([
        ("progress", (f64::from(percent.unwrap_or(0)) / 100.0).into()),
        ("progress-visible", percent.is_some().into()),
    ])
}

/// Starts the thread that sends LauncherEntry signals, so D-Bus never holds up a frame.
#[cfg(target_os = "linux")]
fn start_launcher_entry() -> std::sync::mpsc::Sender<Option<u8>> {
    let (tx, rx) = std::sync::mpsc::channel::<Option<u8>>();
    let uri = desktop_entry_uri(std::env::var("FLATPAK_ID").ok().as_deref());
    let started = std::thread::Builder::new().name("taskbar-progress".into()).spawn(move || {
        let connection = match zbus::blocking::Connection::session() {
            Ok(connection) => connection,
            Err(e) => {
                info!("No session D-Bus for taskbar progress: {e}");
                return;
            }
        };
        for percent in rx {
            let body = (uri.as_str(), launcher_entry_properties(percent));
            if let Err(e) = connection.emit_signal(
                None::<&str>,
                "/io/github/milnet01/RustyPsn/LauncherEntry",
                "com.canonical.Unity.LauncherEntry",
                "Update",
                &body,
            ) {
                warn!("Could not show download progress on the taskbar: {e}");
            }
        }
    });
    if let Err(e) = started {
        warn!("Could not start the taskbar progress thread: {e}");
    }
    tx
}

#[cfg(windows)]
mod windows_taskbar {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{ITaskbarList3, TaskbarList, TBPF_NOPROGRESS, TBPF_NORMAL};

    /// The window's taskbar button. Used only on the window's own thread, where winit has
    /// already set up COM.
    pub(super) struct Taskbar {
        list: ITaskbarList3,
        hwnd: HWND,
    }

    impl Taskbar {
        pub(super) fn new(frame: &eframe::Frame) -> Option<Self> {
            let RawWindowHandle::Win32(handle) = frame.window_handle().ok()?.as_raw() else {
                return None;
            };
            let hwnd = HWND(handle.hwnd.get() as *mut core::ffi::c_void);
            // SAFETY: plain COM calls on the window's thread, with an interface the system provides.
            let list: ITaskbarList3 = unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER) }
                .inspect_err(|e| log::warn!("No taskbar progress: {e}"))
                .ok()?;
            // SAFETY: as above; HrInit must come before any other call.
            unsafe { list.HrInit() }.ok()?;
            Some(Self { list, hwnd })
        }

        pub(super) fn show(&self, percent: Option<u8>) {
            // SAFETY: `hwnd` is this app's live window, and these calls only change its taskbar button.
            let result = unsafe {
                match percent {
                    Some(percent) => self
                        .list
                        .SetProgressState(self.hwnd, TBPF_NORMAL)
                        .and_then(|()| self.list.SetProgressValue(self.hwnd, u64::from(percent), 100)),
                    None => self.list.SetProgressState(self.hwnd, TBPF_NOPROGRESS),
                }
            };
            if let Err(e) = result {
                log::warn!("Could not show download progress on the taskbar: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_a_whole_percent_and_none_while_idle() {
        assert_eq!(progress_percent(0, 0), None);
        assert_eq!(progress_percent(0, 200), Some(0));
        assert_eq!(progress_percent(199, 200), Some(99));
        assert_eq!(progress_percent(200, 200), Some(100));
        // A resumed file can report more than its size for a moment.
        assert_eq!(progress_percent(300, 200), Some(100));
        assert_eq!(progress_percent(u64::MAX / 2, u64::MAX), Some(49));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn launcher_entry_names_the_desktop_entry_kde_knows() {
        assert_eq!(desktop_entry_uri(None), "application://rusty-psn.desktop");
        assert_eq!(
            desktop_entry_uri(Some("io.github.milnet01.RustyPsn")),
            "application://io.github.milnet01.RustyPsn.desktop"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn launcher_entry_shows_a_fraction_or_hides_the_bar() {
        let shown = launcher_entry_properties(Some(42));
        assert_eq!(shown["progress"], zbus::zvariant::Value::from(0.42));
        assert_eq!(shown["progress-visible"], zbus::zvariant::Value::from(true));

        let hidden = launcher_entry_properties(None);
        assert_eq!(hidden["progress-visible"], zbus::zvariant::Value::from(false));
    }
}
