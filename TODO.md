# rusty-psn TODO

## Priority Legend

| Priority | Meaning |
|---|---|
| P0 | Do first - foundational changes other work depends on |
| P1 | High priority - core feature or important fix |
| P2 | Medium priority - valuable enhancement |
| P3 | Nice to have - polish and extras |

---

## 1. Modernize the Codebase [P0]

Foundational work that should be done before adding new features, as it reduces friction for everything that follows.

- [ ] **1.1** Update Rust edition from 2018 to 2021 in `Cargo.toml`
- [ ] **1.2** Replace `#[macro_use] extern crate log` with explicit `use log::{info, warn, error}` imports
- [ ] **1.3** Fix the `PlaformVariant` typo - rename to `PlatformVariant` across the codebase
- [ ] **1.4** Add missing `#[cfg(test)]` to the `tests` module in `psn/mod.rs` (currently compiles in release builds)
- [ ] **1.5** Deduplicate XML parsing logic in `parser.rs` - the `Event::Start` and `Event::Empty` branches for `<package>` elements have near-identical attribute extraction code (lines 48-88 vs 114-148); extract into a shared function
- [ ] **1.6** Fix settings reset path inconsistency - non-macOS reset sets `/pkgs` (absolute root) instead of `pkgs/` (relative, matching the default)
- [ ] **1.7** Fix busy-wait polling in CLI - the download loop in `cli/mod.rs` calls `promise.ready()` in a tight loop with no yield/sleep, consuming 100% CPU; add a small `tokio::time::sleep` or use `promise.block_and_take()` with channel-based progress
- [ ] **1.8** Update dependencies to latest compatible versions
- [ ] **1.9** Add `clippy` linting to CI pipeline alongside `rustfmt`
- [ ] **1.10** Consider migrating from `poll-promise` to native `tokio` channels + `eframe`'s `request_repaint_after()` for a cleaner async-UI bridge

---

## 2. Improved Download Directory Structure [P0]

Restructure download paths to match the user's existing collection layout and improve organization.

### Current Behavior
The app saves to: `{download_path}/{serial} - {title}/{pkg_file}`
(e.g., `pkgs/BCUS98111 - God of War III/UP9000-BCUS98111_00-PATCH00000000000-A0103-V0100-PE.pkg`)

### Desired Behavior
Save to: `{download_path}/{title}/{serial}/{pkg_file}`
(e.g., `pkgs/God of War III (USA)/BCUS98111/UP9000-BCUS98111_00-...pkg`)

Target folder structure:
```
Batman: Arkham Origins/
  BLUS31147/
    UP1018-BLUS31147_00-BATMANORIGIPATCH-A0106-V0100-PE.pkg
God of War III (USA)/
  BCUS98111/
    UP9000-BCUS98111_00-PATCH00000000000-A0103-V0100-PE.pkg
```

- [ ] **2.1** Change `create_new_pkg_path()` in `utils.rs` to produce `{download_path}/{sanitized_title}/{serial}/{pkg_file}` instead of `{download_path}/{serial} - {title}/{pkg_file}`
- [ ] **2.2** Add migration logic to detect and optionally reorganize existing downloads from old format (`{serial} - {title}/`) and current format (`{title}/{pkg_file}` flat) into the new `{title}/{serial}/` structure
- [ ] **2.3** Handle the region suffix in title names - Sony's API returns bare titles (e.g., "God of War III") but existing folders include region (e.g., "God of War III (USA)"); provide a config option or auto-detect region from serial prefix patterns
- [ ] **2.4** Add a GUI setting for download directory structure format (let user choose between current and new layout)
- [ ] **2.5** Remove the legacy `create_old_pkg_path()` migration code in `utils.rs` (or update it to handle all three historical formats)

---

## 3. Batch Download [P1]

Support downloading updates for multiple games in one operation without manual serial entry.

- [ ] **3.1** Add support for importing a text file containing serials (one per line, with optional comments via `#`)
  ```
  # My PS3 games
  BCUS98111   # God of War III
  BCUS98232   # God of War Ascension
  BLUS31147   # Batman Arkham Origins
  ```
- [ ] **3.2** Add a "Batch Import" button to the GUI with a file picker (`.txt` filter)
- [ ] **3.3** Add CLI flag `--batch-file <path>` to load serials from a file
- [ ] **3.4** Add "Import from RPCS3" button that reads `games.yml` to auto-populate the serial list (see Section 7 - RPCS3 Integration)
- [ ] **3.5** Implement a download queue manager that processes batch items concurrently with a configurable parallelism limit (default: 2-3 simultaneous downloads)
- [ ] **3.6** Add batch progress overview in GUI - show overall progress (X of Y titles done), not just individual package progress
- [ ] **3.7** CLI silent mode (`-s`) should work seamlessly with batch files for fully automated downloading
- [ ] **3.8** Add "Download All Missing Updates" - scan existing download folder, compare against queried update info, download only what's missing or incomplete

