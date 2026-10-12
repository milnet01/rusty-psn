<!-- ants-roadmap-format: 1 -->
<!-- Generated from the Ants Terminal roadmap store. Edit it with roadmap_log; hand edits are discarded by the next write. -->
<!-- ants-roadmap-id-high-water: RPSN=82 -->
# rusty-psn — Roadmap

> **Format:** v1 — see roadmap-format.md.
> Every actionable bullet carries a stable ID; ID is identity,
> position is priority.

**Legend** — 📋 planned · 🚧 in progress · ✅ shipped · 💭 considered · 🚫 dropped

Imported from TODO.md on 2026-10-12, which stays readable with
`git show 0b7b623:TODO.md`. Each status was checked against the code
that day. Section priorities: P0 do first, P1 high, P2 medium, P3 nice
to have.

## 1. Modernize the Codebase [P0]

Foundational work that should be done before adding new features, as it reduces friction for everything that follows.

- ✅ [RPSN-0001] **Update Rust edition from 2018 to 2021 in `Cargo.toml`.**
  Checked against the code 2026-10-12: Done — Cargo.toml declares
  edition = "2021".
  Former TODO.md task 1.1.
  Kind: refactor.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0002] **Replace `#[macro_use] extern crate log` with explicit `use log::{info, warn, error}` imports.**
  Checked against the code 2026-10-12: Done — no #[macro_use] in src;
  explicit `use log::{error, info, warn}` in src/psn/mod.rs.
  Former TODO.md task 1.2.
  Kind: refactor.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0003] **Fix the `PlaformVariant` typo - rename to `PlatformVariant` across the codebase.**
  Checked against the code 2026-10-12: Done — `enum PlatformVariant`
  in src/psn/utils.rs; no misspelling remains.
  Former TODO.md task 1.3.
  Kind: refactor.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0004] **Add missing `#[cfg(test)]` to the `tests` module in `psn/mod.rs` (currently compiles in release builds).**
  Checked against the code 2026-10-12: Done — #[cfg(test)] mod tests
  in src/psn/mod.rs.
  Former TODO.md task 1.4.
  Kind: refactor.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0005] **Deduplicate XML parsing logic in `parser.rs`.**
  The `Event::Start` and `Event::Empty` branches for `<package>`
  elements have near-identical attribute extraction code (lines 48-88
  vs 114-148); extract into a shared function
  Checked against the code 2026-10-12: Done — shared
  parse_package_attributes() in src/psn/parser.rs serves both
  branches.
  Former TODO.md task 1.5.
  Kind: refactor.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0006] **Fix settings reset path inconsistency - non-macOS reset sets `/pkgs` (absolute root) instead of `pkgs/` (relative, matching the default).**
  Checked against the code 2026-10-12: Done — reset uses
  default_download_path(), which returns "pkgs/" off macOS.
  Former TODO.md task 1.6.
  Kind: refactor.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0007] **Fix busy-wait polling in CLI.**
  The download loop in `cli/mod.rs` calls `promise.ready()` in a tight
  loop with no yield/sleep, consuming 100% CPU; add a small
  `tokio::time::sleep` or use `promise.block_and_take()` with
  channel-based progress
  Checked against the code 2026-10-12: Done — the CLI loop sleeps 50
  ms when idle (src/cli/mod.rs).
  Former TODO.md task 1.7.
  Kind: refactor.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0008] **Update dependencies to latest compatible versions.**
  Checked against the code 2026-10-12: Not done — cannot be decided
  from the code; needs a version check.
  Former TODO.md task 1.8.
  **Layman:** Bring the app's building blocks up to their newest versions.
  Kind: refactor.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0009] **Add `clippy` linting to CI pipeline alongside `rustfmt`.**
  Checked against the code 2026-10-12: Done —
  .github/workflows/clippy.yml runs clippy with -D warnings.
  Former TODO.md task 1.9.
  Kind: refactor.
  Source: todo-md-import-2026-10-12.

