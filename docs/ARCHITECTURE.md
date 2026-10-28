# Architecture

The desktop executable has a single collector thread and an egui/eframe UI thread. ETW consumes events on its library-owned callback thread. Windows native calls are isolated in `src/capture/`; every unsafe boundary documents allocation, layout, lifetime or handle ownership.

- `model.rs`: shared records, filter matching, counter reset handling, scope classification, adapter attribution, formatting.
- `capture/identity.rs`: cached, local executable version-resource display names keyed by stable full paths.
- `capture/windows.rs`: IP Helper adapter counters/addresses, IPv4/IPv6 owner-PID socket tables, process identity and single-instance handle.
- `capture/etw.rs`: uniquely named, app-owned ETW session; TCP and UDP providers; bounded nonblocking queue; decode/drop/kernel-loss counters. Capture never stops another program's trace session.
- `worker.rs`: sampling, endpoint aggregation, offline geolocation, SQLite writes, filter queries, export and retention. Rendering can stop while collection continues.
- `store.rs`: SQLite WAL mode, transactional minute upserts, gap-aware chart series, filtered export with formula escaping. Failed writes are retained for retry up to a bounded row limit, after which a persistent warning reports that history stopped.
- `graph.rs`: custom egui plot with direction toggles, log scale, time labels, hover and drag zoom.
- `tray.rs`: native tray icon/menu, direct Exit and UI wake-up.
- `app.rs`: screens, themes, view state and persisted preferences.
- `main.rs`: startup, local data path, diagnostics and generated-traffic verification.

## Measurement contract

`source=adapter` means interface bytes; `source=transport` means observed TCP/UDP event bytes. Each database key includes the source. Queries choose exactly one source. Never add source totals, label transport bytes as link-layer bytes, or infer a country's location without a successful local database lookup.

Transport records include an adapter identity only when the event's local IP matches exactly one current adapter. No fallback attributes arbitrary events to the default adapter. Adapter selection affects app records via this inference and must retain an Unassigned option. Interface identity uses a GUID rather than the volatile interface index.

ETW uses payload PID rather than event-header PID. Only data send/receive opcodes 10/11 (IPv4) and 26/27 (IPv6) are counted. UDP has its own provider GUID even though the kernel enable flag is shared with TCP. Event timestamps preserve the original capture minute rather than the callback-delivery minute.

## Storage and resource limits

A bundled SQLite engine avoids an external database dependency. Samples are committed each second into minute buckets, preserving source/adapter/application/PID/remote IP/protocol/scope/traffic type. Country is a descriptive field, never part of the byte identity. Default history retention is 30 days; pruning runs hourly. Database files and optional country databases are excluded from Git.

The ETW queue holds at most 65,536 events, and a tick drains at most that many. The country cache holds at most 4,096 addresses. Only current-interval events and current socket inventory are kept in the live snapshot. History query results are bounded to the selected retention window, with at most roughly 600 plotted time buckets. Detailed endpoint history can grow with traffic volume and needs pagination in a future version.

UI preferences use eframe's local RON storage in the app data directory. There is no telemetry or online country lookup client. Host names are resolved by native GetNameInfoW through the configured Windows resolver; this can generate DNS traffic.

## v0.2 overview and sampling

The Overview source switch is a pair of buttons. Adapter mode draws a bounded 301-sample one-second live ring, independent from persisted minute averages. App mode and Applications share compact ranked lists. Application aggregation is cached by data generation/filter/search. Historical rows use Arc ownership, so rendering does not repeatedly clone the full endpoint history. LAN/WAN scope sums are queried directly in SQL instead of materializing endpoint rows.

Visible adapter membership uses present SetupAPI network-class devices whose NCF_HIDDEN flag is clear. The full internal interface inventory remains available to associate ETW local addresses. A visible_adapters SQLite catalog remembers non-hidden installed and previously observed adapter IDs, preserving all-adapter historical totals after device removal while keeping the current inventory accurate. User selection excludes hidden legacy samples. Database schema user_version is now 2.

The desktop binary embeds requireAdministrator; unit tests live in src/lib.rs and remain unelevated. The MSVC build fails explicitly for an unsupported Windows linker environment instead of silently dropping the administrator requirement.

Capture timestamps are converted directly from the original Windows UTC FILETIME (`raw.div_euclid(10_000_000) - 11_644_473_600`). FerrisETW 1.2.0's optional typed timestamp conversion discards the low DWORD, so the time_rs feature is not used. The conversion is covered by a fixed-vector test.

## v0.2.1 period-linked durable chart

Schema version 3 adds adapter_samples, keyed by timestamp and adapter GUID, with received/sent deltas and elapsed duration. Recent raw samples retain 24 hours of history; minute totals keep the existing 30-day retention. The final partial sample can share a timestamp with the prior tick, so its deltas and elapsed duration are added atomically. Each capture transaction commits the minute-level usage and raw samples together; pending data remains for retry if the transaction fails.

History includes explicit requested window_start/window_end and Point.observed. The chart never derives its time domain from the first or last observed point. Raw samples provide recent rates and minute aggregates cover older legacy history; missing intervals insert gap markers. No zero-fill or independent Live/History mode remains. The UI lays out the adapter view directly rather than inside a scroll area and allocates the plot from the remaining height after reserving the compact summary row.

The ETW consumer is owned rather than detached. Shutdown stops only the app-owned named session, lets ProcessTrace finish delivering buffered events, joins the consumer, drains the event queue, writes the final sample and checkpoints. FULL synchronization and the current SQLite engine protect committed data; integrity_check failures are surfaced instead of looking like empty usage.

