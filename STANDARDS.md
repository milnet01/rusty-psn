# rusty-psn Standards Document

## 1. Project Identity

| Field | Value |
|---|---|
| **Name** | rusty-psn |
| **Version** | 0.5.10 |
| **Language** | Rust (Edition 2021) |
| **License** | MIT (Copyright 2022 RainbowCookie32) |
| **Repository** | https://github.com/RainbowCookie32/rusty-psn |
| **Purpose** | Download PS3, PS4, & PSVita game update packages (.pkg) from Sony's PSN servers for use with emulators (primarily RPCS3) |

---

## 2. Architecture Overview

### 2.1 Module Structure

The source modules, what each is responsible for, and what each may depend
on are in `docs/design.md`.

```
resources/
  NotoSans-Regular.ttf  # Latin font (embedded at compile time)
  NotoSansJP-Regular.otf # Japanese font (embedded at compile time)
  Info.plist            # macOS app bundle metadata
  icon-*.png            # Application icons (48, 64, 128, 256)
  icon.svg              # Vector application icon
```

### 2.2 Feature Flags

The project uses Cargo feature flags to compile either frontend:

| Feature | Default | Dependencies | Purpose |
|---|---|---|---|
| `egui` | Yes | `rfd`, `eframe`, `notify-rust` | Desktop GUI application |
| `cli` | No | `crossterm` | Terminal-based interactive interface |

Build commands:
- GUI (default): `cargo build --release`
- CLI: `cargo build --release --no-default-features --features cli`

### 2.3 Dependency Inventory

`Cargo.toml` is the authority on which crates are used and their versions (`docs/design.md`). Every crate is kept at its latest release; `docs/dependency-holds.md` lists any held back and why.

---

## 3. Domain Model

### 3.1 Platform Variants

```
PlatformVariant::PS3    - Serial prefixes: NP*, BL*, BC*
PlatformVariant::PS4    - Serial prefix: CUSA*
PlatformVariant::PSVita - Serial prefixes: PCS*, VC*, VL*
```

### 3.2 Title Serial Format

- Raw input: `BCES-01234` or `BCES01234` or `bces01234`
- Normalized: `BCES01234` (trimmed, uppercase, dashes removed)
- Normalization performed by `psn::parse_title_id()`

### 3.3 Update Discovery Flow

```
User enters serial
    |
    v
parse_title_id() -> normalize serial
    |
    v
get_platform_variant() -> PS3 or PS4 (or invalid)
    |
    v
get_update_info_url() -> construct API URL
    |                     PS3:    https://a0.ww.np.dl.playstation.net/tpl/np/{id}/{id}-ver.xml
    |                     PS4:    https://gs-sec.ww.np.dl.playstation.net/plo/np/{id}/{hmac}/{id}-ver.xml
    |                     PSVita: https://gs-sec.ww.np.dl.playstation.net/pl/np/{id}/{hmac}/{id}-ver.xml
    v
HTTP GET (certificate check skipped for Sony hosts only)
    |
    v
parser::parse_response() -> parse XML into UpdateInfo + Vec<PackageInfo>
    |
    v
[PS4 only] For each package with manifest_url:
    |   HTTP GET manifest_url
    |   manifest_parser::parse_manifest_response() -> expand into part packages
    |
    v
UpdateInfo ready for download
```

### 3.4 Download Flow

```
PackageInfo::start_download()
    |
    v
Validate the package URL (security::validate_download_url)
    |
    v
HEAD request -> final file name and ETag
    |
    v
Create/open .pkg file (existing content kept)
    |
    v
Hash existing file -> if it matches the expected SHA1, done (already complete)
    |
    v
GET; a file shorter than expected asks for the rest (Range, If-Range: ETag)
An error status stops here and leaves the file untouched
    |
    v
206 from the resume point -> append; 206 from anywhere else -> GET the whole file;
200 -> truncate and write from the start
    |
    v
Stream chunks to disk, checking for pause or abort before each one
    |
    v
Hash downloaded file -> verify SHA1
    |
    v
DownloadStatus::DownloadSuccess, or DownloadError::HashMismatch:
a short file is kept for resume, a whole one is deleted
```

### 3.5 SHA1 Verification

- **PS3 packages**: The last `0x20` bytes of the `.pkg` file contain a SHA1 hash suffix; the hash is computed over all bytes *except* that suffix
- **PS4 packages**: `hash_whole_file = true`; the hash covers the entire file
- Hash comparison is done as lowercase hex string equality

### 3.6 PS4 Multi-Part Merge

