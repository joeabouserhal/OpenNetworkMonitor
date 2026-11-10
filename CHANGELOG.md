# Changelog

Each release's notes on GitHub are taken from the section for its version below. To ship a
release, add a section for the new version here, bump the version in `Cargo.toml` and
`app.manifest`, and push to `main`.

## 0.6.0

### Added

- Storage section in Settings: disk space used, a button to open the data folder, and the program location.
- Old history is merged into hourly (and optionally daily) totals, one month at a time. Byte totals are kept. "Compact now" does it immediately.
- Hosts keep lifetime totals, with a Lifetime view in the Hosts table.
- `onm.log` records what the app does. After a crash it holds the details, the unsaved traffic is saved, and the next start tells you.
- Icons on the Settings cards.

### Changed

- Far fewer disk writes: traffic is held in memory and saved every 5 minutes, on tray show/hide, pause, export, exit and Windows shutdown. Everything on screen stays live.
- New installs open at 1000x660.

### Fixed

- Merging respects local months and daylight saving.
- A tray menu that could stay stuck open, and a few leaks.

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
