# rusty-psn — Design

> **Purpose — so the shape is decided once, and anyone can tell where a
> new piece of work belongs and what it is allowed to touch.**

**This document is a gate.** Work is not broken into items until it is
agreed — `~/.claude/workflow.md` § 4 says when it passes.

**Status:** agreed (2026-10-12).

This records the shape the code already has, plus the rules the owner set
on 2026-10-12 and 2026-10-13. The signs it places are in `docs/discovery.md`.

## The parts

| Part | Responsible for | Files |
|---|---|---|
| Entry | Parses arguments, starts logging, hands over to each front end the build includes | `src/main.rs` |
| Window app | The desktop interface: search, downloads, settings, RPCS3 library, content | `src/egui/`: one file per screen or concern; `mod.rs` holds the shared state and the frame loop |
| Command line | The terminal interface, including batch and silent runs | `src/cli/mod.rs` |
| PSN | Talking to Sony: building update URLs, parsing replies, downloading, resuming, verifying, merging multi-part PS4 updates, firmware | `src/psn/mod.rs`, `src/psn/parser.rs`, `src/psn/manifest_parser.rs`, `src/psn/utils.rs`, `src/psn/firmware.rs` |
| RPCS3 | Finding RPCS3, reading its config and game metadata, comparing versions, installing packages | `src/rpcs3/mod.rs`, `src/rpcs3/auto_detect.rs`, `src/rpcs3/config.rs`, `src/rpcs3/sfo.rs` |
| Content | Scanning a local folder of patches and DLC, and installing DLC zips the user already has | `src/content.rs` |
| Batch | Reading a list of game IDs from a text file | `src/batch.rs` |
| Security | Allowed Sony hosts, path containment, game ID validation, redirect policy | `src/security.rs` |
| Files | Download paths and folder layouts, creating package files, hashing, migrating old layouts | `src/utils.rs` |

## What may depend on what

- **Entry** may use the front ends, and hands them its parsed arguments
  (`Args`). A front end may use `Args` and nothing else of Entry. No other
  part uses Entry.
- **The front ends** (Window app, Command line) may use every part below
  them. **No other part may use a front end.**
- **The front ends never use each other.**
- **PSN** may use Security and Files.
- **RPCS3** may use Files. **It must never use PSN directly**: RPCS3 support
  has to work on packages that came from anywhere.
- **Content** may use RPCS3.
- **Batch** may use PSN and Security.
- **Security uses no other part.** It is the base every check rests on.
- **Files** may use Security.

**No two-way dependency may be added.** Files has its own error type
(`FileError`), which PSN converts into its `DownloadError`.

**Frontend-only code.** PSN and Files already hold some code compiled only
for the window app (`#[cfg(feature = "egui")]`). That code may stay, and a
fix to it stays where it is. **New code used only by one front end goes in
that front end's own folder.**

**New screens.** **A new screen or panel goes in its own file under
`src/egui/`**, the default `~/.claude/standards/coding.md` § 1.8
sets for a second reason to change.

## Where each sign of success lives

| Sign | Part that delivers it |
|---|---|
| S1 — every listed update downloads in full and matches Sony's checksum | PSN, with Files for hashing |
| S2 — a dropped connection resumes and still ends verified | PSN |
| S3 — a multi-part PS4 update ends up as one package | PSN |
| S4 — downloads land only in the chosen folder, Title / ID / files by default | Files, with Security for containment. Both apps still default to ID - Title; a roadmap item makes Title / ID the default, the layout `CLAUDE.md` rule 6 prefers |
| S5 — PS3, PS4 and PSVita IDs each return their updates | PSN |
| S6 — a downloaded update installs into RPCS3 and shows the new version | RPCS3 |
| S7 — a library scan marks every game correctly | RPCS3, with PSN for the latest version, joined in the window app |
| S8 — RPCS3 is found without typing a path, and can be overridden | RPCS3, with the front ends for the setting |
| S9 — both apps work on Linux, Windows and macOS | the whole |
| S10 — a DLC zip the user has installs into RPCS3 and the game sees it | Content, with RPCS3 for the install |

## What every part does the same way

- **Errors** — `STANDARDS.md` § 4.3 owns it: hand-written error enums,
  wrapped with `map_err`.
- **Logging** — `STANDARDS.md` § 4.6 owns it: the `log` macros, written to
  file by `flexi_logger`.
- **State and persistence** — only the window app keeps settings, through
  eframe's persistence. The command line takes everything from its
  arguments. The other parts keep no state between runs.
- **Network** — every request to Sony goes through the one client
  `psn::build_psn_client` builds. Every package, manifest or firmware URL
  taken from a Sony reply passes `security::validate_download_url` before
  it is fetched. Update-lookup URLs are built from fixed Sony hosts in
  `src/psn/utils.rs`. New network code follows the same rules.
- **Downloaded files** — every write into the download folder passes
  `security::validate_path_containment` first: a package file opened
  through `utils::create_pkg_file`, every firmware file, the merged PS4
  package written by `psn::merge_parts`, and the layout migrations in
  `utils::migrate_existing_dir` and `utils::migrate_directories`.
  `CLAUDE.md` rule 1 requires the check.