- 💭 [RPSN-0010] **Consider migrating from `poll-promise` to native `tokio` channels + `eframe`'s `request_repaint_after()` for a cleaner async-UI bridge.**
  Checked against the code 2026-10-12: Undecided — poll-promise is
  still used; no request_repaint_after.
  Former TODO.md task 1.10.
  **Layman:** Maybe change how the window waits for downloads, to make the code simpler.
  Kind: refactor.
  Source: todo-md-import-2026-10-12.

## 2. Improved Download Directory Structure [P0]

Restructure download paths to match the user's existing collection layout and improve organization.

- ✅ [RPSN-0011] **Change `create_new_pkg_path()` in `utils.rs` to produce `{download_path}/{sanitized_title}/{serial}/{pkg_file}` instead of `{download_path}/{serial} - {title}/{pkg_file}`.**
  Checked against the code 2026-10-12: Done —
  DirectoryFormat::TitleSerial in src/utils.rs builds
  base/title/serial; the default is still SerialTitle.
  Former TODO.md task 2.1.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0012] **Add migration logic to reorganize existing downloads into the new `{title}/{serial}/` structure.**
  Add migration logic to detect and optionally reorganize existing
  downloads from old format (`{serial} - {title}/`) and current format
  (`{title}/{pkg_file}` flat) into the new `{title}/{serial}/`
  structure
  Checked against the code 2026-10-12: Done — migrate_existing_dir,
  detect_migratable_directories and migrate_directories in
  src/utils.rs.
  Former TODO.md task 2.2.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0013] **Handle the region suffix in title names.**
  Sony's API returns bare titles (e.g., "God of War III") but existing
  folders include region (e.g., "God of War III (USA)"); provide a
  config option or auto-detect region from serial prefix patterns
  Checked against the code 2026-10-12: Not done — no handling of the
  region suffix, and no region detection from the serial.
  Former TODO.md task 2.3.
  **Layman:** Match folder names that include the region, such as "(USA)", to the bare names Sony sends.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0014] **Add a GUI setting for download directory structure format (let user choose between current and new layout).**
  Checked against the code 2026-10-12: Done — directory-format setting
  in the settings window (src/egui/mod.rs).
  Former TODO.md task 2.4.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0015] **Remove the legacy `create_old_pkg_path()` migration code in `utils.rs` (or update it to handle all three historical formats).**
  Checked against the code 2026-10-12: Done — create_old_pkg_path is
  gone; migration handles all three formats.
  Former TODO.md task 2.5.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

## 3. Batch Download [P1]

Support downloading updates for multiple games in one operation without manual serial entry.

- ✅ [RPSN-0016] **Add support for importing a text file containing serials (one per line, with optional comments via `#`).**
    ```
    # My PS3 games
    BCUS98111   # God of War III
    BCUS98232   # God of War Ascension
    BLUS31147   # Batman Arkham Origins
    ```
  Checked against the code 2026-10-12: Done — src/batch.rs
  parse_batch_file handles # comments and duplicates.
  Former TODO.md task 3.1.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0017] **Add a "Batch Import" button to the GUI with a file picker (`.txt` filter).**
  Checked against the code 2026-10-12: Done — "Import Batch..." button
  with a .txt filter (src/egui/mod.rs).
  Former TODO.md task 3.2.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0018] **Add CLI flag `--batch-file <path>` to load serials from a file.**
  Checked against the code 2026-10-12: Done — --batch-file / -b in
  src/main.rs, used in src/cli/mod.rs.
  Former TODO.md task 3.3.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0019] **Add "Import from RPCS3" button that reads `games.yml` to auto-populate the serial list (see Section 7 - RPCS3 Integration).**
  Checked against the code 2026-10-12: Done — "Scan RPCS3 Library"
  button calls rpcs3::scan_library, which reads games.yml.
  Former TODO.md task 3.4.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0020] **Implement a download queue manager that processes batch items concurrently with a configurable parallelism limit (default: 2-3 simultaneous downloads).**
  Checked against the code 2026-10-12: Done — queue limited by the
  max_concurrent_downloads setting (GUI only).
  Former TODO.md task 3.5.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0021] **Add batch progress overview in GUI - show overall progress (X of Y titles done), not just individual package progress.**
  Checked against the code 2026-10-12: Done — status bar shows batch
  progress and download totals (src/egui/mod.rs).
  Former TODO.md task 3.6.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0022] **CLI silent mode (`-s`) should work seamlessly with batch files for fully automated downloading.**
  Checked against the code 2026-10-12: Done — batch file and silent
  mode work together in src/cli/mod.rs.
  Former TODO.md task 3.7.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0023] **Add "Download All Missing Updates" - scan existing download folder, compare against queried update info, download only what's missing or incomplete.**
  Checked against the code 2026-10-12: Partly done — "Download All"
  skips complete files by hash; there is no scan of the download
  folder and no missing-only mode.
  Former TODO.md task 3.8.
  **Layman:** A button that downloads only the updates missing from your download folder.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

