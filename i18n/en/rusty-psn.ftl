# rusty-psn: every text the window app shows, in English (RPSN-0072).
# Each id starts with the screen or part it belongs to; see docs/specs/RPSN-0072-translation-layer.md § 4.3.
# A translation is a copy of this file under i18n/<language>/ with the right-hand sides translated.

## Shared by several screens

common-cancel = Cancel
common-clear = Clear
common-filter = Filter:
common-download = Download
common-install-to-rpcs3 = Install to RPCS3
common-installing = Installing…
common-serial = Serial
common-title = Title
common-version = Version
common-size = Size
common-install-unchecked = RPCS3 finished installing { $name }, but rusty-psn could not check the result. Set the RPCS3 folder in Settings to check it.
common-open-folder-failed = Could not open the folder: { $reason }
common-no-clipboard = No clipboard is available.
common-copy-failed = Could not copy: { $reason }

## Sidebar and status bar

chrome-page-search = Search
chrome-page-downloads = Downloads
chrome-page-rpcs3-library = RPCS3 library
chrome-page-patches-and-dlc = Patches and DLC
chrome-page-firmware = Firmware
chrome-page-history = History
chrome-page-settings = Settings
chrome-status-downloads = Downloads: { $active } active, { $completed } completed, { $failed } failed, { $pending } queued, { $paused } paused
chrome-status-ready = Ready
chrome-status-eta = ETA: { $time }
chrome-status-batch = Batch: { $done }/{ $total } searched

## Messages panel

messages-title = Messages ({ $count })
messages-empty = No messages yet.
messages-level-note = Note
messages-level-done = Done
messages-level-warning = Warning
messages-level-error = Error

## Search page

search-title-serial = Title Serial:
search-paste = Paste
search-search = Search for updates
search-already-shown = Provided title id results already shown
search-batch-progress = Searching { $done }/{ $total }...
search-import-batch = Import Batch...
search-download-all = Download All
search-download-missing = Download Missing
search-download-missing-hint = Download only the updates not already in the download folder
search-download-all-latest = Download All Latest
search-export = Export results...
search-clear-results = Clear results
search-open-downloads = Open Downloads
search-text-files = Text files
search-error-psn-response = Unexpected error received in a response from PSN ({ $reason }).
search-error-invalid-serial = The provided serial didn't give any results, double-check your input.
search-error-no-updates = The provided serial doesn't have any available updates.
search-error-request = There was an error completing the request ({ $reason }).
search-error-xml = Error parsing response from Sony, try again later ({ $reason }).
search-error-manifest = Error parsing manifest response from Sony, try again later ({ $reason }).
search-error-security = Security check failed: { $reason }
search-error-too-large = Response too large ({ $size } bytes), possible server error.
search-batch-done-failed = Batch search complete: { $found } found, { $failed } failed.
search-batch-done = Batch search complete: { $found } title(s) found.
search-download-missing-done = Download Missing: { $queued } queued, { $present } already in the download folder.
search-batch-skipped = Batch import: skipped { $count } invalid serial(s)
search-batch-all-loaded = All serials from the batch file are already loaded.
search-batch-queued = Batch import: { $count } serial(s) queued for search.
search-batch-import-failed = Failed to import batch file: { $reason }

## Search results