- **Writes outside the download folder** — Content unpacks a DLC zip only
  into a temporary folder, through the zip crate's `enclosed_name`. It
  hands RPCS3's `--installpkg` one folder holding the packages and their
  `.rap` and `.edat` licence files directly, with no subfolders: RPCS3 reads
  only the files at the top of a folder it is given. RPCS3 copies the
  licences into its own `exdata` folder and installs the packages
  (`InstallPackages` in `rpcs3qt/main_window.cpp` and `get_dir_entries` in
  `rpcs3qt/qt_utils.cpp`, RPCS3 commit 6fe3c87). rusty-psn writes nothing
  into RPCS3's folders. Content cannot see whether RPCS3 placed the
  licences: it reports success when `rpcs3::install_pkg` does.

## The stack, and what it rules out

- **Rust**, inherited from the upstream project. The upstream project
  recorded no runner-up.
- **egui with eframe** for the window app: one Rust codebase for Linux,
  Windows and macOS. It rules out a native look on each system.
- **tokio and reqwest (rustls)** for networking, so no system OpenSSL is
  needed.
- **Two front ends chosen at build time** by Cargo features (`egui`, `cli`).
  The default build is the window app.

`Cargo.toml` is the authority on which crates are used, and their versions.

## Close calls

None recorded as an ADR yet.

## Cold-eyes loop log

| Loop | Date | Lanes | Q1 | Q2 | Q3 | Q4 | Outcome |
|------|------|-------|----|----|----|----|---------|
| 1 | 2026-10-12 | 2 (both lanes held every question) | 3 | 2 | 0 | — | 5 verified, 5 fixed, 0 dismissed. Both lanes found Entry's `Args` used by the command line. Others: STANDARDS § 2.3 crate list disagrees with `Cargo.toml`; merged PS4 package skips path containment; a both-front-ends build exists; RPCS3's never-use-PSN rule needed "directly". The packet's import grep was lower-case only; corrected. Open questions resolved: the default folder layout is SerialTitle (a queue matter, not design); the window app does not use `Args`. |
| 2 | 2026-10-12 | 2 (both lanes held every question) | 2 | 2 | 1 | — | 5 verified, 5 fixed, 0 dismissed. Both lanes found S4's Title / ID layout is not the default, and RPCS3 and Content writes had no stated boundary. Also: layout migration writes before the containment check (`migrate_directories` found while verifying); egui-only code sits in PSN and Files, not RPCS3; a fix inside egui-only `merge_parts` had two possible homes. Open question resolved: `config.rs` writes only in tests. |
| 3 | 2026-10-12 | 2 (both lanes held every question) | 0 | 3 | 0 | — | 3 verified, 3 fixed, 0 dismissed. S4 narrowed in discovery to Title / ID by default, with the owner's agreement; S7 placed as RPCS3 with PSN, joined in the window app; the error summary no longer says one enum per area. Cap reached (3 for an ADR); these 3 fixes were read by no lane. Calm cap: 1 of 3 findings landed on text this run wrote (the S4 row). The gated span is the whole document, so the share of findings inside it carries no information. Final sweep: no stale wording found. Grouped and `super::` imports checked: all stay inside one part. |
| 4 | 2026-10-14 | 2 (both lanes held every question) | 1 | 0 | 1 | — | 2 verified, 2 fixed, 1 dismissed. Gate on the RPSN-0112 amendment to the writes-outside-the-download-folder bullet. Both lanes found the bullet read as current code, which still installs each package alone, copies `.rap` itself and drops `.edat`; and that RPCS3 lists only a folder's top-level files while extraction keeps subfolders. Dismissed: how Content learns a licence failed, which the implementer settles and RPSN-0137 confirms. Open questions resolved clean: RPCS3's `init_dirs` defaults on, so `exdata` exists at startup; firmware writes pass `validate_path_containment`. |
| 5 | 2026-10-14 | 2 (both lanes held every question) | 0 | 1 | 1 | — | 1 verified in radius, 1 fixed; 1 out of radius, filed as RPSN-0146. A lane found the bullet never said how Content learns a licence failed, now stated: it cannot, and reports what `rpcs3::install_pkg` reports. Filed: the window-only-code rule does not name Content, which only the window app calls. Open questions resolved clean: `InstallPackages` defaults `from_boot` to false, so licences are copied; `init_dirs` defaults on. |
| 6 | 2026-10-14 | 2 (both lanes held every question) | 0 | 0 | 0 | — | Converged: 0 verified, 1 dismissed. Dismissed: how to flatten nested zip entries and settle name clashes, which the implementer decides under the bullet's no-subfolders rule. Open questions left to the build: RPCS3's boot confirmation prompt in window mode, and whether a Flatpak RPCS3 can see the temporary folder (true of today's code too). Second share: 4 of 4 in-radius verified findings over the run landed in the amended bullet. |
