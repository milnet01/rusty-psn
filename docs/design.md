# rusty-psn — Design

> **Purpose — so the shape is decided once, and anyone can tell where a
> new piece of work belongs and what it is allowed to touch.**

**This document is a gate.** Work is not broken into items until it is
agreed — `~/.claude/workflow.md` § 4 says when it passes.

**Status:** agreed (2026-10-12).
**Genre:** adr

This records the shape the code already has, plus the rules the owner set
from 2026-10-12 on. The signs it places are in `docs/discovery.md`.

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
| Promise | Lets a front end's frame or loop poll a background task's result without waiting | `src/promise.rs` |

## What may depend on what

- **Entry** may use the front ends, and hands them its parsed arguments
  (`Args`). A front end may use `Args` and nothing else of Entry. No other
  part uses Entry.
- **The front ends** (Window app, Command line) may use every part below
  them. **No other part may use a front end.**
- **The front ends never use each other.**
- **PSN** may use Security and Files.
- **RPCS3** may use Security and Files. **It must never use PSN
  directly**: RPCS3 support has to work on packages that came from anywhere.
- **Content** may use RPCS3.
- **Batch** may use PSN and Security.
- **Security uses no other part.** It is the base every check rests on.
- **Files** may use Security.
- **Promise uses no other part.**

**No two-way dependency may be added.** Files has its own error type
(`FileError`), which PSN converts into its `DownloadError`.

**Frontend-only code.** PSN and Files already hold some code compiled only
for the window app (`#[cfg(feature = "egui")]`). That code may stay, and a
fix to it stays where it is. **New code used only by one front end goes in
that front end's own folder.** Content is the exception: it is a part of its
own, so new Content code goes in `src/content.rs`, although today only the
window app calls it.

**New screens.** **A new screen or panel goes in its own file under
`src/egui/`**, the default `~/.claude/standards/coding.md` § 1.8
sets for a second reason to change.

## Where each sign of success lives

| Sign | Part that delivers it |
|---|---|
| S1 — every listed update downloads in full and matches Sony's checksum | PSN, with Files for hashing |
| S2 — a dropped connection resumes and still ends verified | PSN |
| S3 — a multi-part PS4 update ends up as one package | PSN |
| S4 — downloads land only in the chosen folder, Title / ID / files by default | Files, with Security for containment. Both apps default to Title / ID, the layout `CLAUDE.md` rule 6 prefers |
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
  into a temporary folder beside the zip, or the system temp folder where
  that folder cannot be written, through the zip crate's `enclosed_name`. It
  hands RPCS3's `--installpkg` one folder holding the packages and their
  `.rap` and `.edat` licence files directly, with no subfolders: RPCS3 reads
  only the files at the top of a folder it is given. RPCS3 copies the
  licences into its own `exdata` folder and installs the packages
  (`InstallPackages` in `rpcs3qt/main_window.cpp` and `get_dir_entries` in
  `rpcs3qt/qt_utils.cpp`, RPCS3 commit 6fe3c87). rusty-psn writes nothing
  into RPCS3's folders. Content cannot see whether RPCS3 placed the
  licences: it reports success when `rpcs3::install_pkg` does.

