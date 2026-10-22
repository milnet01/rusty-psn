<!-- ants-spec-format: 1 -->
# RPSN-0072 — Route every on-screen text through a translation layer

**Status:** spec draft (2026-10-22).
**Kind:** enhancement.
**Source:** ROADMAP RPSN-0072 (owner, 2026-10-15: the fork will be offered as a download).

**Blocker for:** RPSN-0163.

**Layman:** No visible change: the app still shows English. Behind the scenes
every word on screen now comes from one text file, so a translator can add a
language later without touching the program.

## 1. Goal

Every text the window app shows comes from one English Fluent file,
`i18n/en/rusty-psn.ftl`, built into the program. Adding a language later
(RPSN-0163) means adding one file beside it and a language picker, with no
change to the code that draws the screens. The screens read exactly as they
do today.

## 2. Problem

1. **On-screen text is written inline.** Labels, buttons, tooltips and
   messages are string literals and `format!` strings in the files under
   `src/egui/` (for example `UpdatesApp::draw_settings_page` in
   `src/egui/settings.rs`). A translation would have to edit every one.
2. **Some on-screen text comes from shared parts.** The window app shows the
   `Display` text of error types from other parts — `PsnClientError` and
   `MergeError` (`src/psn/mod.rs`), `FirmwareError` and `FirmwareType`
   (`src/psn/firmware.rs`), `Rpcs3Error` (`src/rpcs3/mod.rs`),
   `ContentError` (`src/content.rs`), `BatchFileError` (`src/batch.rs`),
   `SecurityError` (`src/security.rs`) — and of `DirectoryFormat`
   (`src/utils.rs`). The command line prints the same text.
3. **Nothing stops new inline text.** No rule or check says where
   on-screen text goes.

## 3. Scope decisions (agreed with the user)

