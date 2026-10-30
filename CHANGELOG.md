# Changelog

What changed in this fork of rusty-psn, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). It starts on
2026-10-19; earlier work is recorded in ROADMAP.md.

## [Unreleased]

### Added

- **A disclaimer in Settings, About rusty-psn, and in the README: no piracy, only Sony's official updates and system software, not affiliated with Sony** (RPSN-0166)

### Changed

- **README and About dialog now point at this fork, with a notice that it is unofficial and where to report problems** (RPSN-0162)

- **Use smaller read and write buffers for checksums and merges.** (RPSN-0176)
  Checking and merging downloads uses far less memory.

- **The app's on-screen text is ready for translation** (RPSN-0072)
  Every word the window app shows now comes from one English text file, so other languages can be added later. The app reads exactly as before.

- **The window starts at 1240 x 860 and can shrink to 960 x 600; a narrow window wraps the status bar onto a second row instead of cutting it off** (RPSN-0164)

### Fixed

- **Tests no longer depend on where the build folder is** (RPSN-0172)

- **The format check on pull requests no longer fails on correctly formatted code** (RPSN-0177)

- **Merge a split PS4 update's parts in part order.** (RPSN-0174)

- **Report a refused PS4 manifest request as a network error.** (RPSN-0175)

- **The command line no longer reports older updates as failed installs** (RPSN-0171)
  With --install, an update RPCS3 already has, or one older than the installed version, is skipped with a note instead of being sent to RPCS3 and counted as a failure.

- **A missing Flatpak RPCS3 is reported, not treated as an install** (RPSN-0170)
  If RPCS3 is set up as a Flatpak that is not installed, the app now says so before it starts, instead of marking a DLC installed.

- **An RPCS3 install that worked is no longer reported as failed** (RPSN-0168)
  RPCS3 can crash as it closes after a good install. The app now judges an update or firmware install by the version RPCS3 shows afterwards, not by how RPCS3 exited.

- **On a first start the window opens in the middle of the screen, so on Windows it no longer runs off a 1280 x 1024 screen** (RPSN-0164)

- **STANDARDS.md names the right default download folders for the window app and the command line** (RPSN-0165)

### Security

- **Update ctutils past RUSTSEC-2026-0385.** (RPSN-0173)
  Update a small security library so the audit comes back clean.

- **Secure connections use the fixed rustls-webpki 0.103.16 and webpki-root-certs 1.0.10 (RUSTSEC-2026-0382, RUSTSEC-2026-0383)** (RPSN-0167)
