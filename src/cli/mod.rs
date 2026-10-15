use log::{error, info, warn};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicU8;
use std::sync::Arc;

use crate::promise::Promise;
use bytesize::ByteSize;
use crossterm::{cursor, terminal};
use tokio::runtime::Runtime;

use crate::psn::*;
use crate::utils::DirectoryFormat;
use crate::Args;

/// Read one line of the user's answer, trimmed. `None` when nothing could be read (end of
/// input or a read error), so a caller never mistakes it for the empty answer, which means
/// "everything" or "the default".
fn read_answer(input: &mut impl std::io::BufRead) -> Option<String> {
    let mut line = String::new();
    match input.read_line(&mut line) {
        Ok(n) if n > 0 => Some(line.trim().to_string()),
        _ => None,
    }
}

/// Runs the command line. Returns `false` when any search, download or install it was asked for failed,
/// so the caller can exit non-zero. A serial with no updates, or a prompt left unanswered, is not a failure.
pub fn start_app(args: Args) -> bool {
    let runtime = Runtime::new().expect("Failed to start the async runtime");

    let _guard = runtime.enter();

    // Handle firmware download mode.
    if args.firmware {
        let client = crate::psn::build_psn_client().expect("Failed to build HTTP client");
        let console = parse_firmware_console(&args.firmware_console);
        let region = parse_firmware_region(&args.firmware_region);
        let destination = args.destination_path.clone().unwrap_or_else(|| PathBuf::from("pkgs/"));
        let silent = args.silent;

        return runtime.block_on(download_firmware_cli(&client, console, region, &destination, silent));
    }

    let silent_mode = args.silent;
    let install_mode = args.install;
    let rpcs3_exe_override = args.rpcs3_executable.clone();
    let destination_path = args.destination_path.unwrap_or_else(|| PathBuf::from("pkgs/"));
    let directory_format = match args.directory_format.as_str() {
        "serial-only" => DirectoryFormat::SerialOnly,
        "title-serial" => DirectoryFormat::TitleSerial,
        _ => DirectoryFormat::SerialTitle,
    };

    // Build serial list from --scan-rpcs3, --batch-file, or --titles.
    let titles: Vec<String> = if args.scan_rpcs3 {
        // Scan RPCS3 library for installed game serials.
        let installation = if let Some(config_dir) = &args.rpcs3_config_dir {
            let dev_hdd0_path = crate::rpcs3::config::resolve_dev_hdd0(config_dir);
            crate::rpcs3::Rpcs3Installation {
                config_dir: config_dir.clone(),
                dev_hdd0_path,
                executable: args.rpcs3_executable.clone(),
            }
        } else if let Some(detected) = crate::rpcs3::auto_detect::detect_rpcs3() {
            detected
        } else {
            eprintln!("Error: RPCS3 not found. Use --rpcs3-config-dir to specify the path.");
            std::process::exit(1);
        };

        match crate::rpcs3::scan_library(&installation) {
            Ok(games) => {
                if !silent_mode {
                    println!("RPCS3 library: {} game(s) found.", games.len());
                }
                games.iter().map(|g| g.serial.clone()).collect()
            }
            Err(e) => {
                error!("RPCS3 scan failed: {e}");
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    } else if let Some(batch_path) = &args.batch_file {
        match crate::batch::parse_batch_file(batch_path) {
            Ok(result) => {
                for (line_num, text, reason) in &result.warnings {
                    warn!("Batch line {line_num} skipped ('{text}'): {reason}");
                    if !silent_mode {
                        println!("Warning: line {line_num} skipped ('{text}'): {reason}");
                    }
                }
                if !silent_mode {
                    println!("Loaded {} serial(s) from batch file.", result.serials.len());
                }
                result.serials
            }
            Err(e) => {
                error!("Failed to read batch file: {e}");
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    } else if !args.titles.is_empty() {
        serials_from_titles(&args.titles)
    } else {
        eprintln!("Error: provide --titles (-t), --batch-file (-b), or --scan-rpcs3.");
        std::process::exit(1);
    };

    if silent_mode {
        info!("App started in silent mode!");
    }

    // Resolve RPCS3 executable for --install mode.
    // dev_hdd0, where an install is checked, is auto-detected only together with the executable,
    // so both belong to one RPCS3 (docs/specs/RPSN-0137-headless-confirmed-install.md § 4.3).
    let (rpcs3_exe, rpcs3_dev_hdd0) = if install_mode {
        let detected = if rpcs3_exe_override.is_none() {
            crate::rpcs3::auto_detect::detect_rpcs3()
        } else {
            None
        };
        let dev_hdd0 = match &args.rpcs3_config_dir {
            Some(dir) => Some(crate::rpcs3::config::resolve_dev_hdd0(dir)),
            None => detected.as_ref().map(|i| i.dev_hdd0_path.clone()),
        };
        let exe = rpcs3_exe_override.or_else(|| detected.and_then(|i| i.executable));
        if exe.is_none() {
            eprintln!("Error: --install requires RPCS3. Use --rpcs3-executable or ensure RPCS3 is auto-detectable.");
            std::process::exit(1);
        }
        (exe, dev_hdd0)
    } else {
        (None, None)
    };

    // Build a single HTTP client for all PSN requests. Reusing the client avoids
    // redundant TLS handshakes and enables connection pooling across requests.
    let client = crate::psn::build_psn_client().expect("Failed to build HTTP client");

    let mut failures = 0usize;

    let update_info = {
        let mut info = Vec::new();

        let promises = titles
            .iter()
            .map(|t| {
                let c = client.clone();
                let serial = t.to_string();
                let serial_for_key = serial.clone();
                (
                    serial_for_key,
                    Promise::spawn_async(async move { UpdateInfo::get_info(serial, &c).await }),
                )
            })
            .collect::<Vec<(String, Promise<Result<UpdateInfo, UpdateError>>)>>();

        if !silent_mode {
            println!("Searching for updates...\n");
        }

        for (id, promise) in promises {
            info!("Checking in on search promises");

            match promise.block_and_take() {
                Ok(i) => {
                    info!("Successfully search for updates for {id}");
                    info.push(i);
                }
                Err(e) => {
                    if !matches!(e, UpdateError::NoUpdatesAvailable) {
                        failures += 1;
                    }
                    match e {
                        UpdateError::UnhandledErrorResponse(e) => {
                            error!("Unexpected error received in response from PSN: {e}");
                            println!("{id}: PSN returned an unexpected error: {e}.");
                        }
                        UpdateError::InvalidSerial => {
                            error!("Invalid serial for updates query {id}");
                            println!("{id}: The provided serial didn't give any results, double-check your input.");
                        }
                        UpdateError::NoUpdatesAvailable => {
                            warn!("No updates available for serial {id}");
                            println!("{id}: The provided serial doesn't have any available updates.");
                        }
                        UpdateError::Reqwest(e) => {
                            error!("reqwest error on updates query: {e}");
                            println!("{id}: There was an error on the request: {e}.");
                        }
                        UpdateError::XmlParsing(e) => {
                            error!("Failed to deserialize response for {id}: {e}");
                            println!("{id}: Error parsing response from PSN, try again later ({e}).");
                        }
                        UpdateError::ManifestParsing(e) => {
                            error!("Failed to deserialize manifest response for {id}: {e}");
                            println!("{id}: Error parsing manifest response from PSN, try again later ({e}).");
                        }
                        UpdateError::Security(e) => {
                            error!("Security check failed for {id}: {e}");
                            println!("{id}: Security check failed: {e}.");
                        }
                        UpdateError::ResponseTooLarge(size) => {
                            error!("Response too large for {id}: {size} bytes");
                            println!("{id}: Server response too large ({size} bytes).");
                        }
                    }
                }
            }
        }

        info
    };

    for update in update_info {
        let title = {
            if let Some(title) = update.titles.first() {
                title.clone()
            } else {
                warn!("Failed to get update's title: Last pkg's info didn't contain a title");
                String::from("Untitled")
            }
        };

        if !silent_mode {
            let _ = crossterm::execute!(
                std::io::stdout(),
                terminal::Clear(terminal::ClearType::All),
                cursor::MoveTo(0, 0)
            );

            let total_size = {
                let mut total = 0;

                for pkg in update.packages.iter() {
                    total += pkg.size;
                }

                ByteSize::b(total)
            };

            println!(
                "[{}] {} - {} - {} update(s) ({})",
                update.platform_variant,
                update.title_id,
                title,
                update.packages.len(),
                total_size
            );

            for (i, pkg) in update.packages.iter().enumerate() {
                println!("  {i}. {} ({})", pkg.id(), ByteSize::b(pkg.size));
            }
        }

        let mut updates_to_fetch = Vec::new();

        if !silent_mode {
            info!("Querying user for wanted updates for {}", update.title_id);
            println!("\nEnter the updates you want to download, separated by a space (ie: 1 3 4 5). An empty input will download all updates.");

            let response = match read_answer(&mut std::io::stdin().lock()) {
                Some(answer) => answer,
                None => {
                    println!("No answer read; skipping {}.", update.title_id);
                    continue;
                }
            };

            info!("User input was '{}'", response);

            match parse_update_selection(&response, update.packages.len()) {
                Some(selection) => updates_to_fetch = selection,
                None => {
                    println!("No valid update numbers in '{response}'; skipping {}.", update.title_id);
                    failures += 1;
                    continue;
                }
            }

            let updates = {
                let mut updates = String::new();

                if updates_to_fetch.is_empty() {
                    for (i, pkg) in update.packages.iter().enumerate() {
                        updates.push_str(&pkg.id());

                        if i < update.packages.len() - 1 {
                            updates.push_str(", ");
                        }
                    }
                } else {
                    for (i, update_idx) in updates_to_fetch.iter().enumerate() {
                        updates.push_str(&update.packages[*update_idx].id().to_string());

                        if i < updates_to_fetch.len() - 1 {
                            updates.push_str(", ");
                        }
                    }
                }

                updates
            };

            info!("Downloading updates {updates}");

            let _ = crossterm::execute!(
                std::io::stdout(),
                terminal::Clear(terminal::ClearType::All),
                cursor::MoveTo(0, 0)
            );
            println!("{} {} - Downloading update(s): {}", update.title_id, title, updates);
        }

        // Parts of a multi-part PS4 update that downloaded in this run; see the join below.
        let mut parts_downloaded = 0;
        for (idx, pkg) in update.packages.iter().enumerate() {
            if !updates_to_fetch.is_empty() && !updates_to_fetch.contains(&idx) {
                continue;
            }

            let max_retries = 3u32;
            let mut attempt = 0u32;

            loop {
                attempt += 1;

                let (tx, mut rx) = tokio::sync::mpsc::channel(10);
                let serial = update.title_id.clone();
                let download_path = destination_path.clone();

                let dpkg = pkg.clone();
                let dtitle = title.clone();

                // CLI downloads are not cancellable — pass a signal that's always CANCEL_NONE.
                let cancel = Arc::new(AtomicU8::new(CANCEL_NONE));
                let dl_client = client.clone();
                let promise = Promise::spawn_async(async move {
                    dpkg.start_download(tx, download_path, serial, dtitle, directory_format, cancel, &dl_client)
                        .await
                });

                let mut stdout = std::io::stdout();
                let mut downloaded = 0u64;
                let mut should_retry = false;
                let mut downloaded_file: Option<PathBuf> = None;

                let _ = crossterm::execute!(stdout, cursor::SavePosition);

                loop {
                    match promise.ready() {
                        Some(result) => {
                            if let Ok(path) = result {
                                downloaded_file = Some(path.clone());
                            }
                            if let Err(e) = result {
                                match e {
                                    DownloadError::HashMismatch { short_on_data, .. } => {
                                        if *short_on_data && attempt <= max_retries {
                                            // Sony dropped the connection. Auto-retry with resume.
                                            info!(
                                                "Short transfer for {} {}, retrying ({}/{})",
                                                update.title_id,
                                                pkg.id(),
                                                attempt,
                                                max_retries
                                            );
                                            if !silent_mode {
                                                println!(
                                                    "\n        Transfer interrupted, retrying ({}/{max_retries})...",
                                                    attempt
                                                );
                                            }
                                            should_retry = true;
                                        } else {
                                            error!(
                                                "Download of {} {} failed: hash mismatch. (short on data: {})",
                                                update.title_id,
                                                pkg.id(),
                                                short_on_data
                                            );
                                            println!("Error downloading update: hash mismatch on downloaded file.");
                                            e.discard_kept_file();

                                            if *short_on_data {
                                                println!("The downloaded file is smaller than expected. Max retries exceeded.");
                                            }
                                        }
                                    }
                                    DownloadError::Tokio(e) => {
                                        error!("Download of {} {} failed: {e}", update.title_id, pkg.id());
                                        println!("Error downloading update: {e}.")
                                    }
                                    DownloadError::Reqwest(e) => {
                                        error!("Download of {} {} failed: {e}", update.title_id, pkg.id());
                                        println!("Error downloading update: {e}.")
                                    }
                                    DownloadError::Security(e) => {
                                        error!("Security check failed for {} {}: {e}", update.title_id, pkg.id());
                                        println!("Security check failed: {e}.")
                                    }
                                    // Packages are checked by hash instead; only firmware reports this.
                                    DownloadError::Incomplete { received, expected } => {
                                        error!(
                                            "Download of {} {} ended at {received} of {expected} bytes",
                                            update.title_id,
                                            pkg.id()
                                        );
                                        println!("Error downloading update: it ended at {received} of {expected} bytes.")
                                    }
                                    DownloadError::Cancelled { .. } => {
                                        // CLI downloads are never cancelled, but handle gracefully.
                                        info!("Download cancelled for {} {}", update.title_id, pkg.id());
                                        println!("Download cancelled.")
                                    }
                                }
                            }

                            break;
                        }
                        None => {
                            if let Ok(status) = rx.try_recv() {
                                match status {
                                    DownloadStatus::Progress(bytes) => {
                                        downloaded += bytes;

                                        if !silent_mode {
                                            let _ = crossterm::execute!(
                                                stdout,
                                                cursor::RestorePosition,
                                                terminal::Clear(terminal::ClearType::CurrentLine),
                                                cursor::SavePosition
                                            );
                                            print!(
                                                "        {} - {title} | {} / {}",
                                                pkg.id(),
                                                ByteSize::b(downloaded),
                                                ByteSize::b(pkg.size)
                                            );
                                            let _ = stdout.flush();
                                        }
                                    }
                                    DownloadStatus::Resuming(offset) => {
                                        if !silent_mode {
                                            let _ = crossterm::execute!(
                                                stdout,
                                                cursor::RestorePosition,
                                                terminal::Clear(terminal::ClearType::CurrentLine),
                                                cursor::SavePosition
                                            );
                                            print!("        {} - {title} | Resuming from {}...", pkg.id(), ByteSize::b(offset));
                                            let _ = stdout.flush();
                                        }
                                    }
                                    DownloadStatus::Verifying => {
                                        if !silent_mode {
                                            let _ = crossterm::execute!(
                                                stdout,
                                                cursor::RestorePosition,
                                                terminal::Clear(terminal::ClearType::CurrentLine),
                                                cursor::SavePosition
                                            );
                                            print!("        {} - {title} | Verifying checksum... ", pkg.id());
                                            let _ = stdout.flush();
                                        }
                                    }
                                    DownloadStatus::DownloadSuccess => {
                                        if !silent_mode {
                                            let _ = crossterm::execute!(
                                                stdout,
                                                cursor::RestorePosition,
                                                terminal::Clear(terminal::ClearType::CurrentLine),
                                                cursor::SavePosition
                                            );
                                            println!("        {} - {title} | Download completed successfully. ", pkg.id());
                                            let _ = stdout.flush();
                                        }
                                    }
                                    DownloadStatus::DownloadFailure => {
                                        if !silent_mode {
                                            let _ = crossterm::execute!(
                                                stdout,
                                                cursor::RestorePosition,
                                                terminal::Clear(terminal::ClearType::CurrentLine),
                                                cursor::SavePosition
                                            );
                                            println!("        {} - {title} | Download failed. ", pkg.id());
                                            let _ = stdout.flush();
                                        }
                                    }
                                }
                            } else {
                                // Avoid busy-waiting when no progress updates are available
                                std::thread::sleep(std::time::Duration::from_millis(50));
                            }
                        }
                    }
                }

                if !should_retry {
                    if downloaded_file.is_none() {
                        failures += 1;
                    } else if pkg.part_number.is_some() {
                        parts_downloaded += 1;
                    }
                    // If the download succeeded and --install is set, install exactly the file it wrote.
                    if install_mode {
                        if let (Some(exe), Some(path)) = (rpcs3_exe.as_ref(), downloaded_file.as_ref()) {
                            if !silent_mode {
                                println!("  Installing {} to RPCS3...", path.display());
                            }
                            let check = match &rpcs3_dev_hdd0 {
                                Some(dev_hdd0) => crate::rpcs3::InstallCheck::AppVersion {
                                    dev_hdd0: dev_hdd0.clone(),
                                    serial: update.title_id.clone(),
                                    version: Some(pkg.version.clone()),
                                },
                                None => crate::rpcs3::InstallCheck::Unconfirmed,
                            };
                            match runtime.block_on(crate::rpcs3::install_pkg(exe, path, check)) {
                                Ok(crate::rpcs3::InstallOutcome::Confirmed) => {
                                    if !silent_mode {
                                        println!("  Installed successfully.");
                                    }
                                }
                                Ok(crate::rpcs3::InstallOutcome::Unconfirmed) => {
                                    if !silent_mode {
                                        println!(
                                            "  RPCS3 finished; the install was not checked (pass --rpcs3-config-dir to check it)."
                                        );
                                    }
                                }
                                Err(e) => {
                                    error!("RPCS3 install failed: {e}");
                                    eprintln!("  Install failed: {e}");
                                    failures += 1;
                                }
                            }
                        }
                    }
                    break; // Download succeeded or failed permanently — move to next package.
                }
                // Loop continues: the partial file persists on disk, so start_download()
                // will automatically resume via HTTP Range headers.
            }
        }

        // A multi-part PS4 update is usable only as one file, so join its parts once every one has
        // downloaded, as the window app does (RPSN-0087).
        let join = parts_to_join(&update.packages, parts_downloaded);
        if join == Join::All {
            if !silent_mode {
                println!("  Joining {parts_downloaded} parts into one package...");
            }
            // Room for every progress report, since nothing reads them here.
            let (tx, _progress) = tokio::sync::mpsc::channel(update.packages.len() + 1);
            match runtime.block_on(update.merge_parts(tx, &destination_path, directory_format)) {
                Ok(()) => {
                    if !silent_mode {
                        println!("  Parts joined.");
                    }
                }
                Err(e) => {
                    error!("Joining the parts of {} failed: {e:?}", update.title_id);
                    eprintln!("  Joining the parts failed: {e}.");
                    failures += 1;
                }
            }
        } else if join == Join::Partial && !silent_mode {
            println!("  Not every part was downloaded, so the parts were not joined.");
        }

        if !silent_mode {
            // A pause to read this title's results before the screen is cleared.
            std::thread::sleep(std::time::Duration::from_secs(3));
            let _ = crossterm::execute!(
                std::io::stdout(),
                terminal::Clear(terminal::ClearType::All),
                cursor::MoveTo(0, 0)
            );
        }
    }

    failures == 0
}

fn parse_firmware_console(s: &str) -> crate::psn::firmware::FirmwareConsole {
    use crate::psn::firmware::FirmwareConsole;
    match s.to_uppercase().as_str() {
        "PS3" => FirmwareConsole::PS3,
        "PS4" => FirmwareConsole::PS4,
        "PS5" => FirmwareConsole::PS5,
        "PSVITA" | "VITA" => FirmwareConsole::PSVita,
        _ => {
            eprintln!("Unknown console: {s}. Valid options: PS3, PS4, PS5, PSVita");
            std::process::exit(1);
        }
    }
}

fn parse_firmware_region(s: &str) -> crate::psn::firmware::FirmwareRegion {
    use crate::psn::firmware::FirmwareRegion;
    match s.to_uppercase().as_str() {
        "US" => FirmwareRegion::US,
        "EU" => FirmwareRegion::EU,
        "JP" => FirmwareRegion::JP,
        "KR" => FirmwareRegion::KR,
        "UK" => FirmwareRegion::UK,
        "MX" => FirmwareRegion::MX,
        "AU" => FirmwareRegion::AU,
        "SA" => FirmwareRegion::SA,
        "TW" => FirmwareRegion::TW,
        "RU" => FirmwareRegion::RU,
        "CN" => FirmwareRegion::CN,
        _ => {
            eprintln!("Unknown region: {s}. Valid options: US, EU, JP, KR, UK, MX, AU, SA, TW, RU, CN");
            std::process::exit(1);
        }
    }
}

/// Returns `false` when the firmware was not downloaded. An unanswered prompt is not a failure.
async fn download_firmware_cli(
    client: &reqwest::Client,
    console: crate::psn::firmware::FirmwareConsole,
    region: crate::psn::firmware::FirmwareRegion,
    destination: &std::path::Path,
    silent: bool,
) -> bool {
    if !silent {
        println!("Fetching {} firmware list for {}...", console, region);
    }

    let entries = match crate::psn::firmware::fetch_firmware(client, console, region).await {
        Ok(e) => e,
        Err(e) => {
            error!("Firmware fetch failed: {e}");
            eprintln!("Error: Firmware fetch failed: {e}");
            return false;
        }
    };

    if entries.is_empty() {
        eprintln!("No firmware entries found.");
        return false;
    }

    if !silent {
        println!("\nAvailable firmware:");
        for (i, fw) in entries.iter().enumerate() {
            let size_str = fw
                .size
                .map(|s| ByteSize::b(s).to_string())
                .unwrap_or_else(|| "--".to_string());
            println!("  {i}. {} v{} ({size_str})", fw.fw_type, fw.version);
        }
    }

    // In silent mode, download the first Full firmware. Otherwise prompt.
    let fw = if silent {
        entries
            .iter()
            .find(|e| e.fw_type == crate::psn::firmware::FirmwareType::Full)
            .or(entries.first())
    } else {
        println!("\nEnter the number to download (empty for first Full firmware):");
        let response = match read_answer(&mut std::io::stdin().lock()) {
            Some(answer) => answer,
            None => {
                println!("No answer read; nothing downloaded.");
                return true;
            }
        };
        let response = response.as_str();
        if response.is_empty() {
            entries
                .iter()
                .find(|e| e.fw_type == crate::psn::firmware::FirmwareType::Full)
                .or(entries.first())
        } else if let Ok(idx) = response.parse::<usize>() {
            entries.get(idx)
        } else {
            eprintln!("Invalid selection.");
            return false;
        }
    };

    let Some(fw) = fw else {
        eprintln!("No firmware entry selected.");
        return false;
    };

    if !silent {
        println!(
            "\nDownloading {} v{} ({})...",
            fw.fw_type,
            fw.version,
            fw.size
                .map(|s| ByteSize::b(s).to_string())
                .unwrap_or_else(|| "unknown size".to_string())
        );
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    let cancel = Arc::new(AtomicU8::new(CANCEL_NONE));
    let fw_clone = fw.clone();
    let dest = destination.to_path_buf();
    let dl_client = client.clone();

    let promise = Promise::spawn_async(async move { fw_clone.start_download(tx, dest, cancel, &dl_client).await });

    let mut stdout = std::io::stdout();
    let mut downloaded = 0u64;
    let total_size = fw.size.unwrap_or(0);

    let _ = crossterm::execute!(stdout, cursor::SavePosition);

    loop {
        match promise.ready() {
            Some(result) => {
                if let Err(e) = result {
                    error!("Firmware download failed: {e:?}");
                    eprintln!("\nFirmware download failed: {e:?}");
                } else if !silent {
                    println!("\nFirmware download completed successfully.");
                }
                break result.is_ok();
            }
            None => {
                if let Ok(status) = rx.try_recv() {
                    match status {
                        DownloadStatus::Progress(bytes) => {
                            downloaded += bytes;
                            if !silent {
                                let _ = crossterm::execute!(
                                    stdout,
                                    cursor::RestorePosition,
                                    terminal::Clear(terminal::ClearType::CurrentLine),
                                    cursor::SavePosition
                                );
                                if total_size > 0 {
                                    print!("  {} / {}", ByteSize::b(downloaded), ByteSize::b(total_size));
                                } else {
                                    print!("  {} downloaded", ByteSize::b(downloaded));
                                }
                                let _ = stdout.flush();
                            }
                        }
                        DownloadStatus::DownloadSuccess => {
                            if !silent {
                                println!("\n  Download completed successfully.");
                            }
                        }
                        DownloadStatus::DownloadFailure if !silent => {
                            println!("\n  Download failed.");
                        }
                        _ => {}
                    }
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }
    }
}

/// Collect the serials from every `-t` value, split on any whitespace, normalised and deduplicated
/// as a batch file's are. Validation stays with the search, which reports a bad serial by name.
/// Whether a downloaded update's parts can be joined into one package.
#[derive(Debug, PartialEq)]
enum Join {
    /// Not a multi-part update, or no part arrived.
    None,
    /// Some parts arrived and some did not, so there is nothing whole to join.
    Partial,
    /// Every part arrived.
    All,
}

fn parts_to_join(packages: &[crate::psn::PackageInfo], parts_downloaded: usize) -> Join {
    let multipart = !packages.is_empty() && packages.iter().all(|p| p.part_number.is_some());
    match (multipart, parts_downloaded) {
        (false, _) | (true, 0) => Join::None,
        (true, n) if n == packages.len() => Join::All,
        (true, _) => Join::Partial,
    }
}

fn serials_from_titles(titles: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    titles
        .iter()
        .flat_map(|t| t.split_whitespace())
        .map(crate::psn::parse_title_id)
        .filter(|serial| seen.insert(serial.clone()))
        .collect()
}

/// Parse the update numbers typed at the download prompt. An empty selection means every update.
/// Returns `None` when any entry is not a listed update number, so a typo never means "everything".
fn parse_update_selection(input: &str, count: usize) -> Option<Vec<usize>> {
    let mut selection = Vec::new();
    for token in input.split_whitespace() {
        selection.push(token.parse::<usize>().ok().filter(|idx| *idx < count)?);
    }
    selection.sort_unstable();
    selection.dedup();
    Some(selection)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(number: Option<usize>) -> crate::psn::PackageInfo {
        crate::psn::PackageInfo {
            url: String::new(),
            size: 0,
            version: "01.01".to_string(),
            sha1sum: String::new(),
            hash_whole_file: true,
            manifest_url: String::new(),
            offset: 0,
            part_number: number,
        }
    }

    #[test]
    fn parts_are_joined_only_when_every_one_arrived() {
        // The command line could not join a multi-part PS4 update at all (RPSN-0087).
        let parts = [part(Some(1)), part(Some(2)), part(Some(3))];
        assert_eq!(parts_to_join(&parts, 3), Join::All);
        assert_eq!(parts_to_join(&parts, 2), Join::Partial);
        assert_eq!(parts_to_join(&parts, 0), Join::None);
        assert_eq!(
            parts_to_join(&[part(None)], 0),
            Join::None,
            "a single package is never joined"
        );
    }

    #[test]
    fn read_answer_end_of_input_is_no_answer() {
        // End of input must not read as the empty answer, which downloads everything (RPSN-0039).
        assert_eq!(read_answer(&mut std::io::Cursor::new("")), None);
    }

    #[test]
    fn read_answer_trims_a_line() {
        assert_eq!(read_answer(&mut std::io::Cursor::new("1 3\n")), Some("1 3".to_string()));
        assert_eq!(read_answer(&mut std::io::Cursor::new("\n")), Some(String::new()));
    }

    #[test]
    fn update_selection_empty_means_all() {
        assert_eq!(parse_update_selection("", 3), Some(vec![]));
    }

    #[test]
    fn update_selection_parses_numbers() {
        assert_eq!(parse_update_selection("2 0 2", 3), Some(vec![0, 2]));
    }

    #[test]
    fn update_selection_rejects_invalid_input() {
        // "n" or an out-of-range number must not fall through to "download everything".
        assert_eq!(parse_update_selection("n", 3), None);
        assert_eq!(parse_update_selection("5", 3), None);
        assert_eq!(parse_update_selection("1,2", 3), None);
    }

    #[test]
    fn serials_from_titles_reads_every_flag_and_any_whitespace() {
        // `-t A -t B` used to drop B, and a double space gave an empty serial (RPSN-0105).
        let titles = vec!["bles00001  npeb00002".to_string(), "BLES-00001\tBLUS00003 ".to_string()];
        assert_eq!(serials_from_titles(&titles), vec!["BLES00001", "NPEB00002", "BLUS00003"]);
    }
}
