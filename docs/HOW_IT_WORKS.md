# How OpenNetworkMonitor measures traffic

Different sources measure different things, so read this before comparing figures.


## Overall Traffic (interface counters)

- Uses `GetIfTable2` interface-counter deltas, sampled about once per second.
- Does **not** use the Windows Settings → Data Usage store.
- Counts all protocols on each interface.
- Counters are baselined at startup, so traffic from before launch isn't imported. Resets and reconnects never create negative totals or spikes.

## Wi-Fi connections

- Grouped by network name (SSID), including the same name seen on several adapters.
- The Adapters menu lists networks seen while the app is running, plus an **Unidentified Wi-Fi** option for traffic that can't be assigned with confidence.
- Some traffic can stay unidentified: network switches between samples, Windows lookup failures, and very brief switches may not be detected.
- Traffic recorded before this feature existed counts in all-connection totals but can't be assigned to a specific network.
- Names are stored locally and kept in the catalog even after the matching history expires.

## App traffic (ETW)

- Uses built-in ETW kernel TCP/IP and UDP/IP send and receive events, through [FerrisETW](https://github.com/n4r1b/ferrisetw). Npcap isn't needed.
- Totals reflect observed TCP and UDP transport events. They aren't every link-layer byte. ACK, connect, and retransmit events aren't counted as separate usage.
- If capture can't start, the UI says why. Adapter traffic is still collected.
- Each event is attributed using its payload PID.
- Traffic is matched to a Wi-Fi network only when its local address matches a Wi-Fi adapter. VPN and other unmatched traffic may be left out of a named-network view.
- **Friendly names** come from Windows FileDescription and ProductName resources. Browsers keep their brand even when the binary is named `chrome.exe`.
- Process identities refresh every five seconds. A process that sends traffic between refreshes is named immediately while it's still running. A process that exits before lookup shows as `Process N`. PID reuse within one refresh interval is a known limitation.

## LAN and WAN

- **LAN** covers private, loopback, link-local, multicast, broadcast, and on-link destinations. **WAN** covers public destinations. Reserved and shared ranges are **Unknown**.
- This is address-based, not proof of a routing boundary. Public destinations routed through a private gateway still count as WAN.
- Adapter association comes from a unique local-address match. Ambiguous matches stay **Unassigned**.
- The LAN/WAN bar uses captured TCP/UDP app traffic, so its total is kept apart from interface-counter totals. In App traffic mode, **All adapters** includes loopback.

## Combining sources and VPNs

- The two sources are **never added together**.
- Selecting every adapter sums observations that can count the same traffic at several layers.
- VPN traffic can show up twice: as inner application traffic, and as outer encrypted traffic from the VPN process.
- There's no deduplicated all-PC internet total across arbitrary VPNs in this version.
- Tunnel adapters from VPN, bonding or connection-merging tools carry the same traffic as the physical adapters under them, so selecting all of them counts it twice.
- For clean numbers, select one physical adapter (its wire traffic), or a particular app or transport view. Validating the exact VPN stack is the next milestone.

## Sockets and hosts

- TCP tables show remote endpoints. UDP tables don't, so observed UDP destinations appear in app history while ETW is active.
- Socket inventory refreshes every five seconds, independent of your usage filters.
- Host names use reverse DNS on two background threads with bounded queues and caching. Lookups only cover hosts on screen. Shared hosting and CDNs can show a different name from the site you visited. IP addresses stay in subtitles and tooltips.
- Host lookups create ordinary DNS traffic. No separate IP lookup service is contacted.
- Traffic-type labels are port-based guesses, so encrypted DNS on HTTPS ports shows as HTTPS/TLS.
- Older history without recorded ports stays **Unclassified**, keeping its original byte counts.

## Known limits

- **TCP downloads recorded before the receive-endpoint fix** may have a wrong destination, adapter, network, or traffic type. The original remote endpoint wasn't saved, so those details can't be rebuilt. New traffic uses the corrected mapping.
- **Deleted history** from older versions can't be recovered.

## Time ranges and charts

- The chart follows the selected period: 15m, 30m, longer presets, *Since beginning of month*, *All time*, or a custom rolling duration or date range.
- Month-to-date starts at local midnight on the first day and updates when the month changes.
- Custom ranges use local time and include the full final minute. CSV timestamps stay UTC.
- Views up to 24 hours refresh every second. Longer and all-time views refresh every five seconds.
- Recent adapter samples are kept for 24 hours and survive restarts. Older data uses minute aggregates. Long ranges are downsampled for display.
- Missing samples show as gaps, never as invented zeros or lines across downtime. Measured zero traffic still shows as zero.
- **Freeze** pauses only the chart. Recording, range totals, and live speed keep going.
- Country lookups apply to newly captured traffic and are approximate.

## History tiers

Minute-by-minute history with every remote address is large: busy peer-to-peer and streaming apps add thousands of addresses a day. So old history is merged in steps, which you control in **Settings → Storage**.

| Age | What is kept | Per-host detail |
| --- | --- | --- |
| Newer than **Detailed history** (default 30 days) | One row per minute, app, adapter, remote address, protocol, type and Wi-Fi network | Yes |
| Older, merged into **hours** | One row per hour, app, adapter, protocol, type and Wi-Fi network | No |
| Older still, optionally merged into **days** (60 days, 180 days or 1 year) | The same, per local calendar day | No |

- **Totals never change.** Merging adds rows together, so every app, adapter and total figure, the charts and the data cap read the same before and after. A merged hour or day charts as an average rate across it.
- **Lifetime host totals.** Every remote address keeps a running total (bytes down and up, when first and last seen, country) in a small table that merging never touches. The Hosts card's **Lifetime** button lists them. They are for all traffic, so they ignore the period, adapter and app filters.
- **Merged history in the Hosts card** appears as one *Merged history* line, so the table still adds up.
- **Periods start on a bucket.** A period that begins in the middle of a merged hour or day leaves that hour or day out, as the chart already leaves out partial minutes.
- **Merging cannot be undone,** and nothing is deleted except the per-address rows it replaces. CSV exports carry a `bucket_seconds` column (60, 3600 or 86400) saying how wide each row's time bucket is.
- **When it runs.** A slice of at most a day is merged every couple of seconds until the history is caught up, then the app checks every ten minutes. **Compact now** does it all at once and shrinks the file. The file is also shrunk at launch when a third or more of it is unused.

## Storage reliability

- Captured traffic is **held in memory and saved to the history every 10 seconds while the window is open and every 30 seconds while the app is in the tray** (and when you pause, when you export, and on exit), not every second. Saving every second rewrote the same database pages over and over, which came to roughly 100 KB a second, around 9 GB a day, of disk writes; saving every 30 seconds is about nine times less (every 10 seconds about five times), because each save rewrites the database pages it touches once instead of every second.
- **Nothing lags.** Every chart, total, ranked list, per-app timeline, data-cap figure and lifetime host total reads the saved history and the not-yet-saved traffic together, so a figure is current to the second. A save moves the same bytes to the disk without changing any number, and a failed save leaves everything staged to try again.
- The one cost is that a power cut or crash loses the traffic since the last save, up to 10 seconds with the window open and 30 in the tray. Closing the app or pausing recording saves at once. So does Windows shutting down or signing you out: the app hears the end-of-session notice and saves before Windows terminates it.
- Each save writes adapter samples and usage totals together, using SQLite `FULL` synchronization.
- While the window is hidden in the tray, nothing is drawn, so the history is not read at all; it is read again the moment the window returns.
- Reading history keeps its sorting and grouping scratch space in memory. By default SQLite spills it to a temporary file, which wrote megabytes to the disk on every refresh of the Overview and App traffic pages.
- Because commits are infrequent, each reader's page cache (64 MB) stays valid between saves, so repeated refreshes are served from memory instead of rereading the file. Periods longer than two hours are charted from the minute rows rather than the per-second samples, which reads far less and charts the same shape.
- On a normal exit, the app stops capture, drains its buffers, saves the final partial sample, and runs a non-blocking checkpoint.
- If another program has the database open, committed data stays in the WAL until it can be checkpointed.
- Saved chart data loads before the first new live tick after reopening.
- Integrity is checked in the background at launch, and before any file-layout upgrade.
- A history that can't be opened is **never silently replaced**. The app offers to move the damaged files aside, keeping them, and start a fresh history.
- Chart and list reads use a separate read-only connection, so they never slow capture.
- Bundled SQLite version: 3.54.1.

## Sources

- [Microsoft: GetIfTable2](https://learn.microsoft.com/en-us/windows/win32/api/netioapi/nf-netioapi-getiftable2)
- [Microsoft: connected Wi-Fi network name](https://learn.microsoft.com/en-us/uwp/api/windows.networking.connectivity.wlanconnectionprofiledetails.getconnectedssid)
- [Microsoft: TCP/IP ETW events](https://learn.microsoft.com/en-us/windows/win32/etw/tcpip)
- [Microsoft: UDP IPv4 transfer events](https://learn.microsoft.com/en-us/windows/win32/etw/udpip-typegroup1)
- [Microsoft: GetExtendedTcpTable](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getextendedtcptable)
- [FerrisETW](https://github.com/n4r1b/ferrisetw)
- [egui](https://github.com/emilk/egui)
