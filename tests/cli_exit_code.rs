//! The command line's exit code, checked by running the built binary (RPSN-0106).
#![cfg(all(feature = "cli", not(feature = "egui")))]

use std::process::{Command, Stdio};

#[test]
fn a_failed_search_exits_non_zero() {
    // A malformed serial fails validation before any request, so this needs no network.
    // It runs in an empty folder so nothing lands in the repository.
    let dir = tempfile::tempdir().expect("temp dir");
    let status = Command::new(env!("CARGO_BIN_EXE_rusty-psn"))
        .args(["-t", "bad!", "--silent", "--no-log-file"])
        .current_dir(dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run rusty-psn");
    assert_eq!(status.code(), Some(1));
}