results-filter-hint = Serial or title…
results-filter-tooltip = Show only results whose serial or title contains this text
results-clear-filter = Clear filter
results-sort-by = Sort by:
results-sort-search-order = Search Order
results-sort-serial = Serial
results-sort-title = Title
results-sort-update-count = Update Count
results-sort-total-size = Total Size
results-ascending-hint = Ascending — click to flip to descending
results-descending-hint = Descending — click to flip to ascending
results-count-one = { $total } result
results-count-other = { $total } results
results-showing = Showing { $visible } of { $total }
results-no-match = No results match the current filter.
results-update-count-one = { $count } update
results-update-count-other = { $count } updates
results-merge-needs-all = All parts need to be completed for merge to be available
results-merge-not-multipart = This PS4 update is not a multipart update
results-merging-parts = Merging parts…
results-parts-merged = Parts merged
results-parts-merge-failed = Parts merge failed
results-merge-parts = Merge parts
results-download-latest = Download latest
results-download-latest-hint = Queue only the most recent update
results-download-all = Download all
results-download-all-hint = Queue every update for this title
results-all-downloaded = All downloaded
results-done-failed = { $completed }/{ $total } done · { $failed } failed
results-failed-count = { $failed } failed
results-done-active = { $completed }/{ $total } done · { $active } active
results-downloading-count = { $count } downloading
results-paused-count = { $count } paused
results-done-count = { $completed }/{ $total } done
results-version = v{ $version }
results-latest = ★ Latest
results-offset = · offset { $offset }
results-needs-system = · needs system { $version }
results-queued = Queued
results-abort = Abort
results-pause = Pause
results-resuming = Resuming from { $offset }
results-downloading = Downloading
results-verifying = Verifying
results-resume = Resume
results-paused = Paused
results-open-folder = Open folder
results-installed-in-rpcs3 = Installed in RPCS3
results-completed = Completed
results-retry = Retry
results-failed-incomplete = Failed: file incomplete
results-failed = Failed
results-merge-failed = Merge failed
results-merged = Merged
results-merging = Merging…
results-details = Details
results-sha1 = SHA-1:
results-copy = Copy
results-source-url = Source URL:
results-copy-url = Copy URL
results-speed-eta = { $speed } - ETA { $eta }

## Downloads

downloads-done = { $serial } v{ $version } downloaded successfully!
downloads-retrying = { $serial } v{ $version }: Transfer interrupted, retrying ({ $attempt }/{ $max })...
downloads-hash-mismatch = Failed to download { $serial } v{ $version }: Hash mismatch.
downloads-short-hint = The file arrived smaller than expected, so Sony's servers were probably unreliable. Retry the download now, or wait a few hours and retry.
downloads-failed = Failed to download { $serial } v{ $version }: { $reason }
downloads-security-failed = Security check failed for { $serial } v{ $version }: { $reason }
downloads-incomplete = Failed to download { $serial } v{ $version }: ended at { $received } of { $expected } bytes
downloads-aborted = { $serial } v{ $version } aborted
downloads-paused = { $serial } v{ $version } paused
downloads-merged = { $serial } merged successfully!
downloads-merge-failed = Failed to merge { $serial }: { $reason }

## Downloads page

downloads-page-empty = Nothing downloaded yet this session. Search for a game and press Download on a package.
downloads-page-attention = Needs attention
downloads-page-in-progress = In progress
downloads-page-finished = Finished
downloads-page-section = { $name } ({ $count })
downloads-page-none = None.
downloads-page-retry-all = Retry all failed

## RPCS3 library page

rpcs3-library-detected = RPCS3 auto-detected and scan started.
rpcs3-library-not-found-scan = RPCS3 not found. Go to Settings to configure the RPCS3 path.
rpcs3-library-not-found-settings = RPCS3 not found. Please configure the path manually.
rpcs3-library-not-configured = RPCS3 not configured. Set the path in Settings.
rpcs3-library-scan-done = RPCS3 scan: { $count } game(s) found, checking { $searching } for updates...
rpcs3-library-scan-failed = RPCS3 scan failed: { $reason }
rpcs3-library-scan = Scan RPCS3 library
rpcs3-library-summary = { $games } game(s) in library, { $outdated } with updates available
rpcs3-library-update-all = Update All Outdated
rpcs3-library-show = Show:
rpcs3-library-installed = Installed
rpcs3-library-available = Available
rpcs3-library-status = Status
rpcs3-library-update-available = Update available
rpcs3-library-up-to-date = Up to date
rpcs3-library-no-updates = No updates
rpcs3-library-checking = Checking…
rpcs3-library-version-unknown = version unknown
rpcs3-library-install-done = Installed { $serial } to RPCS3
rpcs3-library-install-failed = RPCS3 install failed: { $reason }
rpcs3-library-filter-all = All
rpcs3-library-filter-needs-update = Needs update
rpcs3-library-filter-no-updates = No updates exist

## Update All Outdated

