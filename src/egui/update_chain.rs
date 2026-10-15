//! "Update All Outdated" installs the updates into RPCS3 as well as downloading them (RPSN-0058).
//!
//! RPCS3 installs an update only onto the version its TARGET_APP_VER names, and a first patch
//! with none only onto a version no newer than itself (Crypto/unpkg.cpp, RPCS3 commit 6fe3c87).
//! So every update newer than the installed version is downloaded, and once all of a game's
//! have arrived they install oldest first, each checked against PARAM.SFO before the next.

use super::*;

/// One game's updates on their way into RPCS3.
pub(super) struct UpdateChain {
    title_id: String,
    title: String,
    /// Every update newer than the installed version, oldest first.
    packages: Vec<PackageInfo>,
    /// The installs, once every download has arrived: the last version installed, or the
    /// version that failed and why.
    promise: Option<Promise<ChainResult>>,
}

type ChainResult = Result<(String, crate::rpcs3::InstallOutcome), (String, crate::rpcs3::Rpcs3Error)>;

/// Where a chain stands against the downloads it waits for.
#[derive(Debug, PartialEq)]
enum ChainState {
    /// Still downloading.
    Waiting,
    /// One of its downloads failed, so nothing installs.
    DownloadFailed,
    /// Every file is here, oldest first, with its version.
    Ready(Vec<(PathBuf, String)>),
}

/// Updates newer than `installed`, oldest first.
pub(super) fn updates_after(installed: &str, packages: &[PackageInfo]) -> Vec<PackageInfo> {
    let mut newer: Vec<PackageInfo> = packages
        .iter()
        .filter(|pkg| crate::rpcs3::compare_versions(installed, &pkg.version) == std::cmp::Ordering::Less)
        .cloned()
        .collect();
    newer.sort_by(|a, b| crate::rpcs3::compare_versions(&a.version, &b.version));
    newer
}

fn chain_state(
    title_id: &str,
    packages: &[PackageInfo],
    completed: &HashMap<(String, String), PathBuf>,
    failed: &[FailedDownload],
) -> ChainState {
    if failed
        .iter()
        .any(|f| f.title_id == title_id && packages.iter().any(|p| p.id() == f.pkg_id))
    {
        return ChainState::DownloadFailed;
    }
    let files: Option<Vec<(PathBuf, String)>> = packages
        .iter()
        .map(|pkg| {
            completed
                .get(&(title_id.to_string(), pkg.id()))
                .map(|path| (path.clone(), pkg.version.clone()))
        })
        .collect();
    files.map_or(ChainState::Waiting, ChainState::Ready)
}

/// Install `files` in order, stopping at the first that fails.
async fn install_in_order(exe: PathBuf, serial: String, files: Vec<(PathBuf, String)>, dev_hdd0: Option<PathBuf>) -> ChainResult {
    let mut last = (String::new(), crate::rpcs3::InstallOutcome::Unconfirmed);
    for (path, version) in files {
        let check = match &dev_hdd0 {
            Some(dev_hdd0) => crate::rpcs3::InstallCheck::AppVersion {
                dev_hdd0: dev_hdd0.clone(),
                serial: serial.clone(),
                version: Some(version.clone()),
            },
            None => crate::rpcs3::InstallCheck::Unconfirmed,
        };
        let outcome = crate::rpcs3::install_pkg(&exe, &path, check)
            .await
            .map_err(|e| (version.clone(), e))?;
        last = (version, outcome);
    }
    Ok(last)
}

impl UpdatesApp {
    /// Download every update each outdated game is missing, and when RPCS3 is set in Settings,
    /// install them once they arrive. `games` is (title id, title, installed version, updates).
    pub(super) fn start_update_all(&mut self, games: Vec<(String, String, String, Vec<PackageInfo>)>) {
        let can_install = self.settings.rpcs3_executable.is_some();
        for (title_id, title, installed, packages) in games {
            let packages = updates_after(&installed, &packages);
            for pkg in &packages {
                self.queue_download_no_ui(title_id.clone(), title.clone(), pkg.clone());
            }
            if can_install && !packages.is_empty() && !self.v.update_chains.iter().any(|c| c.title_id == title_id) {
                self.v.update_chains.push(UpdateChain {
                    title_id,
                    title,
                    packages,
                    promise: None,
                });
            }
        }
        if !can_install {
            self.show_notifications(
                "Downloading only: set RPCS3's executable in Settings to install the updates too.",
                MessageLevel::Info,
            );
        }
    }

