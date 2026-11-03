//! Running inside a Flatpak sandbox (RPSN-0074).
//!
//! The local Flatpak (`packaging/flatpak/`) sees the host's files at their own paths, but runs
//! programs from its own runtime and points the XDG folder variables into `~/.var/app/<app id>`.
//! RPCS3, `flatpak`, `which`, `journalctl` and `dbus-send` are the host's, and RPCS3's folders
//! follow the host's variables, so both are reached through here. Outside a sandbox each helper
//! does what the plain call would.

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

/// Whether this process runs inside a Flatpak. Flatpak puts `/.flatpak-info` in every sandbox.
pub fn in_flatpak() -> bool {
    static IN_FLATPAK: OnceLock<bool> = OnceLock::new();
    *IN_FLATPAK.get_or_init(|| Path::new("/.flatpak-info").exists())
}

/// A command that runs `program` on the host: through `flatpak-spawn --host` inside a Flatpak,
/// which needs the manifest's `--talk-name=org.freedesktop.Flatpak`, and directly elsewhere.
/// `--` keeps a program named like an option from being read as one of flatpak-spawn's.
pub fn host_command(program: impl AsRef<OsStr>) -> Command {
    host_command_for(in_flatpak(), program.as_ref(), &[])
}

/// As `host_command`, with the variables in `unset` removed from the program's environment.
/// flatpak-spawn starts the program in the host session's environment, not this one, so there
/// the removal is its `--unset-env` option.
pub fn host_command_unsetting(program: impl AsRef<OsStr>, unset: &[&str]) -> Command {
    host_command_for(in_flatpak(), program.as_ref(), unset)
}

fn host_command_for(sandboxed: bool, program: &OsStr, unset: &[&str]) -> Command {
    if sandboxed {
        let mut command = Command::new("flatpak-spawn");
        command.arg("--host");
        command.args(unset.iter().map(|name| format!("--unset-env={name}")));
        command.arg("--").arg(program);
        command
    } else {
        let mut command = Command::new(program);
        for name in unset {
            command.env_remove(name);
        }
        command
    }
}

/// The host's value of environment variable `key`. Inside a Flatpak, `XDG_*_HOME` name the
/// sandbox's own folders; Flatpak keeps the host's under `HOST_XDG_*_HOME`, and leaves that
/// unset where the host had none, so the caller's own fallback applies as on the host.
pub fn host_env(key: &str) -> Option<OsString> {
    host_env_from(in_flatpak(), key, |name| std::env::var_os(name))
}

fn host_env_from(sandboxed: bool, key: &str, env: impl Fn(&str) -> Option<OsString>) -> Option<OsString> {
    if sandboxed && key.starts_with("XDG_") && key.ends_with("_HOME") {
        env(&format!("HOST_{key}"))
    } else {
        env(key)
    }
}

/// A folder for temporary files a host program must read. The sandbox's `/tmp` is its own, so
/// inside a Flatpak this is the app's cache folder, which sits at the same path on the host.
#[cfg(feature = "egui")]
pub fn host_visible_temp_dir() -> std::path::PathBuf {
    if in_flatpak() {
        if let Some(cache) = std::env::var_os("XDG_CACHE_HOME").filter(|dir| !dir.is_empty()) {
            return cache.into();
        }
    }
    std::env::temp_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_command_goes_through_flatpak_spawn_only_in_a_sandbox() {
        let sandboxed = host_command_for(true, OsStr::new("rpcs3"), &[]);
        assert_eq!(sandboxed.get_program(), "flatpak-spawn");
        assert_eq!(sandboxed.get_args().collect::<Vec<_>>(), ["--host", "--", "rpcs3"]);

        let native = host_command_for(false, OsStr::new("rpcs3"), &[]);
        assert_eq!(native.get_program(), "rpcs3");
        assert_eq!(native.get_args().count(), 0);
        assert_eq!(native.get_envs().count(), 0);
    }

    #[test]
    fn host_command_unsets_variables_on_either_route() {
        // RPSN-0181: a portable RPCS3 AppImage must not see the session's XDG folders.
        let sandboxed = host_command_for(true, OsStr::new("rpcs3"), &["XDG_CONFIG_HOME", "XDG_DATA_HOME"]);
        assert_eq!(
            sandboxed.get_args().collect::<Vec<_>>(),
            [
                "--host",
                "--unset-env=XDG_CONFIG_HOME",
                "--unset-env=XDG_DATA_HOME",
                "--",
                "rpcs3"
            ]
        );

        let native = host_command_for(false, OsStr::new("rpcs3"), &["XDG_CONFIG_HOME"]);
        assert_eq!(native.get_args().count(), 0);
        assert_eq!(native.get_envs().collect::<Vec<_>>(), [(OsStr::new("XDG_CONFIG_HOME"), None)]);
    }

    #[test]
    fn host_env_reads_the_hosts_xdg_folders_in_a_sandbox() {
        let env = |key: &str| match key {
            "XDG_CACHE_HOME" => Some(OsString::from("/h/.var/app/x/cache")),
            "XDG_CONFIG_HOME" => Some(OsString::from("/h/.var/app/x/config")),
            "HOST_XDG_CACHE_HOME" => Some(OsString::from("/h/.cache")),
            "HOME" => Some(OsString::from("/h")),
            _ => None,
        };
        assert_eq!(host_env_from(true, "XDG_CACHE_HOME", env), Some("/h/.cache".into()));
        assert_eq!(
            host_env_from(false, "XDG_CACHE_HOME", env),
            Some("/h/.var/app/x/cache".into())
        );
        // A host with no XDG_CONFIG_HOME of its own: the sandbox's value must not stand in for it.
        assert_eq!(host_env_from(true, "XDG_CONFIG_HOME", env), None);
        assert_eq!(host_env_from(true, "HOME", env), Some("/h".into()));
    }
}
