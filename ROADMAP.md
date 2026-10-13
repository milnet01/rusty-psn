<!-- ants-roadmap-format: 1 -->
<!-- Generated from the Ants Terminal roadmap store. Edit it with roadmap_log; hand edits are discarded by the next write. -->
<!-- ants-roadmap-id-high-water: RPSN=134 -->
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

- ✅ [RPSN-0008] **Update dependencies to latest compatible versions.**
  Checked against the code 2026-10-12: Not done — cannot be decided
  from the code; needs a version check.
  Former TODO.md task 1.8.
  Resolved (2026-10-12): done by RPSN-0125, every crate and workflow
  action at its latest release (c7ab91e..8dbf8a5).
  **Layman:** Bring the app's building blocks up to their newest versions.
  Kind: refactor.
  Source: todo-md-import-2026-10-12.
  Duplicate-of: RPSN-0125.

- ✅ [RPSN-0009] **Add `clippy` linting to CI pipeline alongside `rustfmt`.**
  Checked against the code 2026-10-12: Done —
  .github/workflows/clippy.yml runs clippy with -D warnings.
  Former TODO.md task 1.9.
  Kind: refactor.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0010] **Consider migrating from `poll-promise` to native `tokio` channels + `eframe`'s `request_repaint_after()` for a cleaner async-UI bridge.**
  Checked against the code 2026-10-12: Undecided — poll-promise is
  still used; no request_repaint_after.
  Former TODO.md task 1.10.
  Decided by the owner (2026-10-13): replace poll-promise with a small
  local helper built on tokio that offers the three calls the code uses
  (spawn_async, ready, block_and_take). No request_repaint_after: each
  task already calls request_repaint. poll-promise 0.3.0 has had no
  push since 2023-09-29.
  Resolved (2026-10-13): src/promise.rs replaces poll-promise with the
  same three calls, so call sites changed only their import. tokio now
  declares rt-multi-thread itself: poll-promise had been enabling it, and
  Runtime::new needs it. Two new tests; the waiting test went red when
  block_and_take was broken to not wait.
  **Layman:** Maybe change how the window waits for downloads, to make the code simpler.
  Kind: refactor.
  Source: todo-md-import-2026-10-12.

- 📋 [RPSN-0134] **Clear the event buffer in each pass of the update XML parser.**
  src/psn/parser.rs parse_response never calls event_buf.clear(), so the
  buffer holds every event of the reply until the parse ends. The
  firmware parser in src/psn/firmware.rs clears it at the end of each
  pass. Clear it the same way; the `continue` in the "Code" arm skips the
  end of the loop, so the clear must run before the match or on that path
  too.
  **Layman:** Use less memory while reading Sony's update list.
  Kind: optimize.
  Source: in-session-2026-10-13 RPSN-0104 review.
  Lanes: PSN.

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

- ✅ [RPSN-0031] **Audit `danger_accept_invalid_certs(true)`.**
  This is currently required for Sony's servers but is applied
  globally. Scope it to only Sony PSN domains:
    - Create a custom `reqwest::Client` per target domain
    - Only disable cert validation for `*.playstation.net` and `*.dl.playstation.net`
    - Use normal TLS validation for any other network requests (future features)
  Checked against the code 2026-10-12: Partly done — one client only;
  build_psn_client still disables certificate checks for every
  request, though the redirect policy limits hosts to Sony.
  Former TODO.md task 5.1.
  Resolved (2026-10-12): build_psn_client uses a rustls config whose
  verifier skips the certificate check only for hosts under
  security::ALLOWED_DOMAINS; every other host gets the system verifier.
  Regression test psn_client_checks_certificates_outside_sony (live,
  self-signed.badssl.com).
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

- 🚫 [RPSN-0034] **Input validation.**
  Harden serial input parsing:
    - Enforce maximum length (9 characters after normalization)
    - Allow only alphanumeric characters after normalization
    - Reject serials that don't match known prefix patterns
  Checked against the code 2026-10-12: Partly done — length 9 and
  alphanumeric are enforced (src/security.rs); an unknown prefix only
  warns.
  Former TODO.md task 5.4.
  Dropped (2026-10-12): the length and alphanumeric checks were
  already done (security::validate_serial), and they close any
  injection route. Rejecting unknown prefixes was left out on purpose:
  VALID_PREFIXES is incomplete (no BLJM, which as far as I know is a
  Japanese PS3 disc prefix; not confirmed against Sony), so a hard
  reject would block real games for no security gain. An unknown
  prefix still logs a warning. Reopen if the owner wants the reject
  with a researched, complete prefix list.
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

- ✅ [RPSN-0036] **Dependency audit.**
  Add `cargo audit` to CI pipeline to check for known vulnerabilities
  in dependencies
  Checked against the code 2026-10-12: Not done — no cargo audit step
  in .github/workflows.
  Former TODO.md task 5.6.
  Resolved (2026-10-12): .github/workflows/audit.yml runs cargo audit on
  push, pull request and weekly; scripts/local-ci.sh runs the same
  pinned version (a243c4b). The workflow has not run yet: GitHub only.
  **Layman:** Check automatically for known security problems in the app's building blocks.
  Kind: security.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0037] **File permissions.**
  On Unix, set downloaded `.pkg` files to `0644` (read-only for
  group/other) rather than inheriting umask
  Checked against the code 2026-10-12: Not done — no permission
  setting on downloaded files.
  Former TODO.md task 5.7.
  Resolved (2026-10-12): utils::download_file_options creates every
  download with mode 0644 on Unix (3915020). Test:
  create_pkg_file_is_not_writable_by_group_or_others.
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