## 4. Download Resume [P1]

Support resuming partially downloaded files instead of re-downloading from scratch.

- ✅ [RPSN-0024] **Use HTTP `Range` header to request only the remaining bytes when a partial file exists.**
    - Check if Sony's CDN supports `Accept-Ranges: bytes` (test with a HEAD request)
    - If supported: send `Range: bytes={existing_size}-` header
    - If not supported: fall back to full re-download (current behavior)
  Checked against the code 2026-10-12: Done — Range plus If-Range
  resume with 206 handling in src/psn/mod.rs.
  Former TODO.md task 4.1.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0025] **Before resuming, verify the existing partial data's integrity.**
    - Option A: Hash the partial content and compare against a stored partial hash (would need to save progress metadata)
    - Option B: Simply check file size against expected size and resume if smaller
    - Option C: Re-hash the complete file after resume completes (current approach, lowest risk)
  Checked against the code 2026-10-12: Done — resumes when the file is
  smaller than expected, then re-hashes the whole file (options B and
  C).
  Former TODO.md task 4.2.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0026] **Store download progress metadata in a `.rusty-psn-progress.json` sidecar file per download directory.**
    ```json
    {
      "url": "https://...",
      "expected_size": 654462304,
      "expected_sha1": "abc123...",
      "downloaded_bytes": 234567890,
      "last_modified": "2026-03-23T10:30:00Z"
    }
    ```
  Checked against the code 2026-10-12: Not done — no progress sidecar
  file; ETag and If-Range are used instead.
  Former TODO.md task 4.3.
  **Layman:** Keep a small notes file so an interrupted download remembers where it stopped.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0027] **Change `create_pkg_file()` to use `OpenOptions::append()` when resuming instead of truncating.**
  Checked against the code 2026-10-12: Done — create_pkg_file opens
  without truncating and seeks to the offset.
  Former TODO.md task 4.4.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0028] **Update the download progress reporting to show "Resuming from X MB" in both GUI and CLI.**
  Checked against the code 2026-10-12: Done — DownloadStatus::Resuming
  is shown in the CLI and the GUI.
  Former TODO.md task 4.5.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0029] **Add a "Retry failed downloads" button in the GUI that attempts to resume all items in the `failed_downloads` list.**
  Checked against the code 2026-10-12: Partly done — a per-item Retry
  button exists; there is no retry-all-failed button.
  Former TODO.md task 4.6.
  **Layman:** One button to retry every download that failed.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0030] **Handle Sony's server instability gracefully - the app already detects short transfers (`received_data < self.size`); instead of just reporting failure, auto-queue for resume.**
  Checked against the code 2026-10-12: Done — auto-retry on a short
  transfer in the GUI (MAX_AUTO_RETRIES) and the CLI.
  Former TODO.md task 4.7.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

## 5. Security Hardening [P1]

Ensure the application has no vulnerabilities and follows security best practices.

