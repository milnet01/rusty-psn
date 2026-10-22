use std::fmt;
use std::io::Read;
use std::path::Path;

/// Maximum size for a batch file (1 MB). Prevents memory exhaustion from oversized files.
const MAX_BATCH_FILE_SIZE: u64 = 1_048_576;

/// Result of parsing a batch file.
pub struct BatchParseResult {
    /// Valid, deduplicated serials ready to search.
    pub serials: Vec<String>,
    /// Lines that were skipped: (line_number, original_text, reason).
    pub warnings: Vec<(usize, String, String)>,
}

/// Errors that can occur when parsing a batch file.
#[derive(Debug)]
pub enum BatchFileError {
    Io(std::io::Error),
    TooLarge(u64),
    NoValidSerials,
}

impl fmt::Display for BatchFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BatchFileError::Io(e) => f.write_str(&tr!("error-batch-read", reason = e.to_string())),
            // Numbers go in as text, so Fluent cannot regroup their digits.
            BatchFileError::TooLarge(size) => f.write_str(&tr!(
                "error-batch-too-large",
                size = size.to_string(),
                max = MAX_BATCH_FILE_SIZE.to_string()
            )),
            BatchFileError::NoValidSerials => f.write_str(&tr!("error-batch-no-serials")),
        }
    }
}

/// Parse a batch text file containing game serials, one per line.
///
/// Format:
/// - One serial per line (e.g., `BCUS98111`, `BCES-01234`, `npub30826`)
/// - Lines starting with `#` are comments (inline `#` also strips the rest)
/// - Blank lines are ignored
/// - Serials are normalized (uppercase, dashes removed) and validated
/// - Duplicates are removed (first occurrence kept)
pub fn parse_batch_file(path: &Path) -> Result<BatchParseResult, BatchFileError> {
    // Check file size before reading.
    let metadata = std::fs::metadata(path).map_err(BatchFileError::Io)?;
    if metadata.len() > MAX_BATCH_FILE_SIZE {
        return Err(BatchFileError::TooLarge(metadata.len()));
    }

    // The reported size can be wrong, as for a pipe or a file still growing, so the read is capped too.
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_BATCH_FILE_SIZE + 1).read_to_end(&mut bytes))
        .map_err(BatchFileError::Io)?;
    if bytes.len() as u64 > MAX_BATCH_FILE_SIZE {
        return Err(BatchFileError::TooLarge(bytes.len() as u64));
    }
    let content =
        String::from_utf8(bytes).map_err(|e| BatchFileError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;

    let mut serials = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut warnings = Vec::new();

    for (line_idx, raw_line) in content.lines().enumerate() {
        let line_number = line_idx + 1;

        // Strip comments: everything from '#' onward.
        let line = match raw_line.find('#') {
            Some(pos) => &raw_line[..pos],
            None => raw_line,
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Normalize the serial (uppercase, remove dashes).
        let normalized = crate::psn::parse_title_id(trimmed);

        // Validate the serial format.
        if let Err(e) = crate::security::validate_serial(&normalized) {
            // The line is echoed to the terminal, so control characters are dropped.
            warnings.push((line_number, crate::security::strip_control(trimmed), e.to_string()));
            continue;
        }

        // Deduplicate.
        if seen.insert(normalized.clone()) {
            serials.push(normalized);
        }
    }

    if serials.is_empty() {
        return Err(BatchFileError::NoValidSerials);
    }

    Ok(BatchParseResult { serials, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn errors_read_as_before_the_translation_layer() {
        let io = BatchFileError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
        assert_eq!(io.to_string(), "Failed to read batch file: gone");
        assert_eq!(
            BatchFileError::TooLarge(2_000_000).to_string(),
            "Batch file too large (2000000 bytes, max 1048576)"
        );
        assert_eq!(
            BatchFileError::NoValidSerials.to_string(),
            "Batch file contains no valid serials"
        );
    }

    fn write_temp_file(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn warning_text_has_no_control_characters() {
        // A skipped line is echoed to the terminal, so an escape sequence must not survive (RPSN-0107).
        let f = write_temp_file("BLUS31147\nBAD\x1b[2JLINE\n");
        let result = parse_batch_file(f.path()).unwrap();
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.warnings[0].1, "BAD[2JLINE");
    }

    #[test]
    fn parse_basic_serials() {
        let f = write_temp_file("BCUS98111\nNPUB30826\nBLUS31147\n");
        let result = parse_batch_file(f.path()).unwrap();
        assert_eq!(result.serials, vec!["BCUS98111", "NPUB30826", "BLUS31147"]);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn parse_comments_and_blanks() {
        let f = write_temp_file("# My PS3 games\nBCUS98111   # God of War III\n\n# Another comment\nNPUB30826\n\n");
        let result = parse_batch_file(f.path()).unwrap();
        assert_eq!(result.serials, vec!["BCUS98111", "NPUB30826"]);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn parse_normalizes_input() {
        // Lowercase and dashes should be normalized.
        let f = write_temp_file("bces-01234\nNPUB-30826\n");
        let result = parse_batch_file(f.path()).unwrap();
        assert_eq!(result.serials, vec!["BCES01234", "NPUB30826"]);
    }

    #[test]
    fn parse_deduplicates() {
        let f = write_temp_file("BCUS98111\nBCUS98111\nNPUB30826\nBCUS98111\n");
        let result = parse_batch_file(f.path()).unwrap();
        assert_eq!(result.serials, vec!["BCUS98111", "NPUB30826"]);
    }

    #[test]
    fn parse_mixed_valid_invalid() {
        let f = write_temp_file("BCUS98111\nINVALID\nNPUB30826\nAB\n");
        let result = parse_batch_file(f.path()).unwrap();
        assert_eq!(result.serials, vec!["BCUS98111", "NPUB30826"]);
        assert_eq!(result.warnings.len(), 2);
        assert_eq!(result.warnings[0].1, "INVALID");
        assert_eq!(result.warnings[1].1, "AB");
    }

    #[test]
    fn parse_only_comments_returns_error() {
        let f = write_temp_file("# just a comment\n# another one\n\n");
        let result = parse_batch_file(f.path());
        assert!(matches!(result, Err(BatchFileError::NoValidSerials)));
    }

    #[test]
    fn parse_only_invalid_returns_error() {
        let f = write_temp_file("INVALID\nBAD\nNOPE\n");
        let result = parse_batch_file(f.path());
        assert!(matches!(result, Err(BatchFileError::NoValidSerials)));
    }

    #[cfg(unix)]
    #[test]
    fn parse_caps_the_read_not_only_the_reported_size() {
        // A pipe reports size 0, so only a capped read stops it (RPSN-0149).
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("batch.fifo");
        let made = std::process::Command::new("mkfifo").arg(&fifo).status().unwrap();
        assert!(made.success());
        let writer_path = fifo.clone();
        let writer = std::thread::spawn(move || {
            let mut pipe = std::fs::OpenOptions::new().write(true).open(writer_path).unwrap();
            let line = b"BCUS98111\n".repeat(1024);
            // The reader stops early, so a later write fails with a broken pipe; that is expected.
            for _ in 0..200 {
                if pipe.write_all(&line).is_err() {
                    break;
                }
            }
        });

        let result = parse_batch_file(&fifo);

        drop(writer.join());
        assert!(matches!(result, Err(BatchFileError::TooLarge(_))));
    }

    #[test]
    fn parse_nonexistent_file_returns_io_error() {
        let result = parse_batch_file(Path::new("/nonexistent/batch.txt"));
        assert!(matches!(result, Err(BatchFileError::Io(_))));
    }
}