PS4 updates can be split into multiple "pieces" described in a JSON manifest. After all parts are downloaded:

1. Parts are sorted by `part_number`
2. Each part is written to a merged output file at its designated `offset`
3. Merged file name strips the `_{index}.pkg` suffix, becoming just `.pkg`
4. Uses 128 MB buffered I/O (`MERGE_CHUNK_SIZE = 1024 * 1024 * 128`)

### 3.7 Key Data Structures

```rust
UpdateInfo {
    title_id: String,          // e.g., "BCUS98148"
    tag_name: String,          // from XML <tag name="...">
    titles: Vec<String>,       // game title in multiple languages
    packages: Vec<PackageInfo>,
    platform_variant: PlatformVariant,
}

PackageInfo {
    url: String,               // direct .pkg download URL
    size: u64,                 // expected file size in bytes
    version: String,           // update version string
    sha1sum: String,           // expected SHA1 hash (hex)
    hash_whole_file: bool,     // false for PS3 (skip last 0x20), true for PS4
    manifest_url: String,      // PS4 manifest JSON URL (empty for PS3)
    offset: u64,               // byte offset for multi-part merge
    part_number: Option<usize>, // Some(n) for PS4 multi-part, None otherwise
}
```

---

## 4. Coding Standards

### 4.1 Formatting

- **Formatter**: `rustfmt` (enforced via CI on pull requests using nightly toolchain)
- **Max line width**: 130 characters (`.rustfmt.toml`: `max_width = 130`)
- CI uses `mbrobbel/rustfmt-check` in review mode, which posts formatting suggestions as PR review comments

### 4.2 Rust Edition & Idioms

- **Edition**: 2021
- **Macro imports**: Uses explicit `use log::{info, warn, error};` (standard 2021-edition style)
- **Conditional compilation**: Extensive use of `#[cfg(feature = "...")]` and `#[cfg(target_os = "...")]` for platform/feature gating

### 4.3 Error Handling

- Custom error enums per domain area, wrapping underlying errors:
  - `UpdateError` - update query failures (invalid serial, network, parsing)
  - `DownloadError` - download failures (hash mismatch, a firmware file short of its stated size, I/O, network)
  - `utils::FileError` - Files failures (I/O, path containment, a file too short to verify); PSN converts it into `DownloadError`
  - `MergeError` - merge failures (filepath issues, unmergeable packages)
  - `parser::ParseError` - XML parsing errors
  - `manifest_parser::ParseError` - JSON parsing errors
- Errors are propagated with `map_err()` to wrap library errors in domain-specific variants
- No use of `anyhow` or `thiserror` - all error types are hand-rolled
- Pattern: `map_err(DomainError::WrappedVariant)?` for error conversion

### 4.4 Async Patterns

- **Runtime**: Tokio with minimal features (`rt`, `io-util`, `macros`, `fs`)
- **UI bridging**: `Promise` in `src/promise.rs` bridges async tasks to synchronous egui/CLI loops
- **Channel communication**: `tokio::sync::mpsc` channels relay download/merge progress from async tasks to UI
- **egui runtime isolation**: Tokio runtime runs in a dedicated thread to prevent egui from blocking async I/O when the window is minimized/suspended
- **Shared state**: `Arc<Mutex<T>>` (tokio Mutex) for progress status shared between async download tasks and UI rendering

### 4.5 Naming Conventions

| Element | Convention | Example |
|---|---|---|
| Structs | PascalCase | `UpdateInfo`, `PackageInfo`, `ActiveDownload` |
| Enums | PascalCase | `DownloadStatus`, `PlatformVariant` |
| Enum variants | PascalCase | `DownloadStatus::Progress(u64)` |
| Functions | snake_case | `get_update_info_url()`, `parse_title_id()` |
| Constants | SCREAMING_SNAKE | `CHUNK_SIZE`, `MERGE_CHUNK_SIZE`, `INVALID_CHARS` |
| Modules | snake_case | `manifest_parser`, `psn` |
| Feature flags | lowercase | `egui`, `cli` |

### 4.6 Logging

- **Facade**: `log` crate macros (`info!`, `warn!`, `error!`)
- **Backend**: `flexi_logger` writing to timestamped `.log` files
- **Log level**: `info` (configurable via `Logger::try_with_str`)
- **Stdout duplication**: Only errors are duplicated to stdout (`Duplicate::Error`)
- **Opt-out**: `--no-log-file` CLI flag disables file logging
- **macOS**: Logs written to `dirs::data_local_dir()/rusty-psn/`
- **Other platforms**: Logs written to current working directory
- Log file naming: `rusty-psn_YYYY-MM-DD_HH-MM-SS.log`

