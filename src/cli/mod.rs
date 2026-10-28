use std::io::Write;
use std::path::PathBuf;

use bytesize::ByteSize;
use crossterm::{cursor, terminal};
use poll_promise::Promise;
use tokio::runtime::Runtime;

use crate::psn::*;
use crate::Args;

pub fn start_app(args: Args) {
    let runtime = Runtime::new().unwrap();

    let _guard = runtime.enter();

    // Serials may be passed as several -t flags and/or as one space-separated string.
    let titles = args.titles.iter().flat_map(|t| t.split_whitespace());
    let silent_mode = args.silent;
    let destination_path = args.destination_path.unwrap_or_else(|| PathBuf::from("pkgs/"));

    if silent_mode {
        info!("App started in silent mode!");
    }

    let update_info = {
        let mut info = Vec::new();

        let promises = titles
            .map(|t| (t.to_string(), Promise::spawn_async(UpdateInfo::get_info(t.to_string()))))
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
                    UpdateError::XmlEncodingError(e) => {
                        error!("Failed to deserialize response for {id}: {e}");
                        println!("{id}: Error parsing response from PSN, try again later ({e}).");
                    }
                    UpdateError::ManifestParsing(e) => {
                        error!("Failed to deserialize manifest response for {id}: {e}");
                        println!("{id}: Error parsing manifest response from PSN, try again later ({e}).");
                    }
                },
            }
        }

        info
    };

    for update in update_info {
        let title = update.titles.first().cloned().unwrap_or_else(|| {
            warn!("Failed to get update's title: Last pkg's info didn't contain a title");
            String::from("Untitled")
        });

        if !silent_mode {
            clear_screen();

            let total_size = ByteSize::b(update.packages.iter().map(|pkg| pkg.size).sum());

            println!(
                "[{}] {} - {} - {} update(s) ({})",
                update.platform_variant,
                update.title_id,
                &title,
                update.packages.len(),
                total_size
            );

            for (i, pkg) in update.packages.iter().enumerate() {
                println!("  {i}. {} ({})", pkg.id(), ByteSize::b(pkg.size));
            }
        }

        let mut response = String::new();
        let mut updates_to_fetch = Vec::new();

        if !silent_mode {
            info!("Querying user for wanted updates for {}", update.title_id);
            println!("\nEnter the updates you want to download, separated by a space (ie: 1 3 4 5). An empty input will download all updates.");

            std::io::stdin().read_line(&mut response).unwrap();
            response = response.trim().to_string();

            info!("User input was '{}'", response);

            if !response.is_empty() {
                updates_to_fetch = response
                    .split_whitespace()
                    .filter_map(|s| s.parse::<usize>().ok())
                    .filter(|idx| *idx < update.packages.len())
                    .collect();

                updates_to_fetch.sort_unstable();
                updates_to_fetch.dedup();
            }

            let updates = if updates_to_fetch.is_empty() {
                update.packages.iter().map(|pkg| pkg.id()).collect::<Vec<_>>()
            } else {
                updates_to_fetch.iter().map(|&idx| update.packages[idx].id()).collect()
            }
            .join(", ");

            info!("Downloading updates {updates}");

            clear_screen();
            println!("{} {} - Downloading update(s): {}", update.title_id, title, updates);
        }

        for (idx, pkg) in update.packages.iter().enumerate() {
            if !updates_to_fetch.is_empty() && !updates_to_fetch.contains(&idx) {
                continue;
            }

            let (tx, mut rx) = tokio::sync::mpsc::channel(10);
            let serial = update.title_id.clone();
            let download_path = destination_path.clone();

            let dpkg = pkg.clone();
            let dtitle = title.clone();

            let promise = Promise::spawn_async(async move { dpkg.start_download(tx, download_path, serial, dtitle).await });

            let mut stdout = std::io::stdout();
            let mut downloaded = 0;

            crossterm::execute!(stdout, cursor::SavePosition).unwrap();

            // Replaces the current download's status line.
            let mut print_status = |status: &str, finished: bool| {
                if silent_mode {
                    return;
                }

                crossterm::execute!(
                    stdout,
                    cursor::RestorePosition,
                    terminal::Clear(terminal::ClearType::CurrentLine),
                    cursor::SavePosition
                )
                .unwrap();

                if finished {
                    println!("        {} - {title} | {status}", pkg.id());
                } else {
                    print!("        {} - {title} | {status}", pkg.id());
                }

                stdout.flush().unwrap();
            };

            // Blocks until the next status arrives, and ends once the download task
            // drops its sender, so every status (including the final one) is shown.
            while let Some(status) = rx.blocking_recv() {
                match status {
                    DownloadStatus::Progress(bytes) => {
                        downloaded += bytes;
                        print_status(&format!("{} / {}", ByteSize::b(downloaded), ByteSize::b(pkg.size)), false);
                    }
                    DownloadStatus::Verifying => print_status("Verifying checksum... ", false),
                    DownloadStatus::DownloadSuccess => print_status("Download completed successfully. ", true),
                    DownloadStatus::DownloadFailure => print_status("Download failed. ", true),
                }
            }

            if let Err(e) = promise.block_and_take() {
                match e {
                    DownloadError::HashMismatch(short_on_data) => {
                        error!(
                            "Download of {} {} failed: hash mismatch. (short on data: {})",
                            update.title_id,
                            pkg.id(),
                            short_on_data
                        );
                        println!("Error downloading update: hash mismatch on downloaded file.");

                        if short_on_data {
                            println!("The downloaded file is smaller than expected. Please try again later, as Sony's servers can sometimes be unreliable");
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
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_secs(3));

        if !silent_mode {
            clear_screen();
        }
    }
}

fn clear_screen() {
    crossterm::execute!(
        std::io::stdout(),
        terminal::Clear(terminal::ClearType::All),
        cursor::MoveTo(0, 0)
    )
    .unwrap();
}