- **Window layout** — the window app opens no floating window, dialog or
  pop-up notice. Every page, panel and message has its own fixed place in
  the main window, and each panel can be folded away. Pages are chosen
  from a sidebar. Messages go to one Messages panel at the foot of the
  window, on every page; a status that belongs to one item, such as a
  failed download, also shows beside that item. Choosing a page, folding
  or unfolding, and the user changing the text size or theme are the only
  things that move another part. A status or a progress bar updates in its
  place without shifting anything around it. A list, such as the messages
  or the downloads, keeps one fixed box: a new row, or a download moving
  to its Finished section, changes only what is inside that box. Where the
  window cannot show everything, its starting and smallest sizes are made
  bigger, or a list scrolls inside its own fixed box; the app does not
  resize its window while it runs. Every message reaches the Messages
  panel whatever the settings say, so the "Show in-app toasts" setting
  goes; a settings file that still holds it loads with it ignored.
  Drop-down lists, hover hints and right-click menus stay: each opens only
  when the user acts on it, and closes without moving anything. The
  desktop's file and folder pickers are the exception: they belong to the
  system, not the app. So is the error box shown when the app cannot set
  up its network connection at startup: the main window never opens, and
  the app closes after it. So are system notifications, which the user turns
  on in Settings and which repeat a message the Messages panel shows. The
  owner set this on 2026-10-14 (RPSN-0157); the code follows it since
  2026-10-15.

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
| 7 | 2026-10-14 | 2 (both lanes held every question) | 1 | 0 | 0 | — | Converged: gate on the RPSN-0146 amendment (Content is a part of its own; new Content code goes in src/content.rs); no finding landed in it. 1 verified, 1 fixed, 0 dismissed, out of radius: both lanes found RPCS3 allowed Files only while src/rpcs3/sfo.rs calls security::strip_control; now Security and Files, fixed in-run as a record of existing code (rule 14 code-exists exception). Filed: src/promise.rs belongs to no part (RPSN-0155). Open questions resolved clean: the content browser marks a DLC installed on a clean exit, as RPSN-0137 § 3 chose; copy_pkg_file runs only inside merge_parts after containment; the default layout is SerialTitle (ID - Title); merge_parts' fixed layouts are in tests; RPCS3's get_dir_entries lists top-level files only. The project's check-doc.py ran degraded (no Genre: line), so paths and symbols were checked by hand. Second share: 0 of 1 verified findings landed in the gated span. |
| 8 | 2026-10-14 | 2 (both lanes held every question) | 0 | 0 | 2 | — | 2 verified, 2 fixed, 0 dismissed. Gate on the RPSN-0157 Window layout bullet. Both lanes found it never said what becomes of the saved "Show in-app toasts" setting: now it goes, and an older settings file loads with it ignored (run: a RON settings file with an unknown field loads). From a lane's open question: "the window is made bigger" left open whether the app resizes itself; now it opens bigger and does not resize while running. Open questions resolved clean: the File integrity check failed window only informs (its one button closes it); system notifications are off by default. Not checkable here: RPCS3 source at 6fe3c87. Surfaced to the owner: the system-notification exception is the author's, not the owner's words. |
| 9 | 2026-10-14 | 2 (both lanes held every question) | 0 | 0 | 3 | — | 3 verified, 3 fixed, 0 dismissed. One lane: changing the text size re-lays out every part, so the live preview needed naming; now the user changing text size or theme, and choosing a page, may move parts. Both lanes asked where a message's place is; with no spec planned that is a Q3, now one Messages panel at the foot of every page, plus a status beside its own item. One lane: eframe restores the last window size, so "opens bigger" became bigger starting and smallest sizes. Also corrected by narrowing: the intro's rule dates. The packet carried RPSN-0157's old "specced together" text; the roadmap had already been corrected. Not checkable here: RPCS3 source at 6fe3c87. A third loop is owed (ADR cap 3). |
| 10 | 2026-10-15 | 2 (both lanes held every question) | 0 | 1 | 2 | — | 3 verified, 3 fixed, 0 dismissed. Found while building the packet: the bullet forbade every dialog, but `src/main.rs` shows an error box when the network client cannot be built at startup, before any window exists; it is now an exception. One lane: whether a row moving inside a list counts as moving a part, which the planned Downloads sections and the Messages panel both decide; now a list keeps one fixed box and its rows change only inside it. One lane: drop-down lists, hover hints and right-click menus were neither allowed nor forbidden; now they stay. Measured: a RON settings string holding `show_toasts`, at the start, middle or end, loads into a struct without it, keeping every other value (ron 0.12.2, serde 1.0.229). Open questions resolved clean: the user resizing the window and eframe restoring a saved size build the same either way. Cap reached (3 for an ADR, loops 8 to 10); these 3 fixes were read by no lane. Calm cap: 1 of 3 final-loop findings landed on text this run wrote (loop 9's list of what may move). The gated span is the whole bullet, so the share inside it carries no information. Final sweep: no other copy of the changed wording in the project's Markdown. Surfaced to the owner: the list-box and drop-down rulings are the author's, from the approved mockups. |