### 4.7 File & Path Handling

- Paths are sanitized via `sanitize_title()` which replaces OS-invalid characters with underscores
  - Windows: `< > : " / \ | ? *`
  - Unix: `/`
- Download directory structure: `{download_path}/{serial} - {sanitized_title}/{package_filename}.pkg`
- Legacy path migration: code checks for old format `{download_path}/{serial}/` and renames to new format
- File creation uses `OpenOptions::create(true)` to preserve existing file contents (supports resume-like behavior)

---

## 5. Security Considerations

### 5.1 TLS Certificate Handling

Sony's PSN update servers present certificates that fail normal validation: the update hosts chain to Sony's private "SCEI DNAS Root 05", signed with SHA-1, and `gs2.ww.prod.dl.playstation.net` serves a certificate for another name. Skipping the check is **intentional** and **required** for those hosts.

It is scoped: `security::sony_tls_config()` skips certificate verification only for hosts under `security::ALLOWED_DOMAINS` and verifies every other host normally. The handshake signature is checked on every host. `build_psn_client()` uses this config for every request.

### 5.2 Cryptographic Keys

Two HMAC-SHA256 keys are **hardcoded** in `psn/utils.rs`:

- **PS4**: `AD62E37F905E06BC19593142281C112CEC0E7EC3E97EFDCAEFCDBAAFA6378D84`
- **PSVita**: `E5E278AA1EE34082A088279C83F9BBC806821C52F2AB5D2B4ABD995450355114`

These are well-known public keys used across PS4/PSVita homebrew/emulation tooling for generating update API URLs. They are not secrets.

### 5.3 Download Integrity

