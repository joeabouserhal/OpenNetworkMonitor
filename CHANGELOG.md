# Changelog

Each release's notes on GitHub are taken from the section for its version below. To ship a
release, add a section for the new version here, bump the version in `Cargo.toml` and
`app.manifest`, and push to `main`.

## 0.6.0

Far fewer disk writes, a Storage section in Settings, and a log that records crashes.

### Added

- **Storage in Settings.** Shows how much disk space the app uses (history, settings), where its
  data folder is, with a button to open it, and where the program file is. The card is
  collapsible and starts closed.
- **Merging old history.** Minute-by-minute history older than your chosen time (7 or 14 days,
  30 days by default, 90 days, or never) is merged into hourly totals, one calendar month at a
  time, so it happens once a month rather than every day. An optional second step merges hourly
  totals into daily ones after 60 days, 180 days or a year. Old data is merged, not deleted:
  every byte total is kept. **Compact now** merges everything older than the setting straight
  away and shrinks the file. On a real 141 MB history this took it to 102 MB.
- **Lifetime totals per host.** Hosts keep their all-time totals however far the detail has been
  merged, and the Hosts table has a Lifetime view.
- **A log and crash handling.** `onm.log` in the data folder records what the app does (start and
  exit, opening the history, capture, merging, failures). If the app crashes, or a Windows
  exception such as an access violation happens, the details are written to the log and the
  traffic still waiting in memory is saved. The next start tells you the last session ended
  unexpectedly. If the monitoring thread itself fails, the window says so instead of silently
  recording nothing.
- **Icons on the Settings cards**, centred on the title and description.

### Changed

- **Much lighter on the disk.** The app used to write constantly, which showed as 2 to 8 MB/s of
  I/O and spikes with every refresh. Traffic is now held in memory and saved to the history every
  5 minutes, and also whenever the window is shown or hidden from the tray, on pause, on export,
  when you exit, and when Windows shuts down or you sign out. Everything on screen stays live,
  because it reads the in-memory traffic together with what is saved. Measured writes fell from
  about 700 KB/s with 10 MB bursts to a few KB/s.
- **Smaller window on first launch.** New installs open at 1000x660. A window saved at an older
  size takes the new size once, and after that keeps whatever size you set.
- **Lighter to run.** The Recently used list no longer scans the whole history, and the app's
  saved preferences shrank from 64 KB to under 1 KB.
- The Storage card's header shows how much is used only while it is closed.

### Fixed

- Merging and history queries use local calendar days and months, including across daylight
  saving changes.
- Very large retention settings no longer overflow, and an interrupted save keeps what was waiting
  and does not count it twice when it is retried.
- A tray menu that could stay stuck open, and several resource leaks, found in a review of the
  new storage code.

## 0.5.2

### Added

- A tray icon colour setting: the app's teal, the accent colour, or monochrome.
- A redrawn app icon with large bars, and a title bar icon that is just the bars in one colour.

### Changed

- The accent colour is used in more places, and the accent bar beside the selected sidebar page is
  gone.
- The Overview stat tiles are shorter, and the Total usage tile shows total download and upload.

## 0.5.1

### Fixed

- The tray menu is drawn as a native popup placed beside the icon. It no longer closes after a few
  seconds while the window is open, and no longer crashes the app when it is running only in the
  tray.

### Changed

- Launching at sign-in can accept the folder risk from the warning, the top apps rows fit without
  a scrollbar, and the live speed tile is gone from the sidebar.

## 0.5.0

- A new interface in a shadcn style, with light and dark themes and an accent colour.
- The app is a single executable, and releases are published from CI.