- ✅ [RPSN-0039] **Memory safety.**
  Review all `unwrap()` calls and replace with proper error handling
  where a panic would be unacceptable (especially in async contexts
  where a panic kills the runtime)
  Checked against the code 2026-10-12: Partly done — many unwrap()
  calls are annotated or gone; about twenty remain in src/cli/mod.rs,
  plus some in egui, firmware and main. One seen 2026-10-12 is
  `self.settings.rpcs3_executable.clone().unwrap()` in the RPCS3
  install buttons in src/egui/mod.rs, with no justifying comment.
  Former TODO.md task 5.9.
  Resolved (2026-10-12, 082030f): no bare unwrap left outside tests
  except two part_number ones justified by the check above them. The
  PSN client is built before the window opens (error dialog on
  failure); locks recover from poisoning; command-line writes no longer
  panic; end of input at a prompt now skips instead of downloading
  everything.
  **Layman:** Replace spots where the app could crash suddenly with proper error messages.
  Kind: security.
  Source: todo-md-import-2026-10-12.

- ✅ [RPSN-0040] **TOCTOU in file operations.**
  The check-then-create pattern in `create_pkg_file()` has a
  time-of-check-to-time-of-use gap. Use atomic file operations where
  possible
  Checked against the code 2026-10-12: Not done — no atomic or
  create_new file operations; check-then-create remains.
  Former TODO.md task 5.10.
  Resolved (2026-10-12): download_file_options opens with O_NOFOLLOW on
  Unix, so a link swapped in after the containment check is refused
  (3915020). A swapped folder higher up the path is not covered. Test:
  download_file_options_refuse_a_link.
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