- All downloads are verified against SHA1 hashes provided by Sony's API
- Hash mismatch detection includes awareness of short transfers (Sony's servers are known to drop connections early)
- Files are not trusted until hash verification passes
- Download resume uses HTTP Range + If-Range (ETag) for safe partial file reassembly

### 5.4 URL and Domain Validation

- All download and metadata URLs are validated against `ALLOWED_DOMAINS` (`.dl.playstation.net`, `.playstation.net`)
- Redirect policy validates each hop stays within allowed domains
- Only `http` and `https` schemes are permitted

### 5.5 Input Validation

- Serial format validated via `security::validate_serial()` (9 alphanumeric chars, known prefixes)
- Batch file size limited to 1 MB, YAML config size limited to 10 MB, SFO files to 1 MB
- Metadata responses limited to `MAX_METADATA_SIZE`, the firmware list to `MAX_FIRMWARE_LIST_SIZE` — `psn::utils::read_capped_text` checks Content-Length, then counts the body as it arrives and stops past the cap
- Download paths validated for containment within the configured base directory

### 5.6 Path Containment

- `security::validate_path_containment()` checks that download targets stay within the base directory
- Uses `fs::canonicalize()` when the base directory exists (resolves symlinks and `..`)
- Falls back to lexical normalization + component-aware `Path::starts_with()` when the base doesn't exist yet
- Game titles sanitized by `utils::sanitize_title()`: strips null bytes, path traversal sequences, OS-invalid chars, reserved names

---

## 6. GUI (egui) Standards

### 6.1 Application Structure

```
UpdatesApp (eframe::App)
  |-- AppSettings (persisted via eframe storage)
  |     |-- pkg_download_path: PathBuf
  |     |-- show_notifications: bool
  |     |-- directory_format: DirectoryFormat
  |     |-- max_concurrent_downloads: u32
  |     |-- rpcs3_config_dir: Option<PathBuf>
  |     |-- theme: AppTheme (Dark/Light/PlayStation/Rpcs3Blue/HighContrast)
  |     |-- text_size_percent: u32 (100 to 200; scales all text and controls)
  |
  |-- VolatileData (not persisted)
        |-- serial_query, update_results
        |-- download_queue: Vec<ActiveDownload>
        |-- pending_downloads: VecDeque<PendingDownload>
        |-- paused_downloads: Vec<PausedDownload>
        |-- merge_queue: Vec<ActiveMerge>
        |-- completed/failed download/merge lists
        |-- search_promises (several at once), batch_search_queue/active
        |-- rpcs3_scan_promise, rpcs3_installed_games
        |-- clipboard, message_log, egui_ctx
```

### 6.2 State Management

- **Persisted state** (`#[derive(Serialize, Deserialize)]`): `AppSettings` saved via `eframe::set_value` / `eframe::get_value` between sessions. The History page's entries are stored beside them under `download_history`, newest 500 kept (`src/egui/history.rs`)
- **Window size and position**: the window starts at 1320 x 860 and cannot be made smaller than 1320 x 700, wide enough at 100% text size for the sidebar and a full status bar (`src/main.rs`). eframe saves the size, and the position where the system reports one. Wayland reports none, so on KDE Plasma `src/egui/placement.rs` asks KWin for it at close and stores it under its own key, `kwin_frame_position` (RPSN-0156)
- **Volatile state**: `VolatileData` marked `#[serde(skip)]`, reset each launch
- Settings modification uses a dirty-flag pattern: `settings_dirty` tracks unsaved changes, `modified_settings` holds the working copy

### 6.3 UI Layout

- **Sidebar** (`src/egui/chrome.rs`): a fixed left panel choosing the page shown in the central area: Search, Downloads, RPCS3 library, Patches and DLC, Firmware and History at the top, Settings at the foot. `docs/design.md` § "Window layout" is the rule it follows (RPSN-0157). Ctrl+F goes to Search and focuses the serial box; Escape goes to Search; Ctrl+S opens Settings, or saves on the Settings page
- **Search page**: serial input and search button; below it Import Batch, Download All, Download All Latest, Clear results and Open Downloads, wrapping at a large text size
- **Downloads page** (`src/egui/downloads_page.rs`): foldable sections Needs attention (failed), In progress (downloading, paused, queued) and Finished (newest first), each row the same package row the results list draws, with its buttons. Needs attention has a Retry all failed button
- **Results list**: Vertical scrollable area with collapsible headers per title (aggregate status in header)
- **Package entries**: Grouped panels with version, size, SHA1, download/pause/abort/resume buttons, progress bar, Copy URL
- **Settings page**: Downloads, Appearance (theme, text size), Notifications, RPCS3 Integration and Content Library, then folding sections for Migrate downloads and About. The fields scroll above a fixed bar: Save changes and Undo changes (each enabled only while something is unsaved), Restore defaults (fills in the defaults as unsaved changes), and words saying "You have unsaved changes" or "All changes saved". Leaving the page drops unsaved changes
- **Status bar**: Bottom panel with fixed-width slots, so a changing value never moves the next: download counts (or "Ready"), total size, speed (EMA), ETA, batch progress, then the aggregate progress bar. Each slot is as wide as the longest text it expects (`status_slot` in `src/egui/chrome.rs`)
- **RPCS3 library page**: a Scan button, filter buttons (All, Needs update, Up to date, No updates exist), then a grid showing serial, title, installed/available versions, update status. `library_status` decides each row's status for both the column and the filters; a failed search is not "No updates exist"
- **Messages panel** (`src/egui/messages.rs`): above the status bar on every page; folds to one button. Unfolded, it lists the newest 200 messages in a fixed-height box that scrolls, each with a status icon, a word and a colour. A download that failed because the file arrived short also says "Failed: file incomplete" beside its package

### 6.4 Font Embedding

Fonts are embedded at compile time via `include_bytes!()`:
- `NotoSans-Regular.ttf` - Primary Latin font
- `NotoSansJP-Regular.otf` - Japanese fallback font

Both are inserted into egui's `Proportional` font family, with Noto Sans as priority 0 and Noto Sans JP as priority 1.

### 6.5 Notification System

Every message goes to the Messages panel (§ 6.3); nothing pops up inside the window. System notifications (`notify-rust`) repeat a message on the desktop when the user turns them on in Settings; they are off by default.

---

## 7. CLI Standards

### 7.1 Arguments

| Argument | Short | Required | Description |
|---|---|---|---|
| `--titles` | `-t` | Conditional | Space-separated serials in quotes; may be given more than once |
| `--batch-file` | `-b` | Conditional | Path to text file with serials (one per line) |
| `--scan-rpcs3` | - | Conditional | Scan RPCS3 library for available updates |
| `--silent` | `-s` | No | Download all without prompts, print only errors |
| `--destination-path` | `-d` | No | Target download folder (default: `pkgs/`) |
| `--directory-format` | - | No | Directory format: serial-only, serial-title, title-serial |
| `--rpcs3-config-dir` | - | No | Path to RPCS3 config directory (containing games.yml) |
| `--no-log-file` | - | No | Disable log file output |

Exactly one of `--titles`, `--batch-file` or `--scan-rpcs3` must be provided, or `--firmware`
instead of all three. Clap refuses two of them together, and an unknown `--directory-format` value.

### 7.2 Interactive Flow

1. Query all serials concurrently via `Promise::spawn_async`
2. Display results: platform, serial, title, update count, total size
3. List individual packages with index numbers
4. Prompt user to select packages by index (space-separated), or press Enter for all
5. Download selected packages with live progress display via crossterm cursor manipulation
6. 3-second pause between title groups

### 7.3 Exit Code

The command line exits with 1 when any search, download, install or firmware download it was asked for failed, and 0 otherwise.
A serial with no updates is not a failure, and nor is a prompt left unanswered. A bad selection typed at a prompt is.

---

## 8. CI/CD Pipeline

### 8.1 Build Matrix

Triggered on every push and pull request:

| OS | Features Built |
|---|---|
| ubuntu-22.04 | `cli`, `egui` |
| windows-latest | `cli`, `egui` |
| macos-latest | `cli`, `egui` (universal binary: arm64 + x86_64) |

### 8.2 Artifacts

| Artifact Name | Contents |
|---|---|
| `rusty-psn-egui-linux` | Linux GUI binary |
| `rusty-psn-cli-linux` | Linux CLI binary |
| `rusty-psn-egui-windows` | Windows GUI `.exe` |
| `rusty-psn-cli-windows` | Windows CLI `.exe` |
| `rusty-psn-egui-macos-universal` | macOS `.app` bundle (universal) |
| `rusty-psn-cli-macos-universal` | macOS CLI binary (universal) |

### 8.3 Release Process

- Automatic release creation on push to `master` branch
- Version derived from `Cargo.toml` via `dante-signal31/rust-app-version`
- Release tag format: `v{version}` (e.g., `v0.5.10`)
- Release notes auto-generated by GitHub
- Old build artifacts cleaned up after 2 days (keeping most recent 2)

### 8.4 Formatting Check

- Runs on pull requests only
- Uses nightly `rustfmt`
- Posts review comments for formatting violations

### 8.5 Dependency Management

- Dependabot configured for weekly Cargo dependency updates

---

## 9. Docker Support

CLI-only Docker image based on `amd64/ubuntu:jammy`:

```dockerfile
FROM amd64/ubuntu:jammy
# Downloads latest CLI release binary from GitHub
# Entrypoint: /rusty-psn/rusty-psn
```

Usage:
```sh
docker build . -t rusty-psn
docker run --rm -v ${PWD}/pkgs:/rusty-psn/pkgs rusty-psn -t "SERIAL1 SERIAL2"
```

---

## 10. Testing

### 10.1 Existing Tests

**Integration tests** (psn/mod.rs) that hit Sony's live servers:

| Test | Serial | Platform |
|---|---|---|
| `parse_ac3` | NPUB30826 | PS3 |
| `parse_lpb` | BCUS98148 | PS3 |
| `parse_infamous2` | NPUA80638 | PS3 |
| `parse_tokyo_jungle` | NPUA80523 | PS3 |
| `parse_vita_hustle_kings` | PCSA00007 | PSVita |
| `parse_vita_killzone` | PCSA00107 | PSVita |

**Unit tests** sit in a `#[cfg(test)] mod tests` beside the code they cover.
`cargo test --bin rusty-psn -- --list` lists every test in the window-app build;
add `--no-default-features --features cli` for the command-line build.

### 10.2 Test Characteristics

- Integration tests are `#[tokio::test]` async tests requiring network access
- Unit tests are `#[test]`, or `#[tokio::test]` where they await, and use `tempfile` for fixtures
- Tests are gated with `#[cfg(test)]` (compiles only in test builds)
- No mocking framework is used — live server tests verify real Sony API responses

---

## 11. Known Technical Debt & Quirks

1. **Busy-wait polling in CLI**: The CLI download loop calls `promise.ready()` in a tight loop with a 50ms sleep. Could use a proper event-based approach.
2. **Newline sanitization**: Special-case handling for BCUS98233 title containing `\n` characters (replaced in `psn/mod.rs` after parsing).

---

## 12. Platform-Specific Behavior

| Behavior | macOS | Linux | Windows |
|---|---|---|---|
| Default download path | `~/Downloads/rusty-psn/pkgs/` | `pkgs/` (relative) | `pkgs/` (relative) |
| Log file location | `~/Library/Application Support/rusty-psn/` | Current directory | Current directory |
| Console window (release) | Hidden (egui) | N/A | Hidden via `windows_subsystem = "windows"` |
| App bundle | `.app` with `Info.plist` | Raw binary | Raw `.exe` |
| Universal binary | arm64 + x86_64 via `lipo` | amd64 only | x86_64 only |
| Path sanitization | Strip `/` | Strip `/` | Strip `< > : " / \ | ? *` |
