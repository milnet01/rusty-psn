// On release builds, this hides the console window that's created on Windows.
#![cfg_attr(all(not(debug_assertions), feature = "egui"), windows_subsystem = "windows")]

use clap::Parser;
use flexi_logger::{Logger, LoggerHandle};
#[cfg(any(feature = "cli", target_os = "macos"))]
use std::path::PathBuf;
#[cfg(feature = "egui")]
use std::sync::Arc;
#[cfg(feature = "egui")]
use tokio::{runtime::Runtime, sync::Notify};

use log::info;

mod batch;
#[cfg(feature = "cli")]
mod cli;
#[cfg_attr(not(feature = "egui"), allow(dead_code))]
mod content;
#[cfg(feature = "egui")]
mod egui;
mod promise;
mod psn;
mod rpcs3;
mod security;
mod utils;

#[derive(Debug, Parser)]
#[clap(author, version, about)]
struct Args {
    #[cfg(feature = "cli")]
    #[clap(
        short,
        long,
        help = "The serial(s) you want to search for, in quotes and separated by spaces. Can be given more than once"
    )]
    titles: Vec<String>,
    #[cfg(feature = "cli")]
    #[clap(
        short = 'b',
        long,
        conflicts_with = "titles",
        help = "Path to a text file with serials (one per line, # for comments)"
    )]
    batch_file: Option<PathBuf>,
    #[cfg(feature = "cli")]
    #[clap(
        short,
        long,
        help = "Downloads all available updates printing only errors, without needing user intervention."
    )]
    silent: bool,
    #[cfg(feature = "cli")]
    #[clap(short, long, help = "Target folder to save the downloaded update files to.")]
    destination_path: Option<PathBuf>,
    #[cfg(feature = "cli")]
    #[clap(
        long,
        default_value = "serial-title",
        value_parser = ["serial-only", "serial-title", "title-serial"],
        help = "Directory format: serial-only, serial-title, or title-serial"
    )]
    directory_format: String,
    #[cfg(feature = "cli")]
    #[clap(long, help = "Path to RPCS3 config directory (containing games.yml).")]
    rpcs3_config_dir: Option<PathBuf>,
    #[cfg(feature = "cli")]
    #[clap(
        long,
        conflicts_with_all = ["titles", "batch_file"],
        help = "Scan RPCS3 library for available updates."
    )]
    scan_rpcs3: bool,
    #[cfg(feature = "cli")]
    #[clap(
        long,
        conflicts_with_all = ["titles", "batch_file", "scan_rpcs3", "install"],
        help = "Download firmware instead of game updates."
    )]
    firmware: bool,
    #[cfg(feature = "cli")]
    #[clap(long, default_value = "PS3", help = "Firmware console: PS3, PS4, PS5, PSVita")]
    firmware_console: String,
    #[cfg(feature = "cli")]
    #[clap(
        long,
        default_value = "US",
        help = "Firmware region: US, EU, JP, KR, UK, MX, AU, SA, TW, RU, CN"
    )]
    firmware_region: String,
    #[cfg(feature = "cli")]
    #[clap(long, help = "Auto-install downloaded PKGs to RPCS3 after download.")]
    install: bool,
    #[cfg(feature = "cli")]
    #[clap(long, help = "Path to RPCS3 executable (AppImage, binary, or launch script).")]
    rpcs3_executable: Option<PathBuf>,
    #[clap(
        long,
        help = "Disables writing the program's log to a .log file. Don't use if you need help."
    )]
    no_log_file: bool,
}