## v0.2.2 application lists

Apps, Hosts and Traffic type share the selected history period and adapters. All lists rank combined received/sent bytes. Selecting an application narrows the adjacent lists without hiding the app ranking. Each list virtualizes rows and scrolls independently; there is no outer page scroll. App directories are visible subtitles; fallback names preserve the complete executable filename. Stable row identities follow the app path or grouped host/service when ranks and scroll positions change.

ETW sport/dport values are converted from network byte order. Missing port metadata retains byte events as Unclassified. The inferred type is part of the database key so traffic to the same IP/app over HTTP and HTTPS never merges into one type. Schema version 4 migrates older traffic atomically, preserving old records with an empty type (displayed Unclassified). CSV includes traffic_type.

hosts.rs uses two native Windows resolver workers, queues of 128 and a cache capped at 2048 addresses (15 minute positive and 2 minute negative TTLs). Neither capture nor UI waits for DNS. Slow native lookups are detached on exit; they own no history handles and cannot delay the final capture flush. No hostname is invented when reverse DNS is unavailable, and a resolved infrastructure name is only a hint. traffic.rs groups names/IPs and service types without duplicating bytes, respecting the app search and selection. No payloads are inspected or decrypted.

## v0.3 UI consistency

Only Overview, Connections, Adapters and Settings are navigation destinations. Overview retains the two direct Adapter traffic / App traffic buttons. A shared draw method renders the same shell in production, headless layout checks and the opt-in sample-data preview. Native monitoring, persistence and tray shutdown keep their existing ownership.

widgets.rs centralizes panel borders/margins, badges, text placement, focus-visible navigation and integer column geometry. Three-column layouts allocate exact widths rather than multiplying fractional minimum widths; this avoids border expansion at the minimum window size. App rows use 15.5-point names, 17-point totals, 13.5-point directory subtitles and separately labeled/color-coded Down/Up values. Hover details retain full paths and current rates. Keyboard focus is visible on navigation and app rows.

Connections virtualizes a fixed six-column table; PID/folder information sits below the app name. Long endpoints truncate within their own cells and remain available in tooltips. Adapters and Settings each own a vertical scroll region. Adapter status/action slots have fixed widths, and View traffic selects that adapter in Overview. Settings uses the same full-width surfaces, with measurement detail collapsed initially. CSV remains tied to the Overview context, so its button appears in Overview and the Settings history section.

The ui-preview feature and examples/ui_preview.rs are development-only fixtures without capture or database access. The ignored export_ui_review test exports actual tessellated meshes and texture atlases. scripts/render_ui.py rasterizes these offscreen using NumPy/Pillow; it does not capture or control desktop windows.

## v0.3.1 compact Overall Traffic

The adapter-counter view is labeled Overall Traffic; persisted source keys and history semantics remain unchanged. Settings gains ui_scale (default 0.9, bounded 0.8–1.1). Existing preferences adopt the compact default through serde defaults. The context's native zoom factor applies the same size to every page, and the Appearance slider persists changes. Non-finite or out-of-range saved values are normalized at startup.

A single 114-point summary replaces the three 166-point cards. Upload/download share one side, with one period label; the larger right side shows captured app bytes as a horizontal LAN/WAN/Other bar and individual amounts/percentages. A divider and an explicit captured label preserve the measurement distinction. Percentages below 0.1% remain visible as such. The chart receives the height saved by the summary.

Offscreen review exports multiply egui geometry and clipping rectangles by pixels_per_point, so the PNGs reflect interface zoom rather than treating logical coordinates as physical pixels. Native monitoring, byte accounting, saved history and exit handling are unchanged.

## v0.3.2 top apps in Overall Traffic

The footer has responsive 26/34/40 percent sections for totals, destinations and the five busiest apps. It removes the repeated Usage/period row. A 104-point content area fits five dense app rows, with full executable paths and down/up breakdowns on hover. Totals stack vertically at narrower widths and sit beside each other when space permits. Empty history has an explicit empty state.

Store::transport_summary groups transport bytes by adapter, executable path and scope in a single SQL scan. It derives both the destination split and app ranking, merges processes/endpoints/types by path, sorts combined down/up and retains five entries. It uses the same minute-bounded period and effective adapter membership as the existing captured-traffic summary; app-page searches do not restrict Overall Traffic. Shared holds the small top_apps vector, refreshes it with history, and clears it on filter changes. Friendly metadata includes paths from the summary, including saved applications that are no longer running.

The default viewport is 1440x820 points. A persisted wide_summary_initialized preference widens an older saved window once, preserving its height and respecting monitor width. Maximized windows are retained, and subsequent manual window resizing continues to persist normally. Native capture, database schema and source-accounting boundaries remain unchanged.

## v0.3.3 dense app traffic

App traffic has one bounded, non-wrapping filter bar: search, period, adapters and network scope. Each dropdown owns a stable ID. The adapter popup uses CloseOnClickOutside to support several checkbox changes before dismissal; the same adapter-choice function serves the Overall Traffic adapter chooser. Filter changes refresh history once, retaining an app selection while replacement data is pending.

The Overview subtitle and standing selection hint are removed. Clear selection moves to All apps in the Apps heading, with a fixed heading height shared by all columns. App/host/type rows shrink from 78 to 56 points. Primary labels/totals stay prominent; directories and other subtitles are 11.5 points, as are Down/Up details. All lists continue to virtualize and scroll independently. Paths and full values remain in hover details. Overall Traffic retains its existing quick period controls.
