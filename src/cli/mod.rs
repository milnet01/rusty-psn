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

pub fn start_app(args: Args) {
    let runtime = Runtime::new().expect("Failed to start the async runtime");

    let _guard = runtime.enter();

    // Handle firmware download mode.
    if args.firmware {
        let client = crate::psn::build_psn_client().expect("Failed to build HTTP client");
        let console = parse_firmware_console(&args.firmware_console);
        let region = parse_firmware_region(&args.firmware_region);
        let destination = args.destination_path.clone().unwrap_or_else(|| PathBuf::from("pkgs/"));
        let silent = args.silent;

        runtime.block_on(async {
            download_firmware_cli(&client, console, region, &destination, silent).await;
        });
        return;
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
        args.titles[0].split(' ').map(|s| s.to_string()).collect()
    } else {
        eprintln!("Error: provide --titles (-t), --batch-file (-b), or --scan-rpcs3.");
        std::process::exit(1);
    };

    if silent_mode {
        info!("App started in silent mode!");
    }

    // Resolve RPCS3 executable for --install mode.
    let rpcs3_exe = if install_mode {
        let exe = rpcs3_exe_override.or_else(|| crate::rpcs3::auto_detect::detect_rpcs3().and_then(|i| i.executable));
        if exe.is_none() {
            eprintln!("Error: --install requires RPCS3. Use --rpcs3-executable or ensure RPCS3 is auto-detectable.");
            std::process::exit(1);
        }
        exe
    } else {
        None
    };

    // Build a single HTTP client for all PSN requests. Reusing the client avoids
    // redundant TLS handshakes and enables connection pooling across requests.
    let client = crate::psn::build_psn_client().expect("Failed to build HTTP client");

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
                Err(e) => match e {
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
                },
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
                                    DownloadError::HashMismatch(short_on_data) => {
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
                    // If the download succeeded and --install is set, install exactly the file it wrote.
                    if install_mode {
                        if let (Some(exe), Some(path)) = (rpcs3_exe.as_ref(), downloaded_file.as_ref()) {
                            if !silent_mode {
                                println!("  Installing {} to RPCS3...", path.display());
                            }
                            match runtime.block_on(crate::rpcs3::install_pkg(exe, path)) {
                                Ok(()) => {
                                    if !silent_mode {
                                        println!("  Installed successfully.");
                                    }
                                }
                                Err(e) => {
                                    error!("RPCS3 install failed: {e}");
                                    eprintln!("  Install failed: {e}");
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

        std::thread::sleep(std::time::Duration::from_secs(3));

        if !silent_mode {
            let _ = crossterm::execute!(
                std::io::stdout(),
                terminal::Clear(terminal::ClearType::All),
                cursor::MoveTo(0, 0)
            );
        }
    }
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

async fn download_firmware_cli(
    client: &reqwest::Client,
    console: crate::psn::firmware::FirmwareConsole,
    region: crate::psn::firmware::FirmwareRegion,
    destination: &std::path::Path,
    silent: bool,
) {
    if !silent {
        println!("Fetching {} firmware list for {}...", console, region);
    }

    let entries = match crate::psn::firmware::fetch_firmware(client, console, region).await {
        Ok(e) => e,
        Err(e) => {
            error!("Firmware fetch failed: {e}");
            eprintln!("Error: Firmware fetch failed: {e}");
            return;
        }
    };

    if entries.is_empty() {
        eprintln!("No firmware entries found.");
        return;
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
                return;
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
            return;
        }
    };

    let Some(fw) = fw else {
        eprintln!("No firmware entry selected.");
        return;
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
                break;
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
}