update-chain-download-only = Downloading only: set RPCS3's executable in Settings to install the updates too.
update-chain-download-failed = Not installing { $serial }: one of its updates failed to download.
update-chain-done = { $serial } { $title } is now at version { $version } in RPCS3.
update-chain-unchecked = RPCS3 finished installing the updates for { $serial }, but rusty-psn could not check them. Set the RPCS3 folder in Settings to check it.
update-chain-failed = Installing update { $version } of { $serial } failed, so later updates were not installed: { $reason }

## Patches and DLC page

content-browser-no-dir = No content directory configured. Set it on the Settings page, under Content Library.
content-browser-open-settings = Open Settings
content-browser-rescan = Rescan
content-browser-loading = Loading...
content-browser-shown = { $shown } game(s) shown (of { $total })
content-browser-patches = Patches ({ $count })
content-browser-dlc = DLC ({ $count })
content-browser-installed = Installed
content-browser-install = Install
content-browser-scan-stopped = The content folder scan stopped unexpectedly.
content-browser-install-done = Installed: { $name }
content-browser-install-failed = Install failed: { $reason }

## Firmware page

firmware-console = Console:
firmware-region = Region:
firmware-check = Check for Firmware
firmware-installed-version = Firmware installed in RPCS3: { $version }
firmware-none = none
firmware-hint = Select a console and region, then click "Check for Firmware".
firmware-type = Type
firmware-downloading = Downloading: { $name }
firmware-downloaded-unknown-size = { $done } downloaded ({ $speed }/s)
firmware-installed = Firmware installed in RPCS3.
firmware-install-unchecked = RPCS3 finished, but the firmware install could not be checked until the RPCS3 folder is set in Settings.
firmware-install-failed = Firmware install failed: { $reason }
firmware-check-failed = Firmware check failed: { $reason }
firmware-download-done = { $name } downloaded successfully!
firmware-download-cancelled = { $name } download cancelled.
firmware-download-failed = Firmware download failed: { $reason }
firmware-type-full = Full Update
firmware-type-patch = Incremental Patch
firmware-type-recovery = Recovery Image
firmware-type-system-data = System Data
firmware-type-pre-install = Pre-install Data

## History page

history-empty = No downloads completed yet.
history-count = { $count } download(s) completed
history-clear = Clear History
history-time = Time
history-ago-seconds = { $seconds }s ago
history-ago-minutes = { $minutes }m ago
history-ago-hours = { $hours }h { $minutes }m ago
history-ago-days = { $days }d ago

## Settings page

settings-save = Save changes
settings-undo = Undo changes
settings-restore-defaults = Restore defaults
settings-unsaved = You have unsaved changes
settings-all-saved = All changes saved
settings-migrate-heading = Migrate downloads
settings-about-heading = About rusty-psn
settings-downloads-heading = Downloads
settings-download-path = Download path:
settings-pick-folder = Pick folder
settings-reset = Reset
settings-folder-structure = Folder structure:
settings-max-downloads = Maximum concurrent downloads:
settings-appearance-heading = Appearance
settings-theme = Theme:
settings-theme-dark = Dark
settings-theme-light = Light
settings-theme-high-contrast = High contrast
settings-theme-rpcs3-blue = RPCS3 Blue
settings-text-size = Text size:
settings-text-size-normal = { $percent }% (normal)
settings-notifications-heading = Notifications
settings-show-notifications = Show system notifications
settings-rpcs3-heading = RPCS3 Integration
settings-rpcs3-config-dir = RPCS3 config directory:
settings-not-configured = Not configured
settings-browse = Browse
settings-auto-detect = Auto-detect
settings-rpcs3-config-found = RPCS3 config detected
settings-games-yml-missing = games.yml not found in this directory
settings-status-not-configured = Status: Not configured
settings-rpcs3-executable = RPCS3 executable:
settings-rpcs3-executable-unset = Not configured (auto-detected on scan)
settings-content-heading = Content Library
settings-content-dir = PS3 content directory (patches + DLC):

## Settings page: About and Migrate downloads