- 📋 [RPSN-0031] **Audit `danger_accept_invalid_certs(true)`.**
  This is currently required for Sony's servers but is applied
  globally. Scope it to only Sony PSN domains:
    - Create a custom `reqwest::Client` per target domain
    - Only disable cert validation for `*.playstation.net` and `*.dl.playstation.net`
    - Use normal TLS validation for any other network requests (future features)
  Checked against the code 2026-10-12: Partly done — one client only;
  build_psn_client still disables certificate checks for every
  request, though the redirect policy limits hosts to Sony.
  Former TODO.md task 5.1.
  **Layman:** Only skip the security certificate check when talking to Sony's servers.
  Kind: security.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0032] **Path traversal protection.**
  The `sanitize_title()` function replaces OS-invalid chars, but does
  not prevent directory traversal. Add checks:
    - Reject or strip `..` path components in titles and filenames
    - Reject absolute paths in package URLs that could write outside the download directory
    - Validate that resolved download paths are children of the configured download directory
  Checked against the code 2026-10-12: Done — sanitize_title strips
  ".." and security::validate_path_containment guards create_pkg_file.
  Former TODO.md task 5.2.
  Kind: security.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0033] **URL validation.**
  Before making HTTP requests, validate that package URLs point to
  expected Sony domains:
    - Whitelist: `*.playstation.net`, `*.dl.playstation.net`
    - Reject URLs pointing to other domains (could be injected via malformed XML)
  Checked against the code 2026-10-12: Done —
  security::validate_download_url checks the allow-list before each
  request.
  Former TODO.md task 5.3.
  Kind: security.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0034] **Input validation.**
  Harden serial input parsing:
    - Enforce maximum length (9 characters after normalization)
    - Allow only alphanumeric characters after normalization
    - Reject serials that don't match known prefix patterns
  Checked against the code 2026-10-12: Partly done — length 9 and
  alphanumeric are enforced (src/security.rs); an unknown prefix only
  warns.
  Former TODO.md task 5.4.
  **Layman:** Turn away game IDs that don't look like real PlayStation IDs.
  Kind: security.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0035] **XML parsing safety.**
  Review `quick-xml` usage for:
    - XML entity expansion attacks (billion laughs / XML bomb) - ensure entity expansion is disabled or limited
    - Extremely large responses - add a maximum response size limit to `reqwest` calls
  Checked against the code 2026-10-12: Done — MAX_METADATA_SIZE limits
  XML and manifest responses; quick-xml does not expand DTD entities.
  Former TODO.md task 5.5.
  Kind: security.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0036] **Dependency audit.**
  Add `cargo audit` to CI pipeline to check for known vulnerabilities
  in dependencies
  Checked against the code 2026-10-12: Not done — no cargo audit step
  in .github/workflows.
  Former TODO.md task 5.6.
  **Layman:** Check automatically for known security problems in the app's building blocks.
  Kind: security.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0037] **File permissions.**
  On Unix, set downloaded `.pkg` files to `0644` (read-only for
  group/other) rather than inheriting umask
  Checked against the code 2026-10-12: Not done — no permission
  setting on downloaded files.
  Former TODO.md task 5.7.
  **Layman:** Save downloaded files so other users on the computer can read but not change them.
  Kind: security.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0038] **Secrets in binary.**
  The PS4 HMAC key is hardcoded. While it's publicly known, document
  why it's acceptable and add a comment explaining its provenance
  Checked against the code 2026-10-12: Done — provenance comment on
  the PS4 key in src/psn/utils.rs.
  Former TODO.md task 5.8.
  Kind: security.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0039] **Memory safety.**
  Review all `unwrap()` calls and replace with proper error handling
  where a panic would be unacceptable (especially in async contexts
  where a panic kills the runtime)
  Checked against the code 2026-10-12: Partly done — many unwrap()
  calls are annotated or gone; about twenty remain in src/cli/mod.rs,
  plus some in egui, firmware and main. One seen 2026-10-12 is
  `self.settings.rpcs3_executable.clone().unwrap()` in the RPCS3
  install buttons in src/egui/mod.rs, with no justifying comment.
  Former TODO.md task 5.9.
  **Layman:** Replace spots where the app could crash suddenly with proper error messages.
  Kind: security.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0040] **TOCTOU in file operations.**
  The check-then-create pattern in `create_pkg_file()` has a
  time-of-check-to-time-of-use gap. Use atomic file operations where
  possible
  Checked against the code 2026-10-12: Not done — no atomic or
  create_new file operations; check-then-create remains.
  Former TODO.md task 5.10.
  **Layman:** Close a small timing gap when the app creates download files.
  Kind: security.
  Source: todo-md-import-2026-10-12.

