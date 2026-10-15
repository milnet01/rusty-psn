//! Embeds the application icon in the Windows executable (RPSN-0078). Other systems take the
//! icon from their bundle or desktop file, so there the script does nothing.

fn main() {
    println!("cargo:rerun-if-changed=resources/rusty-psn.rc");
    println!("cargo:rerun-if-changed=resources/icon.ico");

    // `windows` is the system the build runs on, which is the one the release builds target.
    #[cfg(windows)]
    embed_resource::compile("resources/rusty-psn.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("resources/rusty-psn.rc compiles"); // The release cannot ship without its icon.
}
