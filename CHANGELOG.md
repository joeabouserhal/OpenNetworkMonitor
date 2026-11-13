# Changelog

Each release's notes on GitHub are taken from the section for its version below. To ship a
release, add a section for the new version here, bump the version in `Cargo.toml` and
`app.manifest`, and push to `main`.

## 0.6.1

### Added

- Live upload and download speed for every app: a small chip after the name in Top apps, the speeds in Recently used, and a speed line in each row of the App traffic Apps table.

### Changed

- Recently used is sorted by current speed, busiest first. Apps that are idle follow, most recent first, with a note such as "6 min ago".
- The Apps table on the App traffic page is a little wider than the Hosts and Traffic type tables.

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

### Changed

- The accent colour is used in more places: hover and selected backgrounds, the active tab and some icons.
- The app icon is large bars on a borderless dark tile, and the title bar shows just the bars in one colour from launch.
- The README is shorter, with the icon and new screenshots.

## 0.5.1

### Added

- Tray icon colour setting: the app's teal, the accent colour, or monochrome (white on a dark taskbar, dark gray on a light one).
- A redrawn app icon with large bars.
- You can accept the risk of launching at sign-in from a folder that others can change, with a checkbox.

### Changed

- The Overview stat tiles are shorter, and the Total usage tile shows total download and upload.
- The sidebar starts with navigation: the live speed tile is gone, since Overview shows it.
- The Top apps rows fit without a scrollbar.

### Fixed

- The tray menu is a native popup drawn in the app's colours, placed beside the icon. It no longer closes after a few seconds while the window is open, and no longer crashes the app when it is running only in the tray.

## 0.5.0

### Changed

- A new interface in a shadcn style: Light, Dark or follow Windows, ten accent colours, the Inter font, a sidebar, cards and tables. Pages keep one layout at every window size and only lists scroll.
- Overview gets a compact Top apps card with a Recently used tab, and a bar per app split into download and upload.
- The sidebar shows live speed.
- Releases are built and published by CI when a new version is pushed to `main`.

## 0.4.1

The first published release, with everything in 0.4.0.

### Added

- Per-app network monitoring on Windows, with history kept in a local SQLite database.
- Pages for Overview, App traffic, Connections, Adapters and Settings.
- A history chart for each app, over time, daily (30 days) and monthly (12 months).
- Alerts for a newly seen app and for a monthly data cap (80% and 100%), as Windows notifications and an in-window banner.
- Connections: sortable columns, protocol and status filters, host names, and a right-click menu (copy, show only this app, open location, end process).
- Custom history periods, CSV export, and an option to start at sign-in.
- Match Windows appearance, keyboard shortcuts (Ctrl+1 to 4, Ctrl+F, Ctrl+E, Esc), screen-reader labels, and a title bar coloured to match the theme.
- Tray controls: live speed in the tooltip, Open Settings, and Pause recording.
- A welcome card on first run, and the app icon.
- Installer and uninstaller scripts.
- A newer build replaces a running copy automatically.

### Changed

- A smaller history database: a compact layout with no separate time index.
- The release is the single `OpenNetworkMonitor.exe`.

### Fixed

- A window that was minimized when the app exited no longer opens tiny next time.
- Choosing Exit from the tray no longer stalls while the window is unfocused.
- Slow history queries no longer stall the capture, and file dialogs no longer block exit.
- The app refuses to start at sign-in with administrator rights from a folder that others can change.
- A damaged history database can be moved aside instead of stopping the app from starting.
- The startup location warning is readable.