dialogs-version = rusty-psn version { $version }
dialogs-tagline = Download PlayStation game updates for use with emulators.
dialogs-github = GitHub Repository
dialogs-disclaimer = We do not condone piracy. rusty-psn only downloads official game updates and system software from Sony's own servers; it includes no games. Updates are only useful with a game you own. rusty-psn is not affiliated with or endorsed by Sony Interactive Entertainment. PlayStation is a trademark of Sony Interactive Entertainment.
dialogs-migration-done-skipped = Migration: { $migrated } folder(s) migrated, { $skipped } skipped. The log file says why each was skipped.
dialogs-migration-done = Migration complete: { $migrated } folder(s) migrated.
dialogs-migration-look = Look for folders to migrate
dialogs-migration-running = Migration in progress...
dialogs-migration-none = No folders found that need migration.
dialogs-migration-current-format = Current format: { $format }
dialogs-migration-found = Found { $count } folder(s) to migrate to "{ $format }" format:
dialogs-migrate = Migrate

## Folder layouts (the Files part's DirectoryFormat)

directory-format-serial-only = Serial only
directory-format-serial-title = Serial - Title
directory-format-title-serial = Title / Serial

## Results export

export-filter-csv = CSV (spreadsheet)
export-saved = Results saved to { $path }
export-failed = Could not save the results: { $reason }

## Start-up

main-network-failed = rusty-psn could not set up its network connection: { $reason }

## Shared parts' errors

error-batch-read = Failed to read batch file: { $reason }
error-batch-too-large = Batch file too large ({ $size } bytes, max { $max })
error-batch-no-serials = Batch file contains no valid serials
error-psn-tls = TLS setup failed: { $reason }
error-merge-failed = the parts could not be joined; the log says why
error-merge-not-partial = some packages for the update are not a partial package
error-merge-no-file-name = could not deduce filename from a package url
error-merge-bad-part-name = package name does not end with expected index and extension
error-firmware-network = Network error: { $reason }
error-firmware-parse = Parse error: { $reason }
error-firmware-xml = XML error: { $reason }
error-firmware-security = Security check failed: { $reason }
error-firmware-none = No firmware found for this region
error-firmware-too-large = Response too large ({ $size } bytes)
error-content-io = I/O error: { $reason }
error-content-zip = ZIP error: { $reason }
error-content-no-pkg = No .pkg file found in ZIP
error-content-too-large = ZIP unpacks to far more than its own size
error-content-duplicate = ZIP holds two files named { $name }
error-content-install = Install failed: { $reason }
error-content-unpacking-stopped = unpacking stopped: { $reason }
error-security-domain = URL points to a non-PlayStation domain: { $url }
error-security-path-escape = Path { $attempted } escapes the download directory { $boundary }
error-security-serial = Invalid serial: { $reason }
error-security-serial-too-long = input too long
error-security-serial-length = expected 9 characters, got { $length }
error-security-serial-characters = must contain only alphanumeric characters
error-rpcs3-not-found = RPCS3 installation not found
error-rpcs3-config-not-found = RPCS3 config file not found: { $path }
error-rpcs3-sfo = Failed to parse { $path }: { $reason }
error-rpcs3-yaml = Failed to parse RPCS3 config: { $reason }
error-rpcs3-install = RPCS3 installation failed: { $reason }
error-rpcs3-not-installed = RPCS3 finished, but { $serial } still reports version { $found }
error-rpcs3-not-installed-none = RPCS3 finished, but no installed version of { $serial } was found
error-rpcs3-firmware-not-installed = RPCS3 finished, but its firmware is { $found }, not { $expected }
error-rpcs3-firmware-not-installed-none = RPCS3 finished, but no installed firmware was found (expected { $expected })
error-rpcs3-already-running = RPCS3 is open. Close RPCS3, then install again
error-rpcs3-io = I/O error: { $reason }
error-rpcs3-pkg-missing = PKG path not found: { $path }
error-rpcs3-not-a-serial = not a game serial: { $serial }
error-rpcs3-flatpak-missing = RPCS3 Flatpak { $app } is not installed
error-rpcs3-exe-missing = RPCS3 executable not found: { $path }
error-rpcs3-launch-failed = failed to launch RPCS3: { $reason }
error-rpcs3-firmware-file-missing = firmware file not found: { $path }
error-rpcs3-games-yml-parse = failed to parse games.yml: { $reason }
error-rpcs3-games-yml-not-a-list = games.yml is not a list of games
error-rpcs3-cannot-read = cannot read { $name }: { $reason }