- ✅ [RPSN-0127] **Deliver the material for the Rusty PSN page on antsprojectshub.co.za.**
  Requested 2026-10-12 by the ants-projects-hub-website session, on the
  user's behalf. Deliver into ~/.local/share/claude-handoff/rusty-psn-site-page/,
  then message that session (ants-projects-hub-website). Never edit the
  website repo: every push there publishes the live site. Current page
  text, read-only: src/about/rusty-psn.md in the website repo
  1. Credit: which parts are RainbowCookie32's (upstream author) and which
     are ours; the downloading itself is theirs. One or two sentences.
  2. Licence: confirm the upstream licence file and copyright notice are
     kept; name the files.
  3. Logo: ORIGINAL SVG, square, readable at 64 px, works on a dark
     background, plus a matching app icon if the app ships one (see
     section 10). No upstream branding without permission.
  4. About review: corrections to the current text, plain language, no
     version numbers.
  5. Facts: repo is "private for now" (no fork yet, RPSN-0096); platforms
     that really build; what must happen before a first release.
  6. Screenshots: 2-4 PNGs of the window (an update list, a download in
     progress), one-line caption each, made-up or public serial only.
     Taking them also gives the eframe 0.36 build its first hand test
     (RPSN-0125's open item): open the Settings folder dialog too.
  Trailer footage may be asked for after a release; not now.
  Resolved (2026-10-12): delivered to
  ~/.local/share/claude-handoff/rusty-psn-site-page/ (README.md, logo SVG
  and PNG, four screenshots); messaged ants-projects-hub-website. Hand
  test on a private display: the eframe 0.36 window opens, the RPCS3 scan
  reaches Sony and lists updates, a download runs, Settings draws. Not
  testable there: typed text never reached the serial field (same on the
  pre-upgrade build, and demoreel types fine into xterm, so not a
  regression), and rfd's folder dialog goes to the real desktop through
  the portal. Both left for the owner to try by hand.
  Follow-up (2026-10-12): the website session asked for a wordmark (not
  a square logo) and a better shot 3; both delivered and published by
  that session. It asks to be messaged when there is a public repo and a
  first release, so the page can switch to downloads.
  **Layman:** Give the website what it needs for the Rusty PSN page: credits, licence, a logo, corrected text, facts and screenshots.
  Kind: marketing.
  Source: ants-projects-hub-website session request 2026-10-12 (user asked it).
  Lanes: Docs.

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

- ✅ [RPSN-0082] **Remove personal filesystem paths from tests and comments.**
  Project rule 6 bars machine-specific paths from source. Seen
  2026-10-12: two test inputs in src/rpcs3/mod.rs and a comment in
  src/rpcs3/auto_detect.rs use paths from the owner's own drives.
  Replace them with neutral example paths.
  Resolved 2026-10-12: the two title_from_path tests and the auto_detect
  comment use neutral example paths. A search of every tracked file
  finds none of the owner's drive paths. The ~/Games search location in
  auto_detect is a generic default and stays.
  **Layman:** Take the owner's own folder names out of the code.
  Kind: fix.
  Source: in-session-2026-10-12.

- 📋 [RPSN-0096] **Owner to decide: a GitHub fork, and a versioning-overrides note.**
  Deferred by the owner on 2026-10-12. `origin` points at the upstream
  author's repository, so the work on branch `local-dev` exists only on
  this disk; a fork of the owner's own would give it a remote. Pushing
  there publishes it, so it needs the owner's yes. align-report also lists
  no `docs/standards/versioning-overrides.md`; recommended to wait until a
  release is planned.
  Progress (2026-10-13): fork done. The owner chose a public fork,
  https://github.com/milnet01/rusty-psn, added here as the remote `fork`;
  `local-dev` was pushed and tracks it, and `origin` stays upstream. The
  owner agreed to publish with their usual commit email. Still open: the
  versioning-overrides note, to wait for a planned release.
  **Layman:** Decide whether to back up your work in your own GitHub copy, and when to note what counts as a breaking change.
  Kind: chore.
  Source: in-session-2026-10-12.

- ✅ [RPSN-0097] **Make the folder-boundary check accept relative and linked download folders.**
  `security::validate_path_containment` compared a new file's lexical path
  with the canonical base. Once a relative base existed (the command line's
  default `pkgs/`), every new file was refused. A linked download folder was
  refused the same way, and a linked folder inside the base let a new file
  escape. Found while working RPSN-0083, whose new checks would have spread
  it. Fix: canonicalize the target's deepest existing ancestor, then append
  the missing part.
  Resolved 2026-10-12 in commit a6eec07, with three regression tests
  shown failing first.
  **Layman:** Downloads into a folder named by a short path, or reached through a shortcut link, no longer fail, and a link can't be used to escape the folder.
  Kind: fix.
  Source: in-session-2026-10-12.
  Lanes: Security.

- ✅ [RPSN-0128] **Point the outer rusty-psn folder at rusty-psn-source once Ants Terminal ships ANTS-5667.**
  The tab opens in the outer rusty-psn folder, one level above the
  repo, so the Roadmap button and MCP verbs find nothing there. Ants
  Terminal's fix (ANTS-5667) adds a key to the OUTER folder's
  .ants/project.json, proposed as { "repo_root": "rusty-psn-source" }.
  Blocked: do NOT write the key until the ants-terminal session messages
  that its name is final (its spec was under review on 2026-10-12). Then
  set it with project_settings, never by hand, from the outer folder.
  Workaround until then: open the tab in rusty-psn-source.
  Resolved 2026-10-12: ants-terminal confirmed the key name (ANTS-5667,
  commit 2a7df693). Set with project_settings from the outer folder:
  .ants/project.json holds "repo_root": "rusty-psn-source" (outside this
  repo, so untracked here). roadmap_query from the outer folder now
  resolves to this ROADMAP.md.
  **Layman:** Let the terminal tab find this project's roadmap from the folder it opens in.
  Kind: chore.
  Source: ants-terminal session message 2026-10-12 (ANTS-5664, ANTS-5667).

## 12. Design follow-ups

Work named by docs/design.md and its review on 2026-10-12.

- ✅ [RPSN-0083] **Check the folder boundary on every write into the download folder.**
  docs/design.md names three writes that skip
  `security::validate_path_containment`: the merged PS4 package written by
  `psn::merge_parts`, and the layout migration in
  `utils::migrate_existing_dir` (run by `utils::create_pkg_file` before its
  own check) and `utils::migrate_directories`. CLAUDE.md rule 1 requires the
  check. Claims S4.
  Resolved 2026-10-12: merge_parts checks the merged and part paths;
  migrate_existing_dir checks its target and the old folder it moves
  from; migrate_directories takes the download folder and skips any move
  with either end outside. Four regression tests, each shown failing
  first.
  **Layman:** Make sure merged PS4 updates and folder moves can never write outside your download folder.
  Kind: security.
  Source: design-review-2026-10-12.
  Lanes: PSN, Files, Security.

- 📋 [RPSN-0084] **Make Title / ID the default download layout.**
  Both apps default to ID - Title (`DirectoryFormat::SerialTitle`, and the
  CLI's `--directory-format` default `serial-title`). Discovery S4 and
  CLAUDE.md rule 6 make Title / ID the default; the other layouts stay
  selectable. Claims S4.
  Progress 2026-10-12 (verify-delivery, CLI run): downloads landed
  inside the chosen folder, laid out ID - Title, confirming this item is
  still open.
  **Layman:** Sort downloads into Game Title / Game ID folders unless you pick another layout.
  Kind: feature.
  Source: design-review-2026-10-12.
  Lanes: Files, Window app, Command line.

- ✅ [RPSN-0085] **Untangle the two-way dependency between Files and PSN.**
  `src/utils.rs` uses PSN's `DownloadError` while PSN uses Files.
  docs/design.md allows this only until this item ships. Move the shared
  error so the dependency runs one way.
  Resolved (2026-10-12, 4b11053): utils::FileError replaces PSN's
  DownloadError in Files; PSN converts it with From. design.md's
  exception removed.
  **Layman:** Tidy two parts of the code that lean on each other, so each can change alone.
  Kind: refactor.
  Source: design-2026-10-12.
  Lanes: PSN, Files.

- ✅ [RPSN-0086] **Split the window app into one file per screen.**
  `src/egui/mod.rs` holds the whole window app. The owner chose a planned
  split on 2026-10-12. Until it ships, a new screen goes in its own file
  under `src/egui/`, per docs/design.md.
  Resolved (2026-10-12, 58ed580): src/egui/ now holds ten files by
  screen or concern; mod.rs keeps shared state, new() and the frame
  loop. Move-only, proved by a removed/added line comparison.
  **Layman:** Break the window app's one huge code file into smaller files, one per screen.
  Kind: refactor.
  Source: design-2026-10-12.
  Lanes: Window app.

- 📋 [RPSN-0087] **Let the command line merge multi-part PS4 updates.**
  `psn::merge_parts`, `MergeStatus` and `copy_pkg_file` compile only with
  the `egui` feature, so a command-line build cannot merge a multi-part PS4
  update. Found by the design review 2026-10-12. Claims S3 and S9.
  **Layman:** Make the command-line version join multi-part PS4 updates, like the window app does.
  Kind: feature.
  Source: design-review-2026-10-12.
  Lanes: PSN, Command line.

- ✅ [RPSN-0088] **Bring STANDARDS.md § 2.3's crate list in line with Cargo.toml.**
  § 2.3 lists `sha1_smol` and `serde_yaml`; Cargo.toml uses `sha1` and
  `serde_yaml_ng`, and adds `zip` and `tempfile`. docs/design.md names
  Cargo.toml the authority. Replace the list with a pointer, or correct it.
  Resolved (2026-10-12, 8c1d26c): the table is replaced by a pointer to
  Cargo.toml and docs/dependency-holds.md.
  **Layman:** Fix an out-of-date list of the app's building blocks in the project notes.
  Kind: doc-fix.
  Source: design-review-2026-10-12.
  Lanes: docs.

- 💭 [RPSN-0133] **Redesign the window app after the approved mockups.**
  Mockups: https://claude.ai/artifact/PWr5yiihCDzG1JCswamS3m
  (four screens: Search, Downloads, RPCS3 library, Settings).
  Approved by the owner on 2026-10-12 ("I like the new look").
  Not yet split into build items, and the owner has not said to
  build it. Recommended first pieces: a text-size setting and a
  high-contrast theme, since the owner is partially sighted.
  Other pieces in the mockups: a sidebar in place of the menu
  bar, status labels that pair an icon with words, a library
  table with filter buttons, and a Settings page with Save and
  Undo. Stays considered until the owner picks.
  **Layman:** The owner liked a new look for the app; which parts to build, and in what order, is still theirs to choose.
  Kind: ux.
  Source: user-request-2026-10-12.

## 13. Confirm the signs of success

One item per discovery sign that no other item claimed on 2026-10-12. Each runs
the sign by hand against real Sony servers and a real RPCS3, and names the
shipped items that built it.

- 📋 [RPSN-0089] **Confirm S1: every listed update downloads in full and matches Sony's checksum.**
  Pick game IDs with several updates and download them all. Check every
  file completes and passes the SHA1 check, and that a corrupted file is
  not kept. Built by the upstream download code; RPSN-0025 re-hashes after
  a resume.
  Progress 2026-10-12 (verify-delivery, CLI run): PS3 NPUB30826 01.07
  and PS4 CUSA01021 01.54 downloaded and verified; PSVita PCSA00007
  01.01 failed verification because Sony's Vita sha1sum is upper case
  (fixed in 981f8ff; regression test
  hash_file_accepts_upper_case_checksum). Still open: a failed file is
  kept (RPSN-0116).
  **Layman:** Check that every update for a game downloads completely and undamaged.
  Kind: test.
  Source: check-queue-2026-10-12.
  Lanes: PSN.

- 📋 [RPSN-0090] **Confirm S2: a dropped connection resumes and still ends verified.**
  Interrupt a large download mid-way and restart it. Check it resumes from
  where it stopped and finishes with a matching checksum. Built by
  RPSN-0024, RPSN-0025 and RPSN-0030.
  Progress 2026-10-12: not run. Read: any non-206 reply during resume
  truncated the partial file; fixed in 981f8ff (error statuses now leave
  it intact). Content-Range is still unchecked (RPSN-0118).
  **Layman:** Check that an interrupted download picks up where it left off.
  Kind: test.
  Source: check-queue-2026-10-12.
  Lanes: PSN.

- 📋 [RPSN-0091] **Confirm S5: PS3, PS4 and PSVita game IDs each return their updates.**
  Look up one known game ID for each console and download an update from
  each list, with S1 holding for each. PSVita support was built by
  RPSN-0041, RPSN-0043 and RPSN-0047.
  Progress 2026-10-12 (verify-delivery, CLI run): PS3 NPUB30826, PS4
  CUSA01021 and PSVita PCSA00007 each listed their updates. The psn
  network tests also pass in ./scripts/local-ci.sh.
  **Layman:** Check that updates are found for PS3, PS4 and Vita games.
  Kind: test.
  Source: check-queue-2026-10-12.
  Lanes: PSN.

- 📋 [RPSN-0092] **Confirm S6: a downloaded update installs into RPCS3 and shows the new version.**
  Download a PS3 update, install it through the app, then open RPCS3 and
  check the game reports the new version. Built by RPSN-0053.
  Progress 2026-10-12: not run (no RPCS3 on the test machine). Read: the
  window app's Install to RPCS3 panicked outside the tokio runtime and
  installed the first .pkg in the folder; CLI --install did the same,
  even after a failed download. Both fixed in 981f8ff.
  **Layman:** Check that an update installed through the app shows up in RPCS3.
  Kind: test.
  Source: check-queue-2026-10-12.
  Lanes: RPCS3.

- 📋 [RPSN-0093] **Confirm S7: a library scan marks every installed game correctly.**
  Scan an RPCS3 library holding both up-to-date and outdated games, and
  check each is marked correctly. Built by RPSN-0050, RPSN-0051,
  RPSN-0052 and RPSN-0057.
  Progress 2026-10-12: a disc game with no update installed was never
  marked, because the disc PARAM.SFO's APP_VER was discarded; fixed in
  981f8ff (regression test
  scan_library_reads_disc_app_ver_when_no_update_installed). Unsettled:
  whether games.yml lists digital games at all.
  **Layman:** Check that the library scan correctly shows which games need updates.
  Kind: test.
  Source: check-queue-2026-10-12.
  Lanes: RPCS3, Window app.

- 📋 [RPSN-0094] **Confirm S8: RPCS3 is found without typing a path, and can be overridden.**
  On a fresh settings file, check the app finds the RPCS3 folder by itself,
  and that a path typed in settings replaces it. Built by RPSN-0049,
  RPSN-0054 and RPSN-0056.
  Progress 2026-10-12: not run (no RPCS3 on the test machine). Read:
  macOS and Windows detection rarely succeed (RPSN-0112, RPSN-0113); the
  AppImage fallback can pick any program (RPSN-0110).
  **Layman:** Check that the app finds RPCS3 by itself and lets you change the folder.
  Kind: test.
  Source: check-queue-2026-10-12.
  Lanes: RPCS3.

- 📋 [RPSN-0095] **Confirm S10: a DLC zip the user has installs into RPCS3 and the game sees it.**
  Install a DLC zip through the app, then check in RPCS3 that the game
  lists the DLC and its licence file is in place. Built by
  `content::install_dlc_zip`, which no roadmap item recorded.
  Progress 2026-10-12: not run (no RPCS3). Read: the Content Browser
  Install buttons panicked outside the tokio runtime (fixed in 981f8ff);
  multi-file zips and the licence folder remain (RPSN-0111, RPSN-0112,
  RPSN-0098).
  **Layman:** Check that installing a DLC zip you own makes the DLC appear in the game.
  Kind: test.
  Source: check-queue-2026-10-12.
  Lanes: Content, RPCS3.

## 14. Audit follow-ups (2026-10-12)

Findings from the 2026-10-12 audit (check-code, review-code, verify-delivery)
not fixed in that session. The Critical and High findings were fixed in commit
981f8ff. Each item names its file and line as of that commit and the remedy the
reviewer proposed; check both against the code before fixing.

- ✅ [RPSN-0098] **Resolve the DLC licence folder through vfs.yml, and refuse when no RPCS3 folder is set.**
  src/egui/mod.rs:1731 builds exdata as config_dir/dev_hdd0/home/00000001/exdata, ignoring the dev_hdd0 path vfs.yml sets (used at :1009-1020). With no config dir the fallback is an empty path, so the .rap is copied into the current folder. Remedy: resolve through the same vfs logic; refuse when the config dir is unset.
  Progress (2026-10-12, 5474e04): resolve_dev_hdd0 added and used at all
  four sites; DLC Install needs the config folder. Not yet tried by hand
  in the window app; flip shipped after that or on the next session's
  review.
  Resolved (2026-10-12, 5474e04); the window-app DLC install is still
  untried by hand.
  **Layman:** DLC licence files can land in the wrong folder.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lanes 1+4 MEDIUM.
  Lanes: Window app, Content.

- ✅ [RPSN-0099] **Append a new batch search to a running one instead of replacing it.**
  src/egui/mod.rs:983-986 and :1063-1067 assign batch_search_queue while a batch runs: queued serials are dropped, and in-flight results can make errors exceed the total, so batch_search_total - errors at :941 underflows. Remedy: append to the batch; use saturating_sub.
  Resolved 2026-10-12: both batch sources (batch file, RPCS3 scan) now call
  VolatileData::enqueue_batch_search in src/egui/search.rs, which appends
  to a running batch and starts fresh counts only after one has finished.
  The summary uses saturating_sub. Test:
  second_batch_joins_a_running_one.
  **Layman:** Starting a second batch search can lose game IDs or crash a debug build.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lane 1 MEDIUM.
  Lanes: Window app.

- ✅ [RPSN-0100] **Report folders the migration refused, and check against the scanned folder.**
  src/egui/mod.rs:3081 and :3025 toast only the migrated count; utils::migrate_directories skips refused entries silently. The preview uses the download path at menu time (:2977), the check uses the path at click time. Remedy: return (migrated, skipped), show both, and pass the scanned folder.
  Resolved 2026-10-12: utils::migrate_directories returns (migrated,
  skipped); the window app warns when any folder was skipped. The
  dialog keeps migration_scan_root, the folder the preview scanned, and
  the migration checks containment against it rather than the folder
  set at click time. Test: migrate_directories_skips_moves_outside_
  download_folder now asserts (1, 1).
  **Layman:** The folder-migration message hides folders it skipped.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lane 1 MEDIUM.
  Lanes: Window app, Files.

- ✅ [RPSN-0101] **Log and keep stored settings that fail to load instead of resetting them.**
  src/egui/mod.rs:564 eframe::get_value(...).unwrap_or_default() resets every setting on a deserialize failure (for example after a downgrade meeting a new AppTheme variant), and the next save overwrites the stored copy. Remedy: log the failure; per-field serde defaults or a lenient enum fallback.
  Resolved 2026-10-12: AppSettings takes #[serde(default)], so a missing
  field takes its default; theme and directory_format read through
  default_if_unknown, so an unknown value falls back alone and the rest
  load. A file that still fails to load is logged at error level. Not
  done: a backup of a wholly unreadable file, since eframe hands the app
  read-only storage at startup. Tests: unknown_theme_keeps_the_other_
  settings, missing_setting_takes_its_default, saved_settings_load_
  unchanged.
  **Layman:** A settings file the app cannot read silently resets every setting.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lane 1 MEDIUM.
  Lanes: Window app.

- ✅ [RPSN-0102] **Cap redirects in the Sony redirect policy.**
  src/security.rs:185 reqwest::redirect::Policy::custom replaces reqwest's default limit of 10 and never checks attempt.previous().len(). Remedy: error when previous().len() >= 10.
  Resolved 2026-10-12: security::check_redirect decides each hop and
  refuses with TooManyRedirects once previous() exceeds MAX_REDIRECTS
  (10). That matches reqwest's own default (previous() includes the
  original request), so ten redirects are followed, not the nine a
  >= 10 check would allow. Test: redirects_stop_after_the_limit.
  **Layman:** A redirect loop on Sony's side could hang a download forever.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lane 2 MEDIUM.
  Lanes: Security.

- ✅ [RPSN-0103] **Fail a firmware download that ends short of its stated size.**
  src/psn/firmware.rs:226-229 sends DownloadSuccess without comparing downloaded with self.size, so a dropped connection leaves a truncated PUP reported as done. Remedy: when size is Some and differs, send DownloadFailure and return an error.
  Resolved 2026-10-12: firmware::check_complete compares the bytes
  received with the stated size; a mismatch sends DownloadFailure and
  returns the new DownloadError::Incomplete {received, expected}. Both
  front ends handle the variant. Test: short_firmware_download_fails.
  **Layman:** A cut-off firmware download is reported as a success.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lane 2 MEDIUM.
  Lanes: PSN.

- ✅ [RPSN-0104] **Return reader errors from the update XML parser.**
  src/psn/parser.rs:137 `_ => {}` swallows Err from read_event_into (the firmware parser returns it). Unsettled: whether quick-xml (0.37.5 and 0.38.4 both in Cargo.lock) advances after an error; if not, this loops. Remedy: Err(e) => return Err(ParseError::XmlParsing(e)).
  Resolved (2026-10-13): parse_response now returns
  Err(ParseError::XmlParsing(e)) on a reader error, as the firmware parser
  already did. The project is on quick-xml 0.42 now; the new test
  (parse_response_returns_reader_errors) failed on its assertion before the
  fix and did not hang, so the reader moves past an error and the loop could
  not spin. The return also ends the loop.
  **Layman:** A damaged reply from Sony can give a half-filled update list with no error.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lane 2 MEDIUM.
  Lanes: PSN.

- ✅ [RPSN-0105] **Accept every -t flag and any whitespace between serials.**
  src/cli/mod.rs:103 reads args.titles[0].split(' '): every -t after the first is dropped, and a double space yields an empty serial. Remedy: flat_map over all titles with split_whitespace, then normalise and dedup as batch.rs does.
  Resolved (2026-10-13): serials_from_titles in src/cli/mod.rs reads
  every -t value, splits on any whitespace, normalises with
  parse_title_id and drops repeats. Validation stays with the search,
  which already reports a bad serial by name. The -t help text now says
  it can be given more than once. Test:
  serials_from_titles_reads_every_flag_and_any_whitespace (red on the
  old code, green after).
  **Layman:** Giving -t twice silently ignores the second game.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lane 3 MEDIUM.
  Lanes: Command line.

- 📋 [RPSN-0106] **Exit non-zero when a command-line run had failures.**
  src/cli/mod.rs:157-194, 336-386 and the install path print each failure and exit 0, so --silent batch runs that fail look successful; the same in download_firmware_cli (:574-583, :654-656). Remedy: count failures and exit with a non-zero code.
  **Layman:** Scripts cannot tell when a silent run failed.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lane 3 MEDIUM.
  Lanes: Command line.

- ✅ [RPSN-0107] **Strip control characters from Sony titles and batch-file text before printing.**
  src/cli/mod.rs:229-236, 298, 406, 423 print the game title from Sony's reply, and :86-88 print raw batch-file text, unfiltered; escape sequences reach the terminal. Remedy: drop c.is_control() characters before printing.
  Resolved (2026-10-12, 5be5291): security::strip_control applied
  to Sony titles, PARAM.SFO values, firmware versions and batch warning
  text. Same commit fixes titles containing an entity (such as "&amp;")
  being cut at the first one, a quick-xml 0.38+ change taken in
  RPSN-0125.
  **Layman:** Text from Sony or a batch file can mess with the terminal.
  Kind: security.
  Source: audit-2026-10-12 review-code lane 3 MEDIUM.
  Lanes: Command line.

- 📋 [RPSN-0108] **Decide whether a build with both front ends is supported, then fix main.rs or design.md.**
  src/main.rs:92-96 runs cli::start_app first; with no CLI arguments it exits (cli/mod.rs:104-106), so the window never starts. windows_subsystem also hides CLI output on Windows. Owner decision: compile_error! on cli+egui, or choose the front end at runtime.
  **Layman:** A build with both the window app and the command line never opens the window.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lane 3 MEDIUM.
  Lanes: Entry.

- ✅ [RPSN-0109] **Make the local gate match CI: add the cli build legs and fix its header.**
  scripts/local-ci.sh:4-11 says .github/workflows/ci.yml calls it; no such file exists and the workflows restate their own steps. The gate never builds --no-default-features --features cli, where clippy failed until 981f8ff. Remedy: add clippy, build and test for the cli feature; correct the header.
  Resolved 2026-10-12: scripts/local-ci.sh runs the workflows' own Linux
  commands (nightly rustfmt, clippy per feature, release build per
  feature) and names the Windows and macOS legs as not run. Needs the
  nightly toolchain with rustfmt (installed on this machine 2026-10-12).
  Full run green: 69 + 70 tests.
  **Layman:** The local checks miss problems that CI's command-line build catches.
  Kind: chore.
  Source: audit-2026-10-12 review-code lane 3 MEDIUM + check-code.
  Lanes: Build.

- 📋 [RPSN-0110] **Only fall back to an AppImage whose name says rpcs3.**
  src/rpcs3/auto_detect.rs:126-131 returns the first *.AppImage in the folder when the expected one is missing; install_pkg then runs it with --installpkg. Remedy: require rpcs3 in the name (case-insensitive) or return None.
  **Layman:** Auto-detect could pick an unrelated program as RPCS3.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lane 4 MEDIUM.
  Lanes: RPCS3.

- 📋 [RPSN-0111] **Install every PKG and licence in a multi-file DLC zip.**
  src/content.rs:198-201 keep only the last .pkg and .rap from a zip, yet return Ok. Remedy: collect all, install or copy each, fail if any step fails.
  **Layman:** DLC bundles install only their last item.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lane 4 MEDIUM.
  Lanes: Content.

- 📋 [RPSN-0112] **Prepare the licence folder before installing a DLC package.**
  src/content.rs:223-226 copy the .rap after the PKG install; a missing exdata folder (fresh RPCS3 profile) fails the copy after the PKG is in. Remedy: create or check the folder first, or name the partial state in the error.
  **Layman:** A DLC can install without its licence and say only that it failed.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lane 4 MEDIUM.
  Lanes: Content.

- 📋 [RPSN-0113] **Bound DLC zip extraction and extract outside the system temp folder.**
  src/content.rs:174 and :195 extract with no size limit into the system temp folder (RAM-backed on many systems). Unsettled: Flatpak RPCS3 may not see host /tmp. Remedy: extract beside the downloads and cap the total uncompressed size from the zip headers.
  **Layman:** A large or hostile DLC zip can fill memory.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lane 4 MEDIUM.
  Lanes: Content.

- 📋 [RPSN-0114] **Find RPCS3.app on macOS.**
  src/rpcs3/auto_detect.rs:96-106 uses `which rpcs3`; RPCS3 ships as RPCS3.app, not on PATH. Remedy: also check /Applications/RPCS3.app/Contents/MacOS/rpcs3 and ~/Applications.
  **Layman:** On a Mac, the app almost never finds RPCS3 by itself.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lane 4 MEDIUM.
  Lanes: RPCS3.

- 📋 [RPSN-0115] **Find portable RPCS3 on Windows.**
  src/rpcs3/auto_detect.rs:172-183 probe %APPDATA%\rpcs3 and Program Files; RPCS3 is portable and keeps its config beside rpcs3.exe. Remedy: probe the folder of an rpcs3.exe found on PATH; say in the UI that Windows usually needs the path set.
  **Layman:** On Windows, the app rarely finds RPCS3 by itself.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lane 4 MEDIUM.
  Lanes: RPCS3.

- 📋 [RPSN-0116] **Remove a download that fails its checksum, once retries are exhausted.**
  S1 says a file that fails the check is never kept. Reproduced 2026-10-12: PCSA00007 01.01 failed verification (before F12) and the 13,977,360-byte file stayed. Resume relies on keeping a partial file, so delete only after the final failed attempt; decide whether PSN or the front end owns it (src/cli/mod.rs:336-389).
  **Layman:** A download that failed its check stays on disk.
  Kind: fix.
  Source: audit-2026-10-12 verify-delivery VD2 (S1).
  Lanes: PSN, Command line.

- 📋 [RPSN-0117] **Window app: small review findings from the 2026-10-12 audit.**
  src/egui/mod.rs: :2812 says the executable is auto-detected, but scan sets only the config dir. :803-806 and :2546-2547 retries bypass max_concurrent_downloads. :1541-1549 and :2899 Escape or X keeps a previewed theme. :3023-3031 Escape mid-migration skips the completion check. :2587, :2595, :3302-3310 ignore clipboard and xdg-open errors. :929 expect() panics if the client failed to build (only logged at :440). :823-824 a stored max of 0 is not clamped. :3281 default pkgs/ is relative to the current folder. :859-865 logs the Security error twice.
  **Layman:** Small window-app fixes: wrong hints, ignored errors, unclamped limits.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lane 1 LOW.
  Lanes: Window app.

- 📋 [RPSN-0118] **PSN and Files: small review findings from the 2026-10-12 audit.**
  src/psn/firmware.rs:184 creates the Firmware folder before the containment check at :190; move the check first. src/psn/mod.rs:416 a 206 is not checked against Content-Range. src/psn/mod.rs:256 merge_parts uses create_pkg_path, but a failed migration leaves downloads in the old folder (utils.rs return Ok(alt.clone())); share one resolver. src/utils.rs:166/175 check-then-rename window. src/psn/mod.rs:35: document beside hash_file that the SHA-1 arrives over the same unauthenticated channel as the URL.
  **Layman:** Small download and file-handling fixes.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lane 2 LOW.
  Lanes: PSN, Files, Security.

- 📋 [RPSN-0119] **Command line: small review findings from the 2026-10-12 audit.**
  src/cli/mod.rs:39-43 a misspelt --directory-format silently uses the default (use a ValueEnum). src/main.rs:30-80 contradictory options are silently ranked (use conflicts_with). src/cli/mod.rs: unwrap() calls lack the comment CLAUDE.md rule 4 requires, and crossterm/flush unwraps panic on a closed pipe. scripts/local-ci.sh:74-86 --docs exits 0 verifying nothing. src/cli/mod.rs:513 sleeps 3 s after every title, silent mode included.
  **Layman:** Small command-line fixes: silent defaults and needless waits.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lane 3 LOW.
  Lanes: Command line, Entry, Build.

- 📋 [RPSN-0120] **RPCS3 and Content: small review findings from the 2026-10-12 audit.**
  src/rpcs3/sfo.rs:52 and config.rs:22, :40 check size then read unbounded (use take()). auto_detect scans /mnt and spawns which/flatpak on the window thread (egui/mod.rs:2078, :2948 call it synchronously). src/content.rs:215 extracts synchronously inside an async fn (spawn_blocking). src/content.rs:126 comment promises DLC subfolder layouts the code does not scan. src/rpcs3/config.rs:24 one non-string games.yml value fails the whole scan.
  **Layman:** Small RPCS3 and DLC fixes: unbounded reads and a blocking scan.
  Kind: review-fix.
  Source: audit-2026-10-12 review-code lane 4 LOW.
  Lanes: RPCS3, Content.

- 📋 [RPSN-0121] **Harden the GitHub workflows zizmor flagged.**
  zizmor, 25 findings over the 4 workflows: unpinned-uses 15 (High), artipacked 4, excessive-permissions 4, template-injection 1 (main.yml:54, informational), superfluous-actions 1 (:134). The workflows are the upstream author's; decide with the fork (RPSN-0096).
  **Layman:** Tighten the automated build scripts' security settings.
  Kind: audit-fix.
  Source: audit-2026-10-12 check-code zizmor.
  Lanes: Build.

- 📋 [RPSN-0122] **Tidy YAML style in the GitHub workflows.**
  yamllint -d relaxed (no project config): trailing-spaces 5, new-line-at-end-of-file 1, line-length 8, indentation 1, across .github/. Style only; adopting a .yamllint is the alternative.
  **Layman:** Formatting clean-up in the build scripts.
  Kind: chore.
  Source: audit-2026-10-12 check-code yamllint.
  Lanes: Build.

- 📋 [RPSN-0123] **Fix the spelling mistakes typos found.**
  Mergable/Unmergable (src/egui/mod.rs:850, 2316, 2340, 2555, 3208, 3213, 3328; src/psn/mod.rs:64, 248; STANDARDS.md:199) includes the enum variant PackagesUnmergable, so the rename touches every use. Plaform (STANDARDS.md:219; ROADMAP item text), preceeding (src/psn/parser.rs:101), PN (ROADMAP.md:757, 766; check context). ROADMAP text changes go through roadmap_log.
  **Layman:** Correct a handful of misspelt words in the code and docs.
  Kind: chore.
  Source: audit-2026-10-12 check-code typos.
  Lanes: Window app, PSN, Docs.

- 📋 [RPSN-0124] **Update STANDARDS.md's download-flow diagram.**
  STANDARDS.md 3.4 Download Flow says the file is truncated before streaming. The code resumes with Range/If-Range, and since 981f8ff an HTTP error status leaves the partial file untouched. Already stale before that commit.
  **Layman:** A diagram describes the old download steps.
  Kind: doc-fix.
  Source: audit-2026-10-12 close-findings sweep.
  Lanes: Docs.

- ✅ [RPSN-0125] **Upgrade every dependency and workflow action to its latest version, clearing the RustSec advisories.**
  Owner rule 2026-10-12: every dependency at its latest version; an older one only where a newer one is shown to break a feature, documented with the breaking version (see the standard item below).
  cargo audit 2026-10-12 (local DB, --no-fetch): 12 vulnerabilities, 7 warnings. Advisories: quick-xml 0.37.5/0.38.4/0.39.2 RUSTSEC-2026-0194, -0195 (fixed >=0.41.0; 0.37.5 via notify-rust); rustls 0.23.37 RUSTSEC-2026-0285 (>=0.23.45); rustls-webpki 0.103.10 RUSTSEC-2026-0098, -0099, -0104 (>=0.103.13); quinn-proto 0.11.14 RUSTSEC-2026-0185 (>=0.11.15); webbrowser 1.2.0 RUSTSEC-2026-0257 (>=1.2.2). Warnings: unsafe-libyaml (unmaintained, via serde_yaml_ng), rand 0.9.2, paste, ttf-parser, anyhow, event-listener, memmap2.
  Behind latest (resolved -> latest): dirs 6->7, sha1 0.10->0.11, hmac 0.12->0.13, sha2 0.10->0.11, zip 2.4->9.0, reqwest 0.12->0.13, rfd 0.15->0.17, eframe 0.32->0.36, egui-notify 0.20->0.23, notify-rust 4.12->4.18, tokio 1.50->1.53, plus patch bumps.
  Actions (latest release): actions/checkout v7.0.1, Swatinem/rust-cache v2.9.2, actions/upload-artifact v7.0.2, dawidd6/action-download-artifact v27, softprops/action-gh-release v3.0.3, c-hive/gha-remove-artifacts v1.8.0, mbrobbel/rustfmt-check 0.23.0. Workflows are upstream's; decide with RPSN-0096 and RPSN-0122.
  Re-run check-dependencies first; these figures are a dated snapshot.
  Resolved (2026-10-12): every crate and workflow action at its latest
  release, in commits c7ab91e..8dbf8a5, one dependency (or one set that
  must move together) per commit. cargo audit: 0 vulnerabilities; 2
  unmaintained warnings left (ttf-parser via egui, unsafe-libyaml via
  serde_yaml_ng, both at latest). Nothing held back, so no hold to record.
  Rust stable 1.94 -> 1.99 on this machine (eframe 0.36 needs 1.95).
  Local gate green: 70 + 71 tests. Not yet verified: the window and its
  file dialogs by hand on this build; the Windows and macOS CI legs
  (aws-lc-sys build); every workflow file, which runs only on GitHub
  (RPSN-0096).
  **Layman:** Bring every library the app uses up to date, including several with known security problems.
  Kind: security.
  Source: user-request-2026-10-12; cargo audit via pressless session 2026-10-12.
  Lanes: Build, PSN, Content.

- ✅ [RPSN-0126] **Write the dependency-version rule as a standard, with a record of versions known to break.**
  Owner rule 2026-10-12: always the latest version, for features and security. An older version only when no other way exists and a newer one explicitly breaks a feature; then record which version breaks what, so a later release can be re-tested and the pin lifted.
  First read ~/.claude/standards/dependencies.md: if the machine-wide standard already says this, apply it here; if not, the change belongs to the ~/.claude (claude-config) session, and a project override goes in docs/standards/. A new or changed standard takes CLAUDE.md rule 14's review-contract gate.
  Resolved (2026-10-12): dependencies.md already states latest-by-default
  and the broke-at ledger; the owner's rule is stricter on one point (no
  hold for a migration nobody has time for). Owner chose machine-wide, so
  that change and a standard ledger path went to claude-config (message
  612). Project side: docs/dependency-holds.md, the ledger, no rows.
  **Layman:** Write down the rule that libraries stay up to date, and keep a list of versions that broke something.
  Kind: doc.
  Source: user-request-2026-10-12.
  Lanes: Docs.