- **English only in this item.** Owner, 2026-10-15 (RPSN-0072's body): the
  layer now, translations last, as RPSN-0163.
- **Every on-screen string.** Same source. §4.4 defines what counts.

## 4. Design

### 4.1 Library

`i18n-embed` 0.16.0 with the `fluent-system` feature, `i18n-embed-fl`
0.10.1 and `rust-embed` 8.13.0 — the latest versions on crates.io on
2026-10-22. Fluent is the format Firefox's translators use; it handles
plurals and placeholders per language. The `fl!` macro checks at compile
time that a message id exists in the English file and that its arguments
match.

Source: <https://crates.io/api/v1/crates/i18n-embed-fl>,
<https://github.com/kellpossible/cargo-i18n/blob/master/i18n-embed-fl/README.md>.

No new advisory: `rust-embed`'s RUSTSEC-2021-0126 is patched from 6.3.0
(`~/.cargo/advisory-db/crates/rust-embed/RUSTSEC-2021-0126.md`).

```toml
# Cargo.toml [dependencies], not optional: shared parts use them too
i18n-embed = { version = "0.16.0", features = ["fluent-system"] }
i18n-embed-fl = "0.10.1"
rust-embed = "8.13.0"
```

```toml
# i18n.toml, beside Cargo.toml; read by fl! at compile time
fallback_language = "en"

[fluent]
assets_dir = "i18n"
```

`fl!` takes the domain from `CARGO_PKG_NAME`, so the file is
`i18n/en/rusty-psn.ftl` (`i18n_embed::fluent::FluentLanguageLoader`
builds the name as `<domain>.ftl`).

### 4.2 The Text part

A new part, `src/i18n.rs`, holds the loader and one macro:

```rust
#[derive(rust_embed::RustEmbed)]
#[folder = "i18n"]
struct Localizations;

/// English loaded once; no isolation marks (INV-3).
pub static LOADER: LazyLock<FluentLanguageLoader> = /* new("rusty-psn", "en"), load "en", set_use_isolating(false) */;

/// `tr!("settings-theme")`, `tr!("downloads-count", count = n)`.
#[macro_export]
macro_rules! tr { /* forwards to i18n_embed_fl::fl!($crate::i18n::LOADER, ...) */ }
```

Fluent wraps each placeholder in U+2068 and U+2069 by default; egui's fonts
draw those as boxes, so the loader turns them off.

### 4.3 Message ids

Lower-case words joined by `-`, starting with the area: the file stem of
the screen (`settings-theme`, `firmware-download`), `error-` for shared
parts' error text, `common-` for words several screens share (`common-cancel`).
One id per distinct text: two screens showing the same word use one id only
where it means the same thing in both places.

### 4.4 What goes through it

**Through the layer:** every label, button, tooltip, heading, menu entry,
table header, dialog text and Messages-panel message the window app shows,
and the `Display` text of the shared-part types named in §2.2 except the
proper names below.

**Not through it:** proper names (PlayStation 3, RPCS3, PSN, Sony,
Windows); data — game titles, serials, versions, sizes, paths, and text
Sony or RPCS3 sends; log lines (`log::` macros); the command line's own
messages in `src/cli/` (kept for AI use, English only); numbers and units
formatted by code.

**The About dialog's disclaimer goes through it too.**
`dialogs::tests::the_readme_carries_the_about_disclaimer` then compares the
English message with `README.md`, word for word as today.

**Never translated:** text that becomes a file or folder name, a path, a
URL, a settings value or anything sent to Sony or RPCS3 (INV-5).

### 4.5 The rule for new text

Added to `STANDARDS.md`, beside the naming conventions:

> **On-screen text.** Every text the window app shows, and the `Display`
> text of an error a front end shows, comes from `i18n/en/rusty-psn.ftl`
> through `tr!` (`src/i18n.rs`, RPSN-0072). Proper names, data, log lines and
> the command line's own messages stay literal. Text that becomes a path,
> URL, file name or setting is never translated.

## 5. Invariants

- **INV-1** — Every `tr!` call names a message the English file has, with
  the arguments that message uses.
  *Test:* the build itself: `fl!` reads `i18n/en/rusty-psn.ftl` at compile
  time and fails on an unknown id or argument.
  *Breaks when:* a call names an id or an argument the file lacks.

- **INV-2** — Every message in the English file is used by the code.
  *Test:* `i18n::tests::every_english_message_is_used` — reads each id from
  the file and searches `src/` for it in a `tr!` call.
  *Breaks when:* a message stays in the file after its last use is removed.

- **INV-3** — Translated text carries no U+2068 or U+2069.
  *Test:* `i18n::tests::placeholders_carry_no_isolation_marks` — renders a
  message with an argument and checks the result.
  *Breaks when:* the loader keeps Fluent's default isolation.

- **INV-4** — No window-app text call takes a literal containing a letter.
  *Test:* `i18n::tests::the_window_app_has_no_inline_text` — scans the files
  under `src/egui/` for `label(`, `button(`, `small_button(`, `heading(`,
  `RichText::new(`, `Button::new(`, `Label::new(`, `CollapsingHeader::new(`,
  `on_hover_text(`, `on_disabled_hover_text(`, `hint_text(`, `hyperlink_to(`,
  `checkbox(`, `selectable_label(`, `selectable_value(`, `radio_value(` and
  `show_notifications(` whose string-literal argument contains an ASCII
  letter, and fails naming each. Proper names listed in the test are
  allowed.
  *Breaks when:* someone writes `ui.button("Save")`.

- **INV-5** — No translated text becomes a path, file name, URL, settings
  value or request sent to Sony or RPCS3.
  *Test:* review of every `tr!` call whose result is not passed straight to
  a text call; the existing path and URL tests (`utils::tests`,
  `psn::tests`, `security::tests`) pass unchanged.
  *Breaks when:* a translated word, such as a `DirectoryFormat` name, is
  used to build a folder name.

- **INV-6** — In English, every screen reads as before.
  *Test:* every existing test asserting on-screen or `Display` text passes
  unchanged; and a manual comparison of each sidebar page before and after,
  captured with demoreel at the same window size.
  *Breaks when:* a moved text loses or changes a word, or a placeholder
  renders as `{ $name }`.

- **INV-7** — The command line still builds and prints English.
  *Test:* `cargo test --features cli` passes; `cargo clippy --no-default-features --features cli -- -D warnings` is clean.
  *Breaks when:* a shared part's text needs the window app's build to
  compile.

## 6. Failure modes

- **The English file fails to load at run time.** It is built into the
  program and checked at compile time, so only a broken build reaches this.
  `LOADER` then panics at first use with the loader's error — at start-up,
  not mid-download, because the first frame draws text.
- **A message is missing from a later language's file.** Fluent falls back
  to English for that message (RPSN-0163's concern; nothing in this item
  ships a second language).
- **A placeholder name is misspelled in the file.** INV-1: the build fails.

## 7. Tests

- `i18n::tests::every_english_message_is_used` — INV-2.
- `i18n::tests::placeholders_carry_no_isolation_marks` — INV-3.
- `i18n::tests::the_window_app_has_no_inline_text` — INV-4. Seen failing
  against today's code before any text is moved, naming the inline calls.
- INV-1 — the compile step.
- INV-5 — `utils::tests`, `psn::tests`, `security::tests`, plus review.
- INV-6 — the existing text assertions, plus the manual demoreel
  comparison.
- INV-7 — `cargo test --features cli` and the cli clippy run.

## 8. Alternatives considered (and rejected)

- **`rust-i18n` 4.2.4** — simpler YAML files, but a missing key shows the
  key at run time instead of failing the build, and its README documents no
  plural rules.
- **gettext (`gettext-rs`, `tr`)** — needs the C gettext library: a
  problem for the Windows builds and the Flatpak.
- **`fluent-templates`** — Fluent too, but no compile-time check of ids.
- **Keep shared parts' error text English, translate only `src/egui/`** —
  smaller, but error messages are among the most-read text on screen, and
  the owner asked for every on-screen string.

## 9. Out of scope

- Other languages and a language picker — tracked by RPSN-0163.
- The command line's own messages — deferred; not yet queued.
- Translating the README and the Flatpak metainfo — deferred; not yet
  queued.

## 10. What checks this

| Rule | What catches a breach |
|------|----------------------|
| INV-1 | the compile step (`fl!`) |
| INV-2 | `i18n::tests::every_english_message_is_used` |
| INV-3 | `i18n::tests::placeholders_carry_no_isolation_marks` |
| INV-4 | Partial: `i18n::tests::the_window_app_has_no_inline_text` sees literal arguments only; a `format!` string passed to a text call is caught by review |
| INV-5 | **nothing** automatic — review of `tr!` results used outside text calls; existing path and URL tests catch a changed path only where one covers it |
| INV-6 | Partial: existing text assertions cover the texts they name; the rest rests on the manual demoreel comparison |
| INV-7 | `cargo test --features cli` and the cli clippy run in the pre-push gate |
| §4.5 rule | INV-1 to INV-4's checks; nothing else |

## 11. Cross-doc impact

- `STANDARDS.md` — gains §4.5's rule.
- `docs/design.md` — § The parts gains a row: **Text** — the English
  on-screen text and the `tr!` macro — `src/i18n.rs`, `i18n/en/rusty-psn.ftl`.
  § What may depend on what gains: **Text uses no other part**; the front
  ends, PSN, RPCS3, Content, Batch, Security and Files may use it.
- `CHANGELOG.md` — one entry when it ships.
- `ROADMAP.md` — RPSN-0072's body gains this spec's path.

## 12. Cold-eyes loop log

Rows live in `../reviews/RPSN-0072-translation-layer-loop-log.md`.

## 13. Resource cost

Three new direct dependencies (§4.1), plus their own: Fluent's parser and
`unic-langid`, and proc macros at build time. The English file is held in
memory once.
