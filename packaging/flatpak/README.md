# Local Flatpak

A Flatpak of the window app, built and installed on this machine (RPSN-0074). It is not
for Flathub: the build fetches crates from the network, and the app starts RPCS3
outside its sandbox through `flatpak-spawn --host`.

## Build and install

Needs `flatpak-builder`, the `org.freedesktop.Sdk//26.08` runtime and its Rust
extension:

```sh
flatpak install --user flathub org.freedesktop.Sdk.Extension.rust-stable//26.08
```

From the project folder, with a build folder on a real disk:

```sh
flatpak-builder --user --install --force-clean \
    --state-dir=<build folder>/state <build folder>/rusty-psn \
    packaging/flatpak/io.github.milnet01.RustyPsn.yml
```

Then start it from the application menu, or with
`flatpak run io.github.milnet01.RustyPsn`.

## How it differs from the native build

- **Settings are its own.** The Flatpak keeps them under
  `~/.var/app/io.github.milnet01.RustyPsn/`, apart from the native build's.
- **RPCS3 runs on the host**, as the native build runs it. `src/sandbox.rs` routes
  RPCS3 and the other host programs through `flatpak-spawn --host`, and reads RPCS3's
  folders from the host's XDG variables.
- **It sees every host file** (`--filesystem=host`), so RPCS3's folders and the
  download folder can be anywhere.