fn main() {
    // Before anything else, while no other thread can be reading the environment.
    #[cfg(all(feature = "egui", target_os = "linux"))]
    let dropped_wayland = drop_unreachable_wayland();

    let args = Args::parse();
    let _logger_handle = init_log(args.no_log_file);

    #[cfg(all(feature = "egui", target_os = "linux"))]
    if dropped_wayland {
        log::warn!("WAYLAND_DISPLAY names a socket nothing answers on; using X11 instead");
    }

    #[cfg(feature = "cli")]
    {
        info!("starting cli app");
        if !cli::start_app(args) {
            // process::exit skips destructors, so the log is flushed first.
            _logger_handle.flush();
            std::process::exit(1);
        }
    }

    #[cfg(feature = "egui")]
    {
        info!("starting egui app");

        // Execute tokio runtime in its own thread.
        // Prevents egui blocking the same thread that tokio runtime is running on,
        // which can lead to network and io tasks being blocked when the application
        // is minimised or otherwise suspended by egui.
        // Tokio runtime is required for all async operations; no recovery possible without it.
        let rt = Runtime::new().expect("Failed to start the async runtime");
        let rt_handle = rt.handle().clone();
        let notify_main = Arc::new(Notify::new());
        let notify_thread = notify_main.clone();
        let rt_thread = std::thread::spawn(move || {
            rt.block_on(async {
                notify_thread.notified().await; // Wait for a shutdown signal.
            })
        });

        let icon =
            eframe::icon_data::from_png_bytes(include_bytes!("../resources/icon-256.png")).expect("Failed to load app icon");

        let native_options = eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_inner_size([800.0, 600.0])
                .with_min_inner_size([600.0, 400.0])
                .with_icon(Arc::new(icon)),
            ..Default::default()
        };

        // Every search and download needs this client, so a failure is reported here, once,
        // rather than at the first click.
        let psn_client = match psn::build_psn_client() {
            Ok(client) => client,
            Err(e) => {
                log::error!("Failed to build HTTP client: {e}");
                rfd::MessageDialog::new()
                    .set_level(rfd::MessageLevel::Error)
                    .set_title("rusty-psn")
                    .set_description(format!("rusty-psn could not set up its network connection: {e}"))
                    .show();
                notify_main.notify_one();
                let _ = rt_thread.join();
                return;
            }
        };

        eframe::run_native(
            "rusty-psn",
            native_options,
            Box::new(|cc| Ok(Box::new(egui::UpdatesApp::new(cc, rt_handle, psn_client)))),
        )
        .expect("Failed to run egui app");

        // Signal runtime on a separate thread to shutdown gracefully.
        notify_main.notify_one();
        let _ = rt_thread.join();
    }
}

/// winit picks Wayland whenever WAYLAND_DISPLAY is set, and once that connection fails it cannot
/// try X11 (RPSN-0130). So when the socket takes no connection and DISPLAY is set, WAYLAND_DISPLAY
/// is removed before winit looks. Returns whether it was.
#[cfg(all(feature = "egui", target_os = "linux"))]
fn drop_unreachable_wayland() -> bool {
    let var = |name| std::env::var_os(name).filter(|value| !value.is_empty());
    let unreachable = wayland_unreachable(
        var("WAYLAND_DISPLAY").as_deref(),
        var("WAYLAND_SOCKET").is_some(),
        var("DISPLAY").is_some(),
        var("XDG_RUNTIME_DIR").as_deref(),
    );
    if unreachable {
        std::env::remove_var("WAYLAND_DISPLAY");
    }
    unreachable
}

/// Whether winit would choose a Wayland display that takes no connection while X11 is there.
/// The socket is found as libwayland finds it: WAYLAND_DISPLAY itself when absolute, otherwise
/// under XDG_RUNTIME_DIR. A WAYLAND_SOCKET handed down by a parent is left alone.
#[cfg(all(feature = "egui", target_os = "linux"))]
fn wayland_unreachable(
    wayland_display: Option<&std::ffi::OsStr>,
    wayland_socket: bool,
    x11_display: bool,
    runtime_dir: Option<&std::ffi::OsStr>,
) -> bool {
    let Some(wayland_display) = wayland_display else {
        return false;
    };
    if wayland_socket || !x11_display {
        return false;
    }
    let display = std::path::Path::new(wayland_display);
    let socket = if display.is_absolute() {
        Some(display.to_path_buf())
    } else {
        runtime_dir.map(|dir| std::path::Path::new(dir).join(display))
    };
    !socket.is_some_and(|path| std::os::unix::net::UnixStream::connect(path).is_ok())
}