## 6. PSVita Support [P2]

Add PlayStation Vita game update downloads. Research status: **confirmed feasible**.

- ✅ [RPSN-0041] **Add `PlatformVariant::PSVita` to the platform enum in `psn/utils.rs`.**
  Checked against the code 2026-10-12: Done — PlatformVariant::PSVita
  in src/psn/utils.rs.
  Former TODO.md task 6.1.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0042] **Update `get_platform_variant()` to detect Vita serial prefixes (`PCSA`-`PCSH`, `VC`, `VL`).**
  Original wording: Update `get_platform_variant()` to detect Vita
  serial prefixes (`PCSA`-`PCSH`, `VC*`, `VL*`)
  Checked against the code 2026-10-12: Done — PCS, VC and VL prefixes
  detected in src/psn/utils.rs.
  Former TODO.md task 6.2.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0043] **Update `get_update_info_url()` with a new PSVita branch.**
    - Use HMAC key `E5E278AA1EE34082A088279C83F9BBC806821C52F2AB5D2B4ABD995450355114`
    - Use base path `/pl/np/` (not `/plo/np/` like PS4)
    - Same HMAC computation as PS4: `HMAC-SHA256("np_{title_id}")`
  Checked against the code 2026-10-12: Done — PSVita branch with the
  Vita key and /pl/np/ in src/psn/utils.rs.
  Former TODO.md task 6.3.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0044] **Test with known Vita serials: PCSA00007 (Hustle Kings), PCSE00120 (Persona 4 Golden), PCSA00126 (Killzone Mercenary).**
  Checked against the code 2026-10-12: Partly done — tests exist for
  PCSA00007 and PCSA00107 only; none for PCSE00120 or PCSA00126.
  Former TODO.md task 6.4.
  **Layman:** Test Vita support with a few more real games.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0045] **Test whether Vita `.pkg` files use SHA1 suffix verification (like PS3) or whole-file hashing (like PS4).**
  Checked against the code 2026-10-12: Not done — Vita uses the
  PS3-style suffix hash (hash_whole_file=false); nothing shows it was
  tested.
  Former TODO.md task 6.5.
  **Layman:** Confirm Vita downloads are checked for damage the right way.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0046] **Update the GUI and CLI to display `[PSVita]` platform tag.**
  Checked against the code 2026-10-12: Done — the CLI prints the
  platform tag; the GUI shows a PSVita badge colour.
  Former TODO.md task 6.6.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0047] **Add Vita-specific test cases to `psn/mod.rs`.**
  Checked against the code 2026-10-12: Done — parse_vita_hustle_kings
  and parse_vita_killzone tests in src/psn/mod.rs.
  Former TODO.md task 6.7.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0048] **Optionally parse `psp2_system_ver` to display minimum firmware requirement in the UI.**
  Checked against the code 2026-10-12: Not done — psp2_system_ver is
  not parsed.
  Former TODO.md task 6.8.
  **Layman:** Show the minimum Vita system version an update needs.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

## 7. RPCS3 Integration [P1]

Integrate directly with the RPCS3 emulator to auto-detect games and install updates.

