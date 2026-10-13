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
| `egui` | Yes | `rfd`, `eframe`, `egui-notify`, `notify-rust` | Desktop GUI application |
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
Create/open .pkg file (preserves existing content via OpenOptions::create(true))
    |
    v
Hash existing file -> if matches expected SHA1, skip download (already complete)
    |
    v
Truncate file, stream HTTP response chunks, write to disk
    |
    v
Hash downloaded file -> verify SHA1
    |
    v
DownloadStatus::DownloadSuccess or DownloadError::HashMismatch
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
  - `MergeError` - merge failures (filepath issues, unmergable packages)
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
| Enums | PascalCase | `DownloadStatus`, `PlaformVariant` |
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
- Metadata responses limited to `MAX_METADATA_SIZE` (1 MB) — both Content-Length and body size checked
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
  |     |-- show_toasts: bool
  |     |-- show_notifications: bool
  |     |-- directory_format: DirectoryFormat
  |     |-- max_concurrent_downloads: u32
  |     |-- rpcs3_config_dir: Option<PathBuf>
  |     |-- theme: AppTheme (Dark/Light/PlayStation)
  |
  |-- VolatileData (not persisted)
        |-- serial_query, update_results
        |-- download_queue: Vec<ActiveDownload>
        |-- pending_downloads: VecDeque<PendingDownload>
        |-- paused_downloads: Vec<PausedDownload>
        |-- merge_queue: Vec<ActiveMerge>
        |-- completed/failed download/merge lists
        |-- search_promise, batch_search_queue/active
        |-- rpcs3_scan_promise, rpcs3_installed_games
        |-- clipboard, toasts, egui_ctx
```

### 6.2 State Management

- **Persisted state** (`#[derive(Serialize, Deserialize)]`): `AppSettings` saved via `eframe::set_value` / `eframe::get_value` between sessions
- **Volatile state**: `VolatileData` marked `#[serde(skip)]`, reset each launch
- Settings modification uses a dirty-flag pattern: `settings_dirty` tracks unsaved changes, `modified_settings` holds the working copy

### 6.3 UI Layout

- **Menu bar**: File (Settings, Quit), Tools (Import Batch, Scan RPCS3, Migrate Downloads), Help (About)
- **Search bar**: Serial input, search button, settings gear, Import Batch, Scan RPCS3, Download All, Clear
- **Results list**: Vertical scrollable area with collapsible headers per title (aggregate status in header)
- **Package entries**: Grouped panels with version, size, SHA1, download/pause/abort/resume buttons, progress bar, Copy URL
- **Settings window**: Fixed-size modal (`420x560`) with Downloads, Appearance (themes), Notifications, RPCS3 Integration
- **Status bar**: Bottom panel with batch progress, download counts, speed (EMA), ETA, aggregate progress bar
- **RPCS3 Library window**: Grid showing serial, title, installed/available versions, update status
- **Hash mismatch warning**: Fixed-size modal (`550x100`) explaining Sony server reliability issues

### 6.4 Font Embedding

Fonts are embedded at compile time via `include_bytes!()`:
- `NotoSans-Regular.ttf` - Primary Latin font
- `NotoSansJP-Regular.otf` - Japanese fallback font

Both are inserted into egui's `Proportional` font family, with Noto Sans as priority 0 and Noto Sans JP as priority 1.

### 6.5 Notification System

Two parallel notification systems:
1. **In-app toasts** (`egui-notify`): Anchored bottom-right, 10-second duration, reversible order
2. **System notifications** (`notify-rust`): OS-level desktop notifications

Both are independently toggleable in settings.

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

One of `--titles`, `--batch-file`, or `--scan-rpcs3` must be provided.

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

| Test | Serial | Platform | Expected |
|---|---|---|---|
| `parse_ac3` | NPUB30826 | PS3 | 1 package |
| `parse_lpb` | BCUS98148 | PS3 | 13 packages |
| `parse_infamous2` | NPUA80638 | PS3 | 3 packages |
| `parse_tokyo_jungle` | NPUA80523 | PS3 | 1 package |
| `parse_vita_hustle_kings` | PCSA00007 | PSVita | >= 1 package |
| `parse_vita_killzone` | PCSA00107 | PSVita | >= 1 package (or NoUpdatesAvailable) |

**Unit tests** across modules:

| Module | Tests | Coverage |
|---|---|---|
| `utils` | 12 | Title sanitization (colons, paths, Unicode, Windows reserved names, length) |
| `security` | 7 | URL validation, serial validation, path containment |
| `batch` | 7 | Batch file parsing, comments, normalization, dedup, errors |
| `rpcs3/sfo` | 4 | SFO binary parsing, invalid files |
| `rpcs3/config` | 4 | games.yml, vfs.yml parsing |
| `rpcs3` | 8 | Version comparison, serial detection, title extraction |

### 10.2 Test Characteristics

- Integration tests are `#[tokio::test]` async tests requiring network access
- Unit tests are standard `#[test]` and use `tempfile` for fixtures
- Tests are gated with `#[cfg(test)]` (compiles only in test builds)
- No mocking framework is used — live server tests verify real Sony API responses
- Total: 48 tests

---

## 11. Known Technical Debt & Quirks

1. **`serde_yaml` deprecated**: The `serde_yaml 0.9` crate is archived by its maintainer. Consider migrating to `serde_yml` (maintained fork with compatible API).
2. **reqwest Client per-request**: A new `reqwest::Client` is built for each `get_info()` and `start_download()` call. The client should be constructed once and reused (avoids redundant TLS handshakes and connection pool waste).
3. **`blocking_lock()` in UI thread**: `tokio::sync::Mutex::blocking_lock()` is called in the egui rendering path (status bar, progress display). This can block the UI if an async task holds the lock. Consider `try_lock()` or `std::sync::Mutex` for shared progress state.
4. **egui frame cloning**: `update_results` and `rpcs3_installed_games` are cloned every frame in `draw_results_list` / `draw_rpcs3_library_window` due to borrow checker constraints. Could be optimized with index-based iteration or `Arc<Vec<T>>`.
5. **Busy-wait polling in CLI**: The CLI download loop calls `promise.ready()` in a tight loop with a 50ms sleep. Could use a proper event-based approach.
6. **Newline sanitization**: Special-case handling for BCUS98233 title containing `\n` characters (replaced in `psn/mod.rs` after parsing).

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