- 🚫 [RPSN-0129] **Draw the arrow shaft in resources/icon.svg.**
  The shaft is a vertical line painted with a gradient in the default
  objectBoundingBox units. A vertical line has a zero-width box, so a
  spec-following renderer (librsvg, checked with rsvg-convert) draws
  nothing. The PNG icons are fine. Fix: gradientUnits="userSpaceOnUse"
  with explicit coordinates, as the site logo does.
  Superseded (2026-10-12): the icon was redesigned (a game disc with an
  update badge) at the website session's request; the new icon.svg has
  no gradient on a zero-width line.
  Decided (2026-10-12, user, via the website session): the disc icon
  with the update badge and "PS3·PS4" lettering (8495ca1) is approved
  for the app and the site. Guidance: the icon need not say
  "PlayStation" but should imply it; plain console names do that, Sony
  marks stay out.
  **Layman:** The app's vector icon is missing the stem of its download arrow in some viewers.
  Kind: fix.
  Source: in-session-2026-10-12 (RPSN-0127 logo work).
  Lanes: Build.

- 📋 [RPSN-0130] **Fall back to X11 when the Wayland connection fails, instead of panicking.**
  With WAYLAND_DISPLAY set to a socket that does not exist and DISPLAY
  valid, run_native returns WaylandError(Connection(NoCompositor)) and
  src/main.rs panics on .expect("Failed to run egui app"). Seen under
  demoreel, which points Wayland at nothing on purpose. Research first:
  whether winit has an option to try X11 next, or whether to retry with
  WAYLAND_DISPLAY removed.
  **Layman:** The window app crashes at start if the Wayland setting points nowhere, even when X11 is available.
  Kind: fix.
  Source: in-session-2026-10-12 (RPSN-0127 screenshots).
  Lanes: UI.

