# Validation

The current Windows build passes formatting, strict Clippy, and 81 default automated tests. The optimized MSVC executable embeds `requireAdministrator` with `uiAccess=false`. No driver or Npcap installation is required for the native capture path.

## Reproduce the checks

Run `./build.ps1` on Windows with Rust 1.95+ MSVC and Visual Studio C++ Build Tools. The script checks formatting, runs the default tests, runs strict all-target Clippy with the UI preview enabled, builds the release, and creates a ZIP plus SHA-256 checksum under `dist/`.

The default suite covers expanded period selection, custom duration/date validation, indefinite history across cleanup/reopen, date-filtered chart/summary/CSV parity, source isolation, TCP and UDP endpoint orientation, LAN/WAN classification, adapter selection, minute rollups, saved-history restoration, a nonblocking checkpoint with an active reader, transactional retries, CSV filters and escaping, app/type aggregation, top-five ranking parity with App traffic (including loopback and unattributed bytes), smooth curves without overshoot or gap bridging, window bounds, compact layouts, themes, contrast and dropdown interactions.

Seven live-machine probes are ignored by default. They read Windows startup state, Wi-Fi profiles, adapter/process metadata and a system executable icon, exercise worker shutdown/reopen with a temporary database, and query the configured DNS resolver. The final-event shutdown probe requires administrator access and generates local TCP traffic; an already running monitor may record it. Run individual ignored tests explicitly when appropriate. The generated-traffic capture test is separate: in an elevated terminal, run `cargo run -- --self-test-capture`. It checks attribution, addresses, ports, timestamps, exact TCP/UDP payload counts and event losses over IPv4 and IPv6. The optional `cargo run -- --self-test-public` makes a small HTTP request to 1.1.1.1:80 and checks TCP endpoints, adapter attribution, WAN classification and byte counts. Both capture self-tests passed on the development machine after the TCP receive fix. Diagnostic reports can contain local addresses; keep them local.

## UI review

The production egui layout is checked at supported window sizes, all four palettes and 80–110% interface size. Pointer-event tests cover navigation, app selection/clearing, period and network changes, and multi-adapter selection. An optional offscreen renderer produces sample-data previews without desktop capture or user history; see the README's UI development section. Repository screenshots use generated data.

## Measurement and validation limits

- Adapter counters and observed app transfers measure different layers. Summing virtual, physical and VPN adapters can count the same traffic more than once.
- Exact behavior still needs dedicated testing across Wi-Fi switches, VPN stacks, reconnects, sleep/resume, sustained high throughput and event-loss scenarios.
- Host names come from reverse DNS and may name shared infrastructure. Traffic types are inferred from ports, including for encrypted traffic.
- Country lookup requires a separately supplied database and remains approximate.
- Automated layout/interaction checks and offscreen renders do not replace manual testing of native save dialogs, tray behavior, administrator prompts or every DPI configuration.
- Existing histories from older builds can include unclassified traffic and legacy timestamp limitations. Storage checks detect corruption rather than silently replacing history; preserve affected database and journal files before recovery.

Traffic databases, journals, capture traces, exports, private local diagnostics and build output are excluded from Git. No live traffic files are part of the public repository or packaged app.

Custom period styling was reviewed through sample-data Slate and Light renders. Calendar interaction checks cover choosing a leap day without closing the parent dialog. The native startup definition/status probe passed without registering or executing a task; a real sign-in cycle has not been exercised.

Rolling and date-range modes share the same outlined input styling and fixed dialog body. Geometry checks confirm equal dialog size with and without validation messages in all four themes at 80% and 110% interface size.

Period menu rows use compact spacing. Month-to-date uses local calendar boundaries and is tested for month/year rollover, saved selection, and matching history, summaries and CSV. The redundant rolling end-time row is removed and the fixed dialog body is smaller.
