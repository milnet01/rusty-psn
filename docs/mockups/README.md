# Approved window-app mockups

The four screens the owner approved on 2026-10-12 (RPSN-0133). RPSN-0183 and the items it lists build the app's look from them.

| Screen | Page | Picture |
|---|---|---|
| Search and results | `Main.dc.html` | `Main.png` |
| Downloads | `Downloads.dc.html` | `Downloads.png` |
| RPCS3 library | `Library.dc.html` | `Library.png` |
| Settings | `Settings.dc.html` | `Settings.png` |

Each page's `<style>` block holds the exact colours, sizes and spacing. The pages were designed at 1280×860.

The pictures were rendered offline with headless Chrome at that size. So they use a fallback sans font, not the IBM Plex fonts the pages name.

The pages load `./support.js`, the artifact viewer's runtime. It was never saved, and the pages render without it.

The files were rebuilt on 2026-11-09 from the log of the session that made them (RPSN-0183).