- 📋 [RPSN-0131] **Say "version unknown" in the RPCS3 library window when a game has no installed version on record.**
  Seen on a scan where games.yml lists games but no PARAM.SFO gives an
  APP_VER: the header reads "3 game(s) installed, 0 with updates
  available", Installed and Status show "--", and Available shows a
  version. Correct by the code (nothing to compare against), but it
  reads as a contradiction. Say "in library" in the header and
  "version unknown" in Status.
  **Layman:** The library window calls a game "installed" but leaves its status blank when it can't tell the version, which looks like a contradiction.
  Kind: ux.
  Source: ants-projects-hub-website session 2026-10-12 (shot 3 review).
  Lanes: UI.

- 📋 [RPSN-0132] **Fix two stale passages in STANDARDS.md seen during the 2026-10-12 fixes.**
  1. § 4 (Domain model notes) spells `PlaformVariant`; the type is
     PlatformVariant.
  2. The technical-debt list says a new reqwest::Client is built for
     each get_info() and start_download() call; the client is now built
     once and shared (build_psn_client, passed into UpdatesApp::new).
  Check the rest of that list against the code while there.
  **Layman:** Two places in the project notes say things that are no longer true.
  Kind: doc-fix.
  Source: in-session-2026-10-12 (check-doc-facts over STANDARDS.md).
  Lanes: docs.
