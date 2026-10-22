<!-- ants-spec-format: 1 -->
# RPSN-0137 — Install through headless RPCS3 and confirm what it installed

**Status:** accepted (2026-10-14).
**Kind:** fix.
**Source:** ROADMAP RPSN-0137 (rpcs3-source-review-2026-10-13 finding 2).

**Blocker for:** RPSN-0138, RPSN-0058.

**Layman:** Installing an update into RPCS3 no longer waits until you close
RPCS3, and rusty-psn checks the new version went in or says it could not.

## 1. Goal

An install started from rusty-psn finishes on its own, with no RPCS3 window.
An update is reported installed only when the game's version shows it went
in; where rusty-psn cannot look at an update, it says so instead of claiming
success. A DLC is reported as today (§ 3).

## 2. Problem

`rpcs3::install_pkg` runs `rpcs3 --installpkg <path>` and treats exit status 0
as success.

1. **The call blocks until the user closes RPCS3.** Without `--headless`, RPCS3
   opens its main window, starts the install there and keeps running
   (`main` in `rpcs3/rpcs3.cpp`, RPCS3 commit 6fe3c87, the `arg_installpkg`
   branch). So the window app shows "Installing…" and the command line waits
   until RPCS3 is closed by hand.
2. **Exit status 0 says nothing about the install.** In headless mode the same
   branch calls `main_window::InstallPackages(nullptr, …)` and then returns 0
   whatever happened. In window mode the exit code reports how RPCS3 was
   closed. **Amended 2026-10-22 (RPSN-0168):** headless RPCS3 0.0.43-20291
   and 0.0.43-20352 then crash at teardown ("Verification failed" in
   `stx::manual_typemap`'s destructor) and exit 143 after a successful
   install. Measured on private AppImage copies with NPUB30826 01.07: the
   log says "Successfully installed", APP_VER reads 01.07, the exit is 143.
   So the exit status carries nothing about the install, either way.
3. **Every caller trusts it.** The window app's Install button
   (`src/egui/results.rs`, handled by `handle_rpcs3_install_promises` in
   `src/egui/rpcs3_library.rs`), the content browser's patch Install
   (`src/egui/content_browser.rs`), the command line's `--install`
   (`src/cli/mod.rs`) and the DLC install (`content::install_dlc_zip`) all
   report success on `Ok(())`.

What headless mode does, read at RPCS3 commit 6fe3c87:

- `create_application` makes a `headless_application` (a `QCoreApplication`,
  no window) when `--headless` is on the command line.
- `InstallPackages` with no window takes a folder or a file list, copies
  `.rap` and `.edat` licences into `exdata`, and installs the `.pkg` files
  through `HandlePackageInstallation(nullptr, …)`, which shows no dialog.
- `HandlePackageInstallation` builds `game_compatibility compat(nullptr)` and
  calls `RequestCompatibility()`, whose `online` argument defaults to false
  (`rpcs3qt/game_compatibility.h`): it reads the local
  `compat_database.dat` only. **No network request is made.** The roadmap
  item had left this unverified.

## 3. Scope decisions (agreed with the user)

- **Headless rather than keeping RPCS3's window.** Made by this session under
  the owner's standing go-ahead of 2026-10-12; the roadmap item's remedy names
  headless. The window flow lets a user watch RPCS3 install, but the call
  cannot finish until RPCS3 closes, and its exit code still proves nothing.
  § 8 records the rejected option. The owner may reverse this.
- **A DLC install stays unconfirmed.** `docs/design.md` § Writes outside the
  download folder records that Content cannot see whether RPCS3 placed the
  licences and reports success when `rpcs3::install_pkg` does. This spec keeps
  that sentence true: a DLC install succeeds once RPCS3 has run, whatever its
  exit status (INV-8).
- **A firmware install follows INV-8 too.** Made by this session under the
  owner's standing go-ahead of 2026-10-12. `install_firmware` (RPSN-0141)
  shares `run_headless`, and headless `--installfw` also exits 143 after a
  successful install: measured 2026-10-22 on a private RPCS3 0.0.43-20291
  copy, which installed firmware 4.92. The owner may reverse this.

## 4. Design

### 4.1 Launch

`install_pkg` puts `--headless` before `--installpkg`:

```
<exe> --headless --installpkg <path>
flatpak run <app id> --headless --installpkg <path>
```

### 4.2 What the caller asks to be checked

```rust
/// What an install should leave behind, read once RPCS3 has exited.
pub enum InstallCheck {
    /// An update package. `dev_hdd0/game/<serial>/PARAM.SFO` must carry
    /// APP_VER equal to `version`; with `None`, any APP_VER other than the
    /// one there before the install.
    AppVersion { dev_hdd0: PathBuf, serial: String, version: Option<String> },
    /// Nothing rusty-psn can read back: a DLC, or no known dev_hdd0.
    Unconfirmed,
}

pub enum InstallOutcome {
    Confirmed,
    /// RPCS3 ran; nothing was checked.
    Unconfirmed,
}

pub async fn install_pkg(
    rpcs3_executable: &Path,
    pkg_path: &Path,
    check: InstallCheck,
) -> Result<InstallOutcome, Rpcs3Error>;
```

A new `Rpcs3Error::NotInstalled { serial: String, found: Option<String> }`
reports an install whose check failed; `found` is the APP_VER read after, or
`None` where there was no readable PARAM.SFO.

Order inside `install_pkg`:

1. Validate `serial` with `looks_like_serial`; refuse with `InstallFailed`
   before launching anything.
2. For `version: None`, read APP_VER before launching.
3. Launch (§ 4.1) and wait.
4. The exit status is logged and decides nothing (INV-8). The change is in
   `run_headless`, so `install_firmware` follows it (§ 3). A failure to
   launch the process is still `InstallFailed`: `run_headless` finding no
   executable, or the spawn itself returning an error. **Amended 2026-10-22
   (RPSN-0170):** finding no executable includes the checks behind
   `flatpak`, made before launch (INV-9).
5. Read APP_VER through `sfo::parse_sfo` and compare.

**Versions compare as trimmed text, not with `compare_versions`.** That
function reads an unparsable version as 0.0, so two unreadable versions
would compare equal and confirm an install that did not happen. Sony's
package `version` and PARAM.SFO's APP_VER share the `XX.YY` form.

### 4.3 Callers

| Caller | Check passed |
|---|---|
| Window app Install (`src/egui/results.rs`) | `AppVersion` with the package's `version`, `dev_hdd0` from `settings.rpcs3_config_dir` through `config::resolve_dev_hdd0`; `Unconfirmed` when that setting is empty |
| Command line `--install` | `AppVersion` with the package's `version`, `dev_hdd0` from `--rpcs3-config-dir`, else `auto_detect::detect_rpcs3` when `--rpcs3-executable` is not given; `Unconfirmed` otherwise |
| Content browser patch Install | `AppVersion` with `version: None` (`ContentPatch` carries no version), `dev_hdd0` as for the window app; `Unconfirmed` when `settings.rpcs3_config_dir` is empty |
| `content::install_dlc_zip` | `Unconfirmed` |
| "Update All Outdated" chain (`src/egui/update_chain.rs`) | `AppVersion` with each package's `version`, `dev_hdd0` as for the window app; `Unconfirmed` when `settings.rpcs3_config_dir` is empty. The chain stops at the first `Err`; an `Unconfirmed` step does not stop it |

### 4.4 What the user sees

- **Window app.** `Confirmed`: a success message and the "Installed in
  RPCS3" pill. `Unconfirmed`: a warning message saying RPCS3 finished
  but rusty-psn could not check the result until the RPCS3 folder is set in
  Settings; no pill, so the Install button stays. `Err`: an error message.
- **Command line.** `Confirmed`: "Installed successfully.". `Unconfirmed`: a
  line saying RPCS3 finished and the install was not checked, naming
  `--rpcs3-config-dir`; not counted as a failure. `Err`: counted as a failure,
  as today.
- **Content browser.** A patch is reported as in the window app. A DLC:
  `Ok` of either outcome marks it installed, as today; `Err` shows the
  error.

## 5. Invariants

The tests in § 7 run a stand-in RPCS3: a shell script, written to a temporary
folder, that records its arguments to a file and then does what the test
needs (copy a PARAM.SFO fixture into `dev_hdd0/game/<serial>/`, exit with a
chosen status). Unix only; none of the clauses below can run before the code
exists.

- **INV-1** — `install_pkg` passes RPCS3 exactly `--headless`,
  `--installpkg` and the package path.
  *Test:* `rpcs3::tests::install_runs_rpcs3_headless`: the stand-in's recorded
  arguments equal `["--headless", "--installpkg", <path>]`.
  *Breaks when:* `--headless` is dropped; RPCS3 then opens its window and the
  call waits for the user to close it.

- **INV-2** — With `AppVersion { version: Some(v) }`, an install gives
  `Ok(Confirmed)` only when APP_VER read afterwards equals `v`; otherwise
  `Err(NotInstalled { found })`.
  *Test:* `rpcs3::tests::install_confirms_the_expected_version`: a stand-in
  that writes APP_VER 01.05 with `v` = 01.05 gives `Confirmed`; with `v` =
  01.06 gives `NotInstalled { found: Some("01.05") }`; a stand-in that writes
  nothing gives `NotInstalled { found: None }`.
  *Breaks when:* the exit status alone decides, as today; the stand-in that
  writes nothing then reads as `Confirmed`.

- **INV-3** — With `AppVersion { version: None }`, an install gives
  `Ok(Confirmed)` only when APP_VER afterwards is present and differs from
  APP_VER before.
  *Test:* `rpcs3::tests::install_confirms_a_changed_version`: no PARAM.SFO
  before and the stand-in writes 01.05 gives `Confirmed`; a PARAM.SFO at
  01.05 before and a stand-in that writes nothing gives `NotInstalled`.
  *Breaks when:* APP_VER is read only after the run, so an untouched existing
  PARAM.SFO reads as an install.

- **INV-4** — *Withdrawn 2026-10-22 (RPSN-0168); replaced by INV-8.* It
  made a non-success exit `InstallFailed` whatever PARAM.SFO held, on the
  premise that the exit status could report a failure. § 2 item 2 records
  why it cannot.
  *Test:* none; its test is removed with it.

- **INV-5** — `InstallCheck::Unconfirmed` gives `Ok(Unconfirmed)` once RPCS3
  has run, and never `Confirmed`.
  *Test:* `rpcs3::tests::install_without_a_check_is_unconfirmed`.
  *Breaks when:* a clean exit with nothing checked is reported as
  `Confirmed`, the false claim this spec removes.

- **INV-6** — Trust boundary. A `serial` failing `looks_like_serial` is
  refused with `InstallFailed` before RPCS3 is launched, so no path outside
  `dev_hdd0/game` is read; PARAM.SFO is read only through `sfo::parse_sfo`,
  which caps the read.
  *Test:* `rpcs3::tests::install_refuses_an_unsafe_serial`: serial `../x`
  gives `InstallFailed`, and the stand-in's argument file does not exist.
  *Breaks when:* the serial is joined onto `dev_hdd0/game` unchecked.

- **INV-7** — Each caller passes the check § 4.3 gives it and reports as
  § 4.4 says.
  *Test:* manual, under Xvfb with a stand-in RPCS3 set as the executable:
  Install, the content browser's patch Install and "Update All Outdated", each with and without
  the RPCS3 folder set, show the success message and the warning message in turn; `--install` with and without `--rpcs3-config-dir`
  prints the two § 4.4 lines.
  *Breaks when:* a caller keeps passing `Unconfirmed` where it knows
  `dev_hdd0`, or reports `Unconfirmed` as success without the warning.

- **INV-8** — RPCS3's exit status decides nothing. With `AppVersion`, the
  version read decides as INV-2 and INV-3 say; with `Unconfirmed`, the
  result is `Ok(Unconfirmed)`. `install_firmware` is the same: its
  `FirmwareCheck` decides, and with none the result is `Ok(Unconfirmed)`.
  The exit status never gives `InstallFailed`; a failure to launch the
  process still does (§ 4.2 step 4).
  *Test:* `rpcs3::tests::install_ignores_rpcs3s_exit_status`: a stand-in that
  writes the expected APP_VER and exits 143 gives `Confirmed`; one that
  writes nothing and exits 0 gives `NotInstalled`; under `Unconfirmed`, one
  that exits 143 gives `Ok(Unconfirmed)`; for `install_firmware`, one that
  writes the expected `version.txt` and exits 143 gives `Confirmed`.
  *Breaks when:* a non-success exit is turned into an error before the
  version is read, as before this amendment.

- **INV-9** — *Added 2026-10-22 (RPSN-0170).* `run_headless` checks RPCS3
  is there before launching it, and a missing one is `InstallFailed` under
  every `InstallCheck` and for `install_firmware`. Three routes:
  - the Flatpak sentinel `flatpak:<app id>`: `flatpak info <app id>`, run on
    the host through `sandbox::host_command`, must exit 0;
  - a path, inside this app's own Flatpak: `test -x <path>`, run on the
    host the same way, must exit 0;
  - a path, outside any Flatpak: the path must exist, as before.

  A check that cannot be started is `InstallFailed` too. Measured
  2026-10-22, Flatpak 1.18.4: `flatpak info` exits 1 for an app id that is
  not installed and 0 for one that is; from this app's own Flatpak,
  `flatpak-spawn --host -- test -x` exits 1 for a missing path and 0 for
  `/usr/bin/ls`.
  *Test:* `rpcs3::tests::install_refuses_a_missing_flatpak_rpcs3`:
  `flatpak:net.rpcs3.NotThere` gives `InstallFailed` under `AppVersion` and
  under `Unconfirmed`. Without the check, `flatpak run` exits 1 and INV-8
  gives `NotInstalled` and `Ok(Unconfirmed)`. On a machine with no
  `flatpak`, the launch itself fails, so the test passes with or without
  the check there. The in-Flatpak route is a manual recipe: from the local
  Flatpak build, install with an RPCS3 path that does not exist on the
  host; the result is `InstallFailed` naming the path.
  *Breaks when:* a missing RPCS3 behind `flatpak` reaches INV-8, so a DLC
  is marked installed.

## 6. Failure modes

- **RPCS3 already running.** RPCS3 checks its instance lock
  (`<cache dir>/RPCS3.buf`) before it reads `--headless`, so a second
  instance shows RPCS3's modal fatal-error window, waits for the user to
  close it, and then kills itself (`report_fatal_error` in
  `rpcs3/rpcs3.cpp`). So `install_pkg` test-locks that file before launch
  and returns `AlreadyRunning` while RPCS3 holds it (RPSN-0138). RPCS3
  opened between that check and the launch still meets the window.
- **Two installs at once.** The same lock. `install_pkg` queues them, so
  each waits for the one before it to finish (RPSN-0138).
- **RPCS3 hangs.** The call never returns, as today. No timeout is added: a
  large package can take minutes and no figure would be right.
- **RPCS3 refuses the package** (wrong TARGET_APP_VER, a damaged file).
  Headless RPCS3's exit status is the same as on success; the version check
  reports `NotInstalled`.
  Under `Unconfirmed` this is reported as unchecked, which is the point of
  that outcome.
- **RPCS3 missing behind `flatpak`.** Through `flatpak run`, or through
  `flatpak-spawn --host` from this app's own Flatpak, a missing RPCS3 shows
  only as a non-zero exit, which INV-8 does not read. So INV-9 checks before
  launch and reports `InstallFailed`.
- **RPCS3 removed between INV-9's check and the launch.** The launcher's
  exit is not read (INV-8): `AppVersion` reports `NotInstalled`;
  `Unconfirmed` reports an unchecked install, so the content browser marks
  a DLC installed.
- **dev_hdd0 moved after detection.** The check reads the wrong folder: a
  false `NotInstalled`, or, with `version: Some`, a false `Confirmed` where
  that folder already holds the version.

## 7. Tests

| Test (in `src/rpcs3/mod.rs`) | Locks |
|---|---|
| `install_runs_rpcs3_headless` | INV-1 |
| `install_confirms_the_expected_version` | INV-2 |
| `install_confirms_a_changed_version` | INV-3 |
| `install_without_a_check_is_unconfirmed` | INV-5 |
| `install_refuses_an_unsafe_serial` | INV-6 |
| `install_ignores_rpcs3s_exit_status` | INV-8 |
| `install_refuses_a_missing_flatpak_rpcs3` | INV-9 (the Flatpak sentinel route) |

Each is `#[cfg(unix)]`, using `sfo::tests::make_test_sfo` for the PARAM.SFO.
INV-2's test must be seen failing with § 4.2 step 5 replaced by
`Ok(Confirmed)`, which is today's behaviour under the new signature. INV-7 is the manual
recipe in its clause, and so is INV-9's in-Flatpak route.

## 8. Alternatives considered (and rejected)

- **Keep RPCS3's window and check the version after it closes.** The user
  sees RPCS3 install and its own error dialogs. Rejected: the install still
  reads as running until the user closes RPCS3, which is problem 1, and
  RPSN-0058's batch install would need the user to close RPCS3 once per
  package.
- **Trust a non-success exit as a failure (the original INV-4).** Rejected
  2026-10-22 (RPSN-0168): its premise, that a non-zero exit means the
  install failed, was false; RPCS3 exits 143 after a successful headless
  install.
- **Read RPCS3's log for a success line.** No confirmation data needed from
  the caller. Rejected: the log's wording is not a contract and changes
  between RPCS3 versions.
- **Read the version from the package itself.** Would confirm content-browser
  patches against a fixed version. Rejected for now: it needs PKG metadata
  parsing rusty-psn does not have; `version: None` covers those patches.

## 9. Out of scope

- Checking for a running RPCS3 and running one install at a time — tracked by
  RPSN-0138.
- Confirming a DLC or its licences — deferred; `docs/design.md` records that
  Content cannot see them.
- The VERSION fallback and disc-versus-update choice in the library scan —
  tracked by RPSN-0140.
- A Flatpak RPCS3 that cannot see the package's folder — deferred; not yet
  queued.

## 10. What checks this

| Rule | What catches a breach |
|------|----------------------|
| INV-1 | `rpcs3::tests::install_runs_rpcs3_headless` |
| INV-2 | `rpcs3::tests::install_confirms_the_expected_version` |
| INV-3 | `rpcs3::tests::install_confirms_a_changed_version` |
| INV-4 | withdrawn; see INV-8 |
| INV-5 | `rpcs3::tests::install_without_a_check_is_unconfirmed` |
| INV-6 | `rpcs3::tests::install_refuses_an_unsafe_serial` |
| INV-7 | **nothing** — a manual recipe; no test reaches the window app's messages or the command line's install output |
| INV-8 | `rpcs3::tests::install_ignores_rpcs3s_exit_status` |
| INV-9 | `rpcs3::tests::install_refuses_a_missing_flatpak_rpcs3` for the sentinel route; the manual recipe in its clause for the in-Flatpak route |

## 11. Cross-doc impact

- `STANDARDS.md` — no passage describes how `install_pkg` launches RPCS3;
  none changes.
- `docs/design.md` — none: § 3's DLC decision keeps its Writes outside the
  download folder sentence true.
- `ROADMAP.md` — RPSN-0137's body gains this spec's path.

## 12. Cold-eyes loop log

Rows live in `../reviews/RPSN-0137-headless-confirmed-install-loop-log.md`.