---

## 4. Download Resume [P1]

Support resuming partially downloaded files instead of re-downloading from scratch.

- [ ] **4.1** Use HTTP `Range` header to request only the remaining bytes when a partial file exists
  - Check if Sony's CDN supports `Accept-Ranges: bytes` (test with a HEAD request)
  - If supported: send `Range: bytes={existing_size}-` header
  - If not supported: fall back to full re-download (current behavior)
- [ ] **4.2** Before resuming, verify the existing partial data's integrity:
  - Option A: Hash the partial content and compare against a stored partial hash (would need to save progress metadata)
  - Option B: Simply check file size against expected size and resume if smaller
  - Option C: Re-hash the complete file after resume completes (current approach, lowest risk)
- [ ] **4.3** Store download progress metadata in a `.rusty-psn-progress.json` sidecar file per download directory:
  ```json
  {
    "url": "https://...",
    "expected_size": 654462304,
    "expected_sha1": "abc123...",
    "downloaded_bytes": 234567890,
    "last_modified": "2026-03-23T10:30:00Z"
  }
  ```
- [ ] **4.4** Change `create_pkg_file()` to use `OpenOptions::append()` when resuming instead of truncating
- [ ] **4.5** Update the download progress reporting to show "Resuming from X MB" in both GUI and CLI
- [ ] **4.6** Add a "Retry failed downloads" button in the GUI that attempts to resume all items in the `failed_downloads` list
- [ ] **4.7** Handle Sony's server instability gracefully - the app already detects short transfers (`received_data < self.size`); instead of just reporting failure, auto-queue for resume

---

## 5. Security Hardening [P1]

Ensure the application has no vulnerabilities and follows security best practices.

- [ ] **5.1** **Audit `danger_accept_invalid_certs(true)`**: This is currently required for Sony's servers but is applied globally. Scope it to only Sony PSN domains:
  - Create a custom `reqwest::Client` per target domain
  - Only disable cert validation for `*.playstation.net` and `*.dl.playstation.net`
  - Use normal TLS validation for any other network requests (future features)
- [ ] **5.2** **Path traversal protection**: The `sanitize_title()` function replaces OS-invalid chars, but does not prevent directory traversal. Add checks:
  - Reject or strip `..` path components in titles and filenames
  - Reject absolute paths in package URLs that could write outside the download directory
  - Validate that resolved download paths are children of the configured download directory
- [ ] **5.3** **URL validation**: Before making HTTP requests, validate that package URLs point to expected Sony domains:
  - Whitelist: `*.playstation.net`, `*.dl.playstation.net`
  - Reject URLs pointing to other domains (could be injected via malformed XML)
- [ ] **5.4** **Input validation**: Harden serial input parsing:
  - Enforce maximum length (9 characters after normalization)
  - Allow only alphanumeric characters after normalization
  - Reject serials that don't match known prefix patterns
- [ ] **5.5** **XML parsing safety**: Review `quick-xml` usage for:
  - XML entity expansion attacks (billion laughs / XML bomb) - ensure entity expansion is disabled or limited
  - Extremely large responses - add a maximum response size limit to `reqwest` calls
- [ ] **5.6** **Dependency audit**: Add `cargo audit` to CI pipeline to check for known vulnerabilities in dependencies
- [ ] **5.7** **File permissions**: On Unix, set downloaded `.pkg` files to `0644` (read-only for group/other) rather than inheriting umask
- [ ] **5.8** **Secrets in binary**: The PS4 HMAC key is hardcoded. While it's publicly known, document why it's acceptable and add a comment explaining its provenance
- [ ] **5.9** **Memory safety**: Review all `unwrap()` calls and replace with proper error handling where a panic would be unacceptable (especially in async contexts where a panic kills the runtime)
- [ ] **5.10** **TOCTOU in file operations**: The check-then-create pattern in `create_pkg_file()` has a time-of-check-to-time-of-use gap. Use atomic file operations where possible

---

## 6. PSVita Support [P2]

Add PlayStation Vita game update downloads. Research status: **confirmed feasible**.

### Confirmed Technical Details

**URL Format** (different from both PS3 and PS4):
```
https://gs-sec.ww.np.dl.playstation.net/pl/np/{TITLE_ID}/{HMAC_HASH}/{TITLE_ID}-ver.xml
```