- ✅ [RPSN-0049] **Auto-detect RPCS3 installation.**
  Search common paths:
    - Linux AppImage: `~/.config/rpcs3/` or `*.AppImage.home/.config/rpcs3/`
    - Linux native: `~/.config/rpcs3/`
    - Windows: `%APPDATA%/rpcs3/` or alongside `rpcs3.exe`
    - macOS: `~/Library/Application Support/rpcs3/`
    - Allow manual path override in settings
  Checked against the code 2026-10-12: Done — src/rpcs3/auto_detect.rs
  searches Linux, Flatpak, AppImage, Windows and macOS paths; the
  settings allow an override.
  Former TODO.md task 7.1.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0050] **Parse `games.yml`.**
  Read the serial-to-path mapping to build a list of all installed
  games with their serials
  Checked against the code 2026-10-12: Done — parse_games_yml in
  src/rpcs3/config.rs.
  Former TODO.md task 7.2.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0051] **Parse `PARAM.SFO`.**
  Read binary SFO files from `dev_hdd0/game/{serial}/PARAM.SFO` to
  extract:
    - `TITLE` - game name
    - `APP_VER` - currently installed update version
    - `TITLE_ID` - serial (for cross-referencing)
  Checked against the code 2026-10-12: Done — src/rpcs3/sfo.rs reads
  TITLE, APP_VER and TITLE_ID.
  Former TODO.md task 7.3.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0052] **"Scan RPCS3 Library" button.**
  In the GUI, add a button that:
    1. Reads `games.yml` to get all serials
    2. Queries Sony's API for updates for each serial (with progress indicator)
    3. Compares available update versions against installed versions (from `PARAM.SFO`)
    4. Shows which games have updates available vs already up-to-date
  Checked against the code 2026-10-12: Done — scan_library plus a
  library grid comparing versions (src/egui/mod.rs).
  Former TODO.md task 7.4.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0053] **Install updates directly to RPCS3.**
  After downloading a `.pkg` file:
    - Option A: Copy/move the `.pkg` to a staging area and invoke RPCS3's package installer (RPCS3 can install `.pkg` files via CLI: `rpcs3 --installfw` / `rpcs3 --installpkg`)
    - Option B: Parse and extract the `.pkg` contents directly into `dev_hdd0/game/{serial}/` (more complex but doesn't require RPCS3 running)
    - Prefer Option A for reliability; fall back to manual copy
  Checked against the code 2026-10-12: Done — rpcs3::install_pkg runs
  --installpkg; "Install to RPCS3" button in src/egui/mod.rs.
  Former TODO.md task 7.5.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0054] **Add RPCS3 path to settings.**
  New setting `rpcs3_path` in `AppSettings` with folder picker and
  auto-detect button
  Checked against the code 2026-10-12: Done — rpcs3_config_dir and
  rpcs3_executable settings with folder picker and auto-detect.
  Former TODO.md task 7.6.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0055] **Add RPCS3 integration to CLI.**
  New flag `--rpcs3-path <path>` and `--scan-rpcs3` for automated
  scanning
  Checked against the code 2026-10-12: Done — --rpcs3-config-dir,
  --scan-rpcs3, --install and --rpcs3-executable in src/main.rs.
  Former TODO.md task 7.7.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0056] **Parse `vfs.yml`.**
  Resolve `$(EmulatorDir)` to find the actual `dev_hdd0` location (it
  might be remapped)
  Checked against the code 2026-10-12: Done — parse_vfs_yml in
  src/rpcs3/config.rs resolves $(EmulatorDir).
  Former TODO.md task 7.8.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0057] **Show installed vs available version.**
  In the results list, display the currently installed version
  alongside the latest available version (e.g., "Installed: 1.03 |
  Available: 1.10")
  Checked against the code 2026-10-12: Done — installed and available
  version columns in src/egui/mod.rs.
  Former TODO.md task 7.9.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0058] **Batch update all outdated games.**
  "Update All" button that downloads and installs updates for every
  game that has a newer version available
  Checked against the code 2026-10-12: Partly done — "Update All
  Outdated" downloads only; nothing installs automatically.
  Former TODO.md task 7.10.
  **Layman:** Make "Update All Outdated" install the updates into RPCS3 as well as downloading them.
  Kind: feature.
  Source: todo-md-import-2026-10-12.

## 8. Add Themes [P2]

Add visual theming support to the egui GUI.

