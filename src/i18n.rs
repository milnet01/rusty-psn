//! The on-screen text (RPSN-0072, `docs/specs/RPSN-0072-translation-layer.md`).
//!
//! Every text the window app shows comes from `i18n/en/rusty-psn.ftl` through `tr!`. The file is
//! built into the program, and `fl!` checks every message id and argument against it at compile
//! time. English is the only language until RPSN-0163.

use std::sync::LazyLock;

use i18n_embed::fluent::FluentLanguageLoader;
use i18n_embed::LanguageLoader;

#[derive(rust_embed::RustEmbed)]
#[folder = "i18n"]
struct Localizations;

/// English, loaded once.
pub static LOADER: LazyLock<FluentLanguageLoader> = LazyLock::new(|| {
    let loader = FluentLanguageLoader::new("rusty-psn", "en".parse().expect("\"en\" is a language id"));
    loader
        .load_languages(&Localizations, &[loader.fallback_language().clone()])
        // fl! has already read this file at compile time, so only a broken build gets here.
        .expect("the English text built into the program loads");
    // Fluent wraps each placeholder in U+2068 and U+2069, which egui's fonts draw as boxes.
    loader.set_use_isolating(false);
    loader
});

/// `tr!("settings-theme")`, or with arguments `tr!("downloads-count", count = n)`.
#[macro_export]
macro_rules! tr {
    ($($args:tt)*) => {
        i18n_embed_fl::fl!($crate::i18n::LOADER, $($args)*)
    };
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    const ENGLISH: &str = include_str!("../i18n/en/rusty-psn.ftl");

    /// Every `.rs` file under `dir`, in a stable order.
    fn rust_files(dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for entry in fs::read_dir(dir).expect("source folder reads").flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(rust_files(&path));
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
        files.sort();
        files
    }

    /// `text` with every `//` comment removed. A `//` inside a string literal is kept.
    fn without_comments(text: &str) -> String {
        let mut out = String::new();
        for line in text.lines() {
            let mut in_string = false;
            let mut escaped = false;
            let mut cut = line.len();
            let bytes = line.as_bytes();
            for i in 0..bytes.len() {
                let b = bytes[i];
                if in_string {
                    if escaped {
                        escaped = false;
                    } else if b == b'\\' {
                        escaped = true;
                    } else if b == b'"' {
                        in_string = false;
                    }
                } else if b == b'"' {
                    in_string = true;
                } else if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
                    cut = i;
                    break;
                }
            }
            out.push_str(&line[..cut]);
            out.push('\n');
        }
        out
    }

    /// The message ids the English file defines: lines that start with a letter and hold " =".
    fn english_ids() -> Vec<&'static str> {
        ENGLISH
            .lines()
            .filter(|line| line.starts_with(|c: char| c.is_ascii_lowercase()))
            .filter_map(|line| line.split_once(" =").map(|(id, _)| id.trim()))
            .collect()
    }

    #[test]
    fn every_english_message_is_used() {
        let code: String = rust_files(Path::new("src"))
            .iter()
            .map(|path| without_comments(&fs::read_to_string(path).expect("source file reads")))
            .collect();
        // rustfmt may put the id on the line after `tr!(`.
        let code = code.split("tr!(").map(str::trim_start).collect::<Vec<_>>().join("tr!(");
        let unused: Vec<_> = english_ids()
            .into_iter()
            .filter(|id| !code.contains(&format!("tr!(\"{id}\"")))
            .collect();
        assert!(unused.is_empty(), "messages no tr! call uses: {unused:?}");
    }

    #[test]
    fn placeholders_carry_no_isolation_marks() {
        let text = tr!("error-batch-read", reason = "disk gone");
        assert_eq!(text, "Failed to read batch file: disk gone");
        assert!(!text.contains(['\u{2068}', '\u{2069}']));
    }

    /// A call that draws text, and which of its arguments is the text (0 is the first).
    const TEXT_CALLS: &[(&str, usize)] = &[
        ("label", 0),
        ("button", 0),
        ("small_button", 0),
        ("heading", 0),
        ("RichText::new", 0),
        ("Button::new", 0),
        ("Label::new", 0),
        ("CollapsingHeader::new", 0),
        ("on_hover_text", 0),
        ("on_disabled_hover_text", 0),
        ("hint_text", 0),
        ("hyperlink_to", 0),
        ("checkbox", 1),
        ("selectable_label", 1),
        ("selectable_value", 2),
        ("radio_value", 2),
        ("strong", 0),
        ("weak", 0),
        ("add_filter", 0),
        ("status_pill", 1),
        ("show_notifications", 0),
    ];

    /// Literals that are names rather than words, so they stay as they are.
    const PROPER_NAMES: &[&str] = &["RPCS3", "PSN", "rusty-psn", "JSON"];

    /// The arguments of the call whose `(` is at `open`, split at top-level commas.
    fn call_arguments(code: &str, open: usize) -> Vec<&str> {
        let bytes = code.as_bytes();
        let (mut depth, mut start, mut args) = (0usize, open + 1, Vec::new());
        let (mut in_string, mut escaped) = (false, false);
        for (i, &b) in bytes.iter().enumerate().skip(open) {
            if in_string {
                if escaped {
                    escaped = false;
                } else if b == b'\\' {
                    escaped = true;
                } else if b == b'"' {
                    in_string = false;
                }
                continue;
            }
            match b {
                b'"' => in_string = true,
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        args.push(&code[start..i]);
                        return args;
                    }
                }
                b',' if depth == 1 => {
                    args.push(&code[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        args
    }

    /// Whether a literal's source holds a letter, not counting `\u{...}` escapes, which draw symbols.
    fn holds_a_letter(literal: &str) -> bool {
        let (mut kept, mut rest) = (String::new(), literal);
        while let Some(at) = rest.find("\\u{") {
            kept.push_str(&rest[..at]);
            rest = rest[at..].split_once('}').map_or("", |(_, after)| after);
        }
        kept.push_str(rest);
        kept.chars().any(|c| c.is_ascii_alphabetic())
    }

    /// Text calls in `code` whose text argument is a string literal holding a letter.
    fn inline_text(code: &str) -> Vec<String> {
        let mut found = Vec::new();
        for &(name, index) in TEXT_CALLS {
            let pattern = format!("{name}(");
            for (at, _) in code.match_indices(&pattern) {
                let before = code[..at].chars().next_back();
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || (c == ':' && !name.contains("::"))) {
                    continue; // A longer name ending in this one, or a path to another `new`.
                }
                if code[..at].trim_end().ends_with("fn") {
                    continue; // The definition, not a call.
                }
                let args = call_arguments(code, at + pattern.len() - 1);
                let Some(text) = args.get(index).map(|arg| arg.trim()) else {
                    continue;
                };
                let literal = text.starts_with('"') && text.ends_with('"') && text.len() >= 2;
                if literal && holds_a_letter(text) && !PROPER_NAMES.contains(&&text[1..text.len() - 1]) {
                    found.push(format!("{name}({text}"));
                }
            }
        }
        found
    }

    #[test]
    fn the_window_app_has_no_inline_text() {
        let mut found = Vec::new();
        for path in rust_files(Path::new("src/egui")) {
            let text = fs::read_to_string(&path).expect("source file reads");
            // Test code may use any literal it likes.
            let code = without_comments(text.split("#[cfg(test)]").next().unwrap_or_default());
            found.extend(
                inline_text(&code)
                    .into_iter()
                    .map(|call| format!("{}: {call}", path.display())),
            );
        }
        assert!(
            found.is_empty(),
            "on-screen text written inline, not through tr!:\n{}",
            found.join("\n")
        );
    }

    #[test]
    fn the_inline_text_scan_sees_each_argument_position() {
        let code = r#"ui.label("Hello"); ui.checkbox(&mut on, "Show"); ui.selectable_value(&mut v, Kind::A, "All");
            ui.hyperlink_to(tr!("x"), "https://example.com"); fn label(text: &str) {} ui.label("RPCS3");
            ui.selectable_label(true, "Pick"); ui.label(tr!("x")); show_notifications("Done", level);
            ui.small_button("\u{2715}");"#;
        assert_eq!(
            inline_text(code),
            [
                r#"label("Hello""#,
                r#"checkbox("Show""#,
                r#"selectable_label("Pick""#,
                r#"selectable_value("All""#,
                r#"show_notifications("Done""#
            ]
        );
    }
}