| Platform | Server | Path | HMAC Required |
|---|---|---|---|
| PS3 | `a0.ww.np.dl.playstation.net` | `/tpl/np/{id}/{id}-ver.xml` | No |
| PSVita | `gs-sec.ww.np.dl.playstation.net` | `/pl/np/{id}/{hmac}/{id}-ver.xml` | Yes |
| PS4 | `gs-sec.ww.np.dl.playstation.net` | `/plo/np/{id}/{hmac}/{id}-ver.xml` | Yes |

**HMAC-SHA256 Key** (different from PS4):
```
E5E278AA1EE34082A088279C83F9BBC806821C52F2AB5D2B4ABD995450355114
```
HMAC input: `"np_{TITLE_ID}"` (same pattern as PS4). Key provenance: discovered by developer Proxima, published via VTU_HMAC tool and widely used by PKGj, NPS Browser, VitaDB.

**Serial Prefixes**:
- Primary: `PCSA`, `PCSB`, `PCSC`, `PCSD`, `PCSE`, `PCSF`, `PCSG`, `PCSH`
- Physical card: `VCAS`, `VCJS`, `VCJX`, `VCKS`, `VLAS`, `VLJM`, `VLJS`, `VLKS`
- System apps: `NPXS` (probably not needed)

**XML Response**: Same `<titlepatch>` structure as PS3. The existing parser will work as-is. Vita adds extra attributes (`psp2_system_ver`, `content_id`) that the parser's `_ => {}` catch-all safely ignores.

Example verified response for PCSA00007 (Hustle Kings):
```xml
<titlepatch status="alive" titleid="PCSA00007">
  <tag name="PCSA00007_T0" signoff="true">
    <package version="01.01" size="13977360"
             sha1sum="25E00E3CB5DDFB57BCDBAF53F283ECFB40EE136D"
             url="http://gs.ww.np.dl.playstation.net/ppkg/np/PCSA00007/..."
             psp2_system_ver="23461888"
             content_id="UP9000-PCSA00007_00-UHUSTLEKIN000001">
      <paramsfo><title>Hustle Kings(TM)</title></paramsfo>
    </package>
  </tag>
</titlepatch>
```

### Implementation Tasks
- [ ] **6.1** Add `PlatformVariant::PSVita` to the platform enum in `psn/utils.rs`
- [ ] **6.2** Update `get_platform_variant()` to detect Vita serial prefixes (`PCSA`-`PCSH`, `VC*`, `VL*`)
- [ ] **6.3** Update `get_update_info_url()` with a new PSVita branch:
  - Use HMAC key `E5E278AA1EE34082A088279C83F9BBC806821C52F2AB5D2B4ABD995450355114`
  - Use base path `/pl/np/` (not `/plo/np/` like PS4)
  - Same HMAC computation as PS4: `HMAC-SHA256("np_{title_id}")`
- [ ] **6.4** Test with known Vita serials: PCSA00007 (Hustle Kings), PCSE00120 (Persona 4 Golden), PCSA00126 (Killzone Mercenary)
- [ ] **6.5** Test whether Vita `.pkg` files use SHA1 suffix verification (like PS3) or whole-file hashing (like PS4)
- [ ] **6.6** Update the GUI and CLI to display `[PSVita]` platform tag
- [ ] **6.7** Add Vita-specific test cases to `psn/mod.rs`
- [ ] **6.8** Optionally parse `psp2_system_ver` to display minimum firmware requirement in the UI