#[cfg(target_os = "macos")]
fn init_log(no_log_file: bool) -> LoggerHandle {
    let mut logger = Logger::try_with_str("info").expect("Failed to create logger");

    if no_log_file {
        logger = logger.do_not_log();
    } else {
        // Falls back to current directory if platform data dir is unavailable.
        let mut logs_dir = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
        logs_dir.push("rusty-psn");

        match std::fs::create_dir_all(&logs_dir) {
            Ok(_) => {} // Logger not started yet, can't log here.
            Err(e) => match e.kind() {
                std::io::ErrorKind::AlreadyExists => {}
                _ => panic!("{}", e),
            },
        }

        logger = logger.log_to_file(flexi_logger::FileSpec::default().directory(logs_dir));
    }

    logger
        .duplicate_to_stdout(flexi_logger::Duplicate::Error)
        .start()
        .expect("Failed to start logger!")
}

#[cfg(not(target_os = "macos"))]
fn init_log(no_log_file: bool) -> LoggerHandle {
    let mut logger = Logger::try_with_str("info").expect("Failed to create logger");

    if no_log_file {
        logger = logger.do_not_log();
    } else {
        logger = logger.log_to_file(flexi_logger::FileSpec::default());
    }

    logger
        .duplicate_to_stdout(flexi_logger::Duplicate::Error)
        .start()
        .expect("Failed to start logger!")
}

#[cfg(all(test, feature = "cli"))]
mod tests {
    use super::Args;
    use clap::Parser;

    fn parses(args: &[&str]) -> bool {
        Args::try_parse_from(std::iter::once("rusty-psn").chain(args.iter().copied())).is_ok()
    }

    #[test]
    fn a_misspelt_directory_format_is_refused() {
        // It used to fall back to the default without a word (RPSN-0119).
        assert!(parses(&["-t", "NPUB30826", "--directory-format", "title-serial"]));
        assert!(!parses(&["-t", "NPUB30826", "--directory-format", "titel-serial"]));
    }

    #[test]
    fn two_sources_of_serials_are_refused() {
        // One used to win silently, in a fixed order the help never stated (RPSN-0119).
        assert!(!parses(&["--firmware", "-t", "NPUB30826"]));
        assert!(!parses(&["--scan-rpcs3", "-t", "NPUB30826"]));
        assert!(!parses(&["--batch-file", "list.txt", "-t", "NPUB30826"]));
        assert!(!parses(&["--firmware", "--install"]));
        assert!(parses(&["--scan-rpcs3", "--install"]));
    }
}

#[cfg(all(test, feature = "egui", target_os = "linux"))]
mod wayland_tests {
    use super::wayland_unreachable;
    use std::ffi::OsStr;
    use std::os::unix::net::UnixListener;

    #[test]
    fn a_dead_wayland_socket_gives_way_to_x11() {
        // The window app used to panic at start instead (RPSN-0130).
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = Some(dir.path().as_os_str());
        let _compositor = UnixListener::bind(dir.path().join("wayland-live")).unwrap();
        let live = Some(OsStr::new("wayland-live"));
        let dead = Some(OsStr::new("wayland-dead"));

        assert!(wayland_unreachable(dead, false, true, runtime_dir));
        assert!(!wayland_unreachable(live, false, true, runtime_dir));
        let absolute = dir.path().join("wayland-live");
        assert!(!wayland_unreachable(Some(absolute.as_os_str()), false, true, None));
        // No runtime folder means libwayland finds no socket either.
        assert!(wayland_unreachable(live, false, true, None));
    }

    #[test]
    fn wayland_is_kept_when_there_is_nothing_to_fall_back_to() {
        let dead = Some(OsStr::new("wayland-dead"));
        assert!(!wayland_unreachable(dead, false, false, None));
        assert!(!wayland_unreachable(dead, true, true, None));
        assert!(!wayland_unreachable(None, false, true, None));
    }
}