- 📋 [RPSN-0059] **Add a theme selector to the settings window with at least.**
    - **Dark** (egui default `Visuals::dark()`)
    - **Light** (`Visuals::light()`)
    - **RPCS3 Blue** (custom theme matching RPCS3's UI colors)
    - **PlayStation** (dark theme with PS brand blue `#006FCD` accents)
  Checked against the code 2026-10-12: Partly done — Dark, Light and
  PlayStation themes exist; RPCS3 Blue does not.
  Former TODO.md task 8.1.
  **Layman:** Add an RPCS3-style blue colour theme.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0060] **Persist selected theme in `AppSettings` (already has eframe persistence).**
  Checked against the code 2026-10-12: Done — the theme is persisted
  in the settings.
  Former TODO.md task 8.2.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0061] **Apply theme on startup in `UpdatesApp::new()` via `cc.egui_ctx.set_visuals()`.**
  Checked against the code 2026-10-12: Done — settings.theme.apply()
  runs at startup.
  Former TODO.md task 8.3.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0062] **Add custom accent colors for status indicators.**
    - Success: green (keep current)
    - Error: red (keep current)
    - In-progress: accent color from theme (currently hardcoded yellow `1.0, 1.0, 0.6`)
    - Neutral: theme default
  Checked against the code 2026-10-12: Partly done — the
  STATUS_COLOR_* constants are fixed, not taken from the theme.
  Former TODO.md task 8.4.
  **Layman:** Make the status colours follow the chosen theme.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0063] **Allow theme hot-switching without restart (apply immediately on selection).**
  Checked against the code 2026-10-12: Done — the theme applies as
  soon as it is selected.
  Former TODO.md task 8.5.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- 💭 [RPSN-0064] **Consider loading custom themes from a TOML/JSON file for user-created themes.**
  Checked against the code 2026-10-12: Undecided — no custom theme
  file loading.
  Former TODO.md task 8.6.
  **Layman:** Maybe let people load their own colour themes from a file.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0065] **Ensure all hardcoded `egui::Rgba::from_rgb(...)` and `egui::Color32` values in `egui/mod.rs` use theme-aware colors instead of fixed values.**
  Checked against the code 2026-10-12: Not done — fixed Rgba::from_rgb
  and Color32 values remain in src/egui/mod.rs.
  Former TODO.md task 8.7.
  **Layman:** Make every colour in the window follow the chosen theme.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

## 9. Additional Enhancements [P3]

Lower priority items that would improve the overall experience.

- ✅ [RPSN-0066] **Download speed display.**
  Show current download speed (KB/s, MB/s) alongside progress
  Checked against the code 2026-10-12: Done — download speed shown in
  src/egui/mod.rs.
  Former TODO.md task 9.1.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0067] **ETA calculation.**
  Estimate time remaining based on rolling average download speed
  Checked against the code 2026-10-12: Done — ETA shown in
  src/egui/mod.rs.
  Former TODO.md task 9.2.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0068] **Download history.**
  Persist a log of completed downloads with timestamps, shown in a
  "History" tab
  Checked against the code 2026-10-12: Partly done — history lasts one
  session only and shows in a window, not a tab.
  Former TODO.md task 9.3.
  **Layman:** Keep a history of finished downloads between sessions, in its own tab.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0069] **Concurrent serial search.**
  The GUI currently allows only one search at a time (`search_promise`
  is a single `Option`); allow concurrent searches
  Checked against the code 2026-10-12: Partly done — batch searches
  run concurrently; the single search_promise remains.
  Former TODO.md task 9.4.
  **Layman:** Allow more than one game search at the same time.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0070] **Package URL copy.**
  Add a "Copy URL" button for each package to allow downloading via
  external tools
  Checked against the code 2026-10-12: Done — "Copy URL" button in
  src/egui/mod.rs.
  Former TODO.md task 9.5.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0071] **Export update list.**
  Export search results as JSON/CSV for documentation or sharing
  Checked against the code 2026-10-12: Not done — no JSON or CSV
  export.
  Former TODO.md task 9.6.
  **Layman:** Save search results to a spreadsheet-style or data file.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0072] **Localization.**
  The GUI embeds Japanese fonts already; add basic i18n framework for
  UI strings
  Checked against the code 2026-10-12: Not done — no translation
  framework.
  Former TODO.md task 9.7.
  **Layman:** Prepare the app for translation into other languages.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0073] **System tray.**
  Minimize to system tray during long downloads (Linux: `tray-icon`
  crate)
  Checked against the code 2026-10-12: Not done — no system tray.
  Former TODO.md task 9.8.
  **Layman:** Let the app shrink to the system tray during long downloads.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0074] **Flatpak/Snap packaging.**
  For easier Linux distribution alongside the current
  AppImage-targeted binary
  Checked against the code 2026-10-12: Not done — no Flatpak or Snap
  manifest or CI step.
  Former TODO.md task 9.9.
  **Layman:** Offer Flatpak or Snap packages for easier Linux installs.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0075] **PSP support.**
  PSP uses similar NP serial prefixes (`NPUG`, `NPEH`, etc.) and the
  same update URL format; could be added alongside PSVita with minimal
  effort
  Checked against the code 2026-10-12: Not done — no PSP support.
  Former TODO.md task 9.10.
  **Layman:** Add PSP game updates.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