### Sources
- [Vita Developer Wiki - Online Connections](https://www.psdevwiki.com/vita/Online_Connections)
- [VitaSmith/VitaDB - Update.cs](https://github.com/VitaSmith/VitaDB/blob/master/Update.cs)
- [devnoname120/vitanpupdatelinks](https://github.com/devnoname120/vitanpupdatelinks)
- [blastrock/pkgj](https://github.com/blastrock/pkgj)

---

## 7. RPCS3 Integration [P1]

Integrate directly with the RPCS3 emulator to auto-detect games and install updates.

### RPCS3 Directory Structure
```
<rpcs3_config>/                    # User-configurable, auto-detected
  games.yml                        # Serial -> game path mapping
  vfs.yml                          # Virtual filesystem path mappings
  dev_hdd0/game/                   # Installed game data & updates
    {SERIAL}/                      # e.g., BCUS98111 (God of War III)
      PARAM.SFO                    # Game metadata (title, version, serial)
      ICON0.PNG
      USRDIR/                      # Update data files
  patches/
    patch.yml                      # Game patches
```

### Key Files
- **`games.yml`**: Maps serials to ISO/folder paths. Format: `SERIAL: /path/to/game.iso`
- **`vfs.yml`**: Defines `$(EmulatorDir)` and virtual device paths. `dev_hdd0` is at `$(EmulatorDir)dev_hdd0/`
- **`PARAM.SFO`**: Binary metadata file per game containing `TITLE`, `TITLE_ID`, `APP_VER`, `VERSION`

All RPCS3 paths are auto-detected with manual override in settings. No paths are hardcoded.

### Implementation Tasks
- [ ] **7.1** **Auto-detect RPCS3 installation**: Search common paths:
  - Linux AppImage: `~/.config/rpcs3/` or `*.AppImage.home/.config/rpcs3/`
  - Linux native: `~/.config/rpcs3/`
  - Windows: `%APPDATA%/rpcs3/` or alongside `rpcs3.exe`
  - macOS: `~/Library/Application Support/rpcs3/`
  - Allow manual path override in settings
- [ ] **7.2** **Parse `games.yml`**: Read the serial-to-path mapping to build a list of all installed games with their serials
- [ ] **7.3** **Parse `PARAM.SFO`**: Read binary SFO files from `dev_hdd0/game/{serial}/PARAM.SFO` to extract:
  - `TITLE` - game name
  - `APP_VER` - currently installed update version
  - `TITLE_ID` - serial (for cross-referencing)
- [ ] **7.4** **"Scan RPCS3 Library" button**: In the GUI, add a button that:
  1. Reads `games.yml` to get all serials
  2. Queries Sony's API for updates for each serial (with progress indicator)
  3. Compares available update versions against installed versions (from `PARAM.SFO`)
  4. Shows which games have updates available vs already up-to-date
- [ ] **7.5** **Install updates directly to RPCS3**: After downloading a `.pkg` file:
  - Option A: Copy/move the `.pkg` to a staging area and invoke RPCS3's package installer (RPCS3 can install `.pkg` files via CLI: `rpcs3 --installfw` / `rpcs3 --installpkg`)
  - Option B: Parse and extract the `.pkg` contents directly into `dev_hdd0/game/{serial}/` (more complex but doesn't require RPCS3 running)
  - Prefer Option A for reliability; fall back to manual copy
- [ ] **7.6** **Add RPCS3 path to settings**: New setting `rpcs3_path` in `AppSettings` with folder picker and auto-detect button
- [ ] **7.7** **Add RPCS3 integration to CLI**: New flag `--rpcs3-path <path>` and `--scan-rpcs3` for automated scanning
- [ ] **7.8** **Parse `vfs.yml`**: Resolve `$(EmulatorDir)` to find the actual `dev_hdd0` location (it might be remapped)
- [ ] **7.9** **Show installed vs available version**: In the results list, display the currently installed version alongside the latest available version (e.g., "Installed: 1.03 | Available: 1.10")
- [ ] **7.10** **Batch update all outdated games**: "Update All" button that downloads and installs updates for every game that has a newer version available

---

## 8. Add Themes [P2]

Add visual theming support to the egui GUI.

- [ ] **8.1** Add a theme selector to the settings window with at least:
  - **Dark** (egui default `Visuals::dark()`)
  - **Light** (`Visuals::light()`)
  - **RPCS3 Blue** (custom theme matching RPCS3's UI colors)
  - **PlayStation** (dark theme with PS brand blue `#006FCD` accents)
- [ ] **8.2** Persist selected theme in `AppSettings` (already has eframe persistence)
- [ ] **8.3** Apply theme on startup in `UpdatesApp::new()` via `cc.egui_ctx.set_visuals()`
- [ ] **8.4** Add custom accent colors for status indicators:
  - Success: green (keep current)
  - Error: red (keep current)
  - In-progress: accent color from theme (currently hardcoded yellow `1.0, 1.0, 0.6`)
  - Neutral: theme default
- [ ] **8.5** Allow theme hot-switching without restart (apply immediately on selection)
- [ ] **8.6** Consider loading custom themes from a TOML/JSON file for user-created themes
- [ ] **8.7** Ensure all hardcoded `egui::Rgba::from_rgb(...)` and `egui::Color32` values in `egui/mod.rs` use theme-aware colors instead of fixed values

---

## 9. Additional Enhancements [P3]

Lower priority items that would improve the overall experience.

- [ ] **9.1** **Download speed display**: Show current download speed (KB/s, MB/s) alongside progress
- [ ] **9.2** **ETA calculation**: Estimate time remaining based on rolling average download speed
- [ ] **9.3** **Download history**: Persist a log of completed downloads with timestamps, shown in a "History" tab
- [ ] **9.4** **Concurrent serial search**: The GUI currently allows only one search at a time (`search_promise` is a single `Option`); allow concurrent searches
- [ ] **9.5** **Package URL copy**: Add a "Copy URL" button for each package to allow downloading via external tools
- [ ] **9.6** **Export update list**: Export search results as JSON/CSV for documentation or sharing
- [ ] **9.7** **Localization**: The GUI embeds Japanese fonts already; add basic i18n framework for UI strings
- [ ] **9.8** **System tray**: Minimize to system tray during long downloads (Linux: `tray-icon` crate)
- [ ] **9.9** **Flatpak/Snap packaging**: For easier Linux distribution alongside the current AppImage-targeted binary
- [ ] **9.10** **PSP support**: PSP uses similar NP serial prefixes (`NPUG`, `NPEH`, etc.) and the same update URL format; could be added alongside PSVita with minimal effort

---

## 10. Custom Application Icon [P2]

Create a unique, recognizable icon for rusty-psn that displays in the taskbar/task manager and on the compiled binary.

- [ ] **10.1** Design a custom icon representing rusty-psn's purpose (PlayStation update downloader + Rust)
  - SVG source for scalability
  - Must look good at small sizes (16x16, 32x32) and large sizes (256x256, 512x512)
  - Color scheme: PlayStation blue (#006FCD) with Rust orange (#CE422B) accent, or a unique combination
- [ ] **10.2** Generate platform-specific icon formats:
  - Windows: `.ico` file (multi-resolution: 16, 32, 48, 64, 128, 256)
  - Linux: `.png` files at standard sizes (48, 64, 128, 256, 512) + `.desktop` file
  - macOS: `.icns` file (already has `Info.plist` for app bundle)
- [ ] **10.3** Embed the icon in the Windows binary using `winres` or `embed-resource` crate in `build.rs`
- [ ] **10.4** Set the window icon at runtime via `eframe::NativeOptions::viewport` with `ViewportBuilder::with_icon()`
- [ ] **10.5** Update the macOS `.app` bundle to include the icon in `Info.plist` (`CFBundleIconFile`)
- [ ] **10.6** Add icon to the Linux `.desktop` file for proper desktop integration

---

## Implementation Order (Suggested)

```
Phase 1 - Foundation
  1. Modernize Codebase       [P0] - Clean base for new work
  2. Download Directory        [P0] - New path structure before adding more downloads
  5. Security Hardening        [P1] - Fix before adding network features

Phase 2 - Core Features
  4. Download Resume           [P1] - Needed before batch (large batches will fail without it)
  3. Batch Download            [P1] - Key productivity feature
  7. RPCS3 Integration         [P1] - The primary goal

Phase 3 - Polish
  6. PSVita Support            [P2] - Platform expansion
  8. Themes                    [P2] - Visual polish
  10. Application Icon         [P2] - Branding and desktop integration
  9. Additional Enhancements   [P3] - Nice-to-haves
```

---

## Reference: RPCS3 Directory Structure

The RPCS3 integration needs to understand these locations (all auto-detected or user-configurable, never hardcoded):

### Key Files & Directories
- **`games.yml`**: Maps serial -> game ISO/folder path (e.g., `BCUS98111: /path/to/God of War III (USA).iso`)
- **`vfs.yml`**: Virtual filesystem mappings, defines `$(EmulatorDir)` and where `dev_hdd0` lives
- **`dev_hdd0/game/{SERIAL}/`**: Installed game data & updates (contains `PARAM.SFO`, `USRDIR/`, etc.)
- **`PARAM.SFO`**: Binary metadata per game - `TITLE`, `TITLE_ID`, `APP_VER`, `VERSION`
- **`patches/patch.yml`**: Game patches (FPS unlocks, fixes, etc.)

### Common RPCS3 Config Paths (for auto-detection)
- Linux native: `~/.config/rpcs3/`
- Linux AppImage: `<AppImage>.home/.config/rpcs3/`
- Windows: `%APPDATA%/rpcs3/` or alongside `rpcs3.exe`
- macOS: `~/Library/Application Support/rpcs3/`

### Download Directory Structure (configurable)
```
{download_path}/
  {Game Title}/
    {Serial}/
      UP9000-BCUS98111_00-PATCH00000000000-A0103-V0100-PE.pkg
```

All paths are user-configurable in the app settings. The app should auto-detect where possible but always allow manual override.
