# Versioning overrides — loop log

Review record for `docs/standards/versioning-overrides.md`.

## Cold-eyes loop log

| Loop | Date | Lanes | Q1 | Q2 | Q3 | Q4 | Outcome |
|------|------|-------|----|----|----|----|---------|
| 1 | 2026-10-23 | 2 | 1 | 1 | 2 | 0 | Both lanes held every question. 4 findings, 3 verified / 1 dismissed; all 3 fixed. Q1 (both lanes): window position credited to `UpdatesApp::save`; eframe stores size and position, in a folder named from the `run_native` app name. Q3 (one lane): default layout and folder not protected. Q3 (from both lanes' open questions): batch format narrower than the parser accepts. Q2 dismissed: 1.0.0 is stated outright and `cut-release` takes an explicit version. Two open questions resolved clean (Enter key, which build's `--help`). Loop 2 dispatched. |
| 2 | 2026-10-23 | 2 | 1 | 0 | 0 | 0 | Both lanes held every question; neither filed a finding. 1 verified from both lanes' open questions, fixed: shortcuts named Ctrl only, while the code uses egui's `command` (Cmd on macOS) and CI builds macOS. Previous-loop share 0 of 1. Four open questions resolved clean (Flatpak storage, Enter key, batch blank lines and size cap, `cut-release` taking an explicit 1.0.0). Loop 3 dispatched. |
| 3 | 2026-10-23 | 2 | 0 | 0 | 2 | 0 | Both lanes held every question. 2 findings, both verified: 1 fixed, 1 surfaced to the owner. Fixed (Q3): the saved-state folder follows the viewport `app_id` first, and a `persistence_path` overrides both. Surfaced (Q3): the list is silent on whether the command line's printed output is a surface. Previous-loop share 1 of 2 (the fix landed on loop 1 text). **Capped** at the standard cap of 3; calm cap: the own-fix finding narrowed a loop-1 claim rather than repairing a repair. This loop's 1 fix is read by no lane. No second share: the whole document is the armed change. |