## 10. Custom Application Icon [P2]

Create a unique, recognizable icon for rusty-psn that displays in the taskbar/task manager and on the compiled binary.

- ✅ [RPSN-0076] **Design a custom icon representing rusty-psn's purpose (PlayStation update downloader + Rust).**
    - SVG source for scalability
    - Must look good at small sizes (16x16, 32x32) and large sizes (256x256, 512x512)
    - Color scheme: PlayStation blue (#006FCD) with Rust orange (#CE422B) accent, or a unique combination
  Checked against the code 2026-10-12: Done — resources/icon.svg plus
  48, 64, 128 and 256 px PNGs.
  Former TODO.md task 10.1.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0077] **Generate platform-specific icon formats.**
    - Windows: `.ico` file (multi-resolution: 16, 32, 48, 64, 128, 256)
    - Linux: `.png` files at standard sizes (48, 64, 128, 256, 512) + `.desktop` file
    - macOS: `.icns` file (already has `Info.plist` for app bundle)
  Checked against the code 2026-10-12: Partly done — PNGs and a
  .desktop file exist; no .ico, no .icns and no 512 px PNG.
  Former TODO.md task 10.2.
  **Layman:** Make icon files for Windows and macOS, and a larger Linux icon.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0078] **Embed the icon in the Windows binary using `winres` or `embed-resource` crate in `build.rs`.**
  Checked against the code 2026-10-12: Not done — no build.rs; neither
  winres nor embed-resource is a dependency.
  Former TODO.md task 10.3.
  **Layman:** Build the icon into the Windows program file.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0079] **Set the window icon at runtime via `eframe::NativeOptions::viewport` with `ViewportBuilder::with_icon()`.**
  Checked against the code 2026-10-12: Done —
  ViewportBuilder.with_icon() in src/main.rs.
  Former TODO.md task 10.4.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0080] **Update the macOS `.app` bundle to include the icon in `Info.plist` (`CFBundleIconFile`).**
  Checked against the code 2026-10-12: Not done — resources/Info.plist
  has no CFBundleIconFile.
  Former TODO.md task 10.5.
  **Layman:** Show the icon on macOS.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0081] **Add icon to the Linux `.desktop` file for proper desktop integration.**
  Checked against the code 2026-10-12: Done —
  resources/rusty-psn.desktop sets Icon=rusty-psn.
  Former TODO.md task 10.6.
  Kind: enhancement.
  Source: todo-md-import-2026-10-12.

## 11. Found during adoption

Gaps noticed while aligning the project to the global workflow on 2026-10-12.

- 📋 [RPSN-0082] **Remove personal filesystem paths from tests and comments.**
  Project rule 6 bars machine-specific paths from source. Seen
  2026-10-12: two test inputs in src/rpcs3/mod.rs and a comment in
  src/rpcs3/auto_detect.rs use paths from the owner's own drives.
  Replace them with neutral example paths.
  **Layman:** Take the owner's own folder names out of the code.
  Kind: fix.
  Source: in-session-2026-10-12.
