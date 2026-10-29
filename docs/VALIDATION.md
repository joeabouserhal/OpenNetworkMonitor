# Validation

The current Windows build passes formatting, strict Clippy, and 72 default automated tests. The optimized MSVC executable embeds `requireAdministrator` with `uiAccess=false`. No driver or Npcap installation is required for the native capture path.

## Reproduce the checks

Run `./build.ps1` on Windows with Rust 1.95+ MSVC and Visual Studio C++ Build Tools. The script checks formatting, runs the default tests, runs strict all-target Clippy with the UI preview enabled, builds the release, and creates a ZIP plus SHA-256 checksum under `dist/`.

The default suite covers source isolation, TCP and UDP endpoint orientation, LAN/WAN classification, adapter selection, minute rollups, saved-history restoration, a nonblocking checkpoint with an active reader, transactional retries, CSV filters and escaping, app/type aggregation, top-five ranking parity with App traffic (including loopback and unattributed bytes), smooth curves without overshoot or gap bridging, window bounds, compact layouts, themes, contrast and dropdown interactions.

Six live-machine probes are ignored by default. They read Windows Wi-Fi profiles, adapter/process metadata and a system executable icon, exercise worker shutdown/reopen with a temporary database, and query the configured DNS resolver. The final-event shutdown probe requires administrator access and generates local TCP traffic; an already running monitor may record it. Run individual ignored tests explicitly when appropriate. The generated-traffic capture test is separate: in an elevated terminal, run `cargo run -- --self-test-capture`. It checks attribution, addresses, ports, timestamps, exact TCP/UDP payload counts and event losses over IPv4 and IPv6. The optional `cargo run -- --self-test-public` makes a small HTTP request to 1.1.1.1:80 and checks TCP endpoints, adapter attribution, WAN classification and byte counts. Both capture self-tests passed on the development machine after the TCP receive fix. Diagnostic reports can contain local addresses; keep them local.

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