    /// Start each chain whose downloads have all arrived, and report each that finished.
    pub(super) fn handle_update_chains(&mut self, messages: &mut Vec<(String, MessageLevel)>) {
        let mut i = 0;
        while i < self.v.update_chains.len() {
            let chain = &self.v.update_chains[i];
            match &chain.promise {
                Some(promise) if promise.ready().is_some() => {
                    let chain = self.v.update_chains.remove(i);
                    let result = chain.promise.expect("matched Some above").block_and_take();
                    self.report_chain(&chain.title_id, &chain.title, result, messages);
                    continue;
                }
                Some(_) => {}
                None => match chain_state(
                    &chain.title_id,
                    &chain.packages,
                    &self.v.completed_paths,
                    &self.v.failed_downloads,
                ) {
                    ChainState::Waiting => {}
                    ChainState::DownloadFailed => {
                        let chain = self.v.update_chains.remove(i);
                        messages.push((
                            format!("Not installing {}: one of its updates failed to download.", chain.title_id),
                            MessageLevel::Warning,
                        ));
                        continue;
                    }
                    ChainState::Ready(files) => self.start_chain(i, files),
                },
            }
            i += 1;
        }
    }

    fn start_chain(&mut self, index: usize, files: Vec<(PathBuf, String)>) {
        let Some(exe) = self.settings.rpcs3_executable.clone() else {
            return;
        };
        let dev_hdd0 = self
            .settings
            .rpcs3_config_dir
            .as_deref()
            .map(crate::rpcs3::config::resolve_dev_hdd0);
        let serial = self.v.update_chains[index].title_id.clone();
        let ctx = self.v.egui_ctx.clone();
        let _guard = self
            .v
            .rt_handle
            .as_ref()
            .expect("rt_handle set in UpdatesApp::new before any UI code runs")
            .enter();
        self.v.update_chains[index].promise = Some(Promise::spawn_async(async move {
            let result = install_in_order(exe, serial, files, dev_hdd0).await;
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
            result
        }));
    }

    fn report_chain(&mut self, title_id: &str, title: &str, result: ChainResult, messages: &mut Vec<(String, MessageLevel)>) {
        match result {
            Ok((version, crate::rpcs3::InstallOutcome::Confirmed)) => {
                messages.push((format!("{title_id} {title} is now at version {version} in RPCS3."), MessageLevel::Success));
                if let Some(game) = self.v.rpcs3_installed_games.iter_mut().find(|g| g.serial == title_id) {
                    game.installed_version = Some(version.clone());
                }
                self.v.rpcs3_installed_pkgs.insert((title_id.to_string(), version));
            }
            Ok((_, crate::rpcs3::InstallOutcome::Unconfirmed)) => messages.push((
                format!(
                    "RPCS3 finished installing the updates for {title_id}, but rusty-psn could not check them. Set the RPCS3 folder in Settings to check it."
                ),
                MessageLevel::Warning,
            )),
            Err((version, e)) => {
                error!("Installing update {version} of {title_id} failed: {e}");
                messages.push((
                    format!("Installing update {version} of {title_id} failed, so later updates were not installed: {e}"),
                    MessageLevel::Error,
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(version: &str) -> PackageInfo {
        PackageInfo {
            url: String::new(),
            size: 0,
            version: version.to_string(),
            sha1sum: String::new(),
            hash_whole_file: false,
            manifest_url: String::new(),
            offset: 0,
            part_number: None,
            min_system_version: None,
        }
    }

    #[test]
    fn every_newer_update_is_taken_oldest_first() {
        let listed = [package("01.03"), package("01.01"), package("01.02"), package("01.10")];
        let versions: Vec<_> = updates_after("01.01", &listed).into_iter().map(|p| p.version).collect();
        assert_eq!(versions, ["01.02", "01.03", "01.10"]);
    }

    #[test]
    fn a_chain_waits_for_every_download_and_stops_on_a_failure() {
        let packages = updates_after("01.00", &[package("01.01"), package("01.02")]);
        let mut completed = HashMap::new();
        completed.insert(("BLES00001".to_string(), "01.02".to_string()), PathBuf::from("b.pkg"));
        assert_eq!(chain_state("BLES00001", &packages, &completed, &[]), ChainState::Waiting);

        completed.insert(("BLES00001".to_string(), "01.01".to_string()), PathBuf::from("a.pkg"));
        assert_eq!(
            chain_state("BLES00001", &packages, &completed, &[]),
            ChainState::Ready(vec![
                (PathBuf::from("a.pkg"), "01.01".to_string()),
                (PathBuf::from("b.pkg"), "01.02".to_string()),
            ])
        );

        let failed = FailedDownload {
            title_id: "BLES00001".to_string(),
            pkg_id: "01.02".to_string(),
            title: String::new(),
            pkg: package("01.02"),
            incomplete: false,
        };
        assert_eq!(
            chain_state("BLES00001", &packages, &completed, &[failed]),
            ChainState::DownloadFailed
        );
    }
}
