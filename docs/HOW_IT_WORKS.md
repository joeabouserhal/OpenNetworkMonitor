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

## Storage reliability

- Adapter samples and usage totals are committed together, using SQLite `FULL` synchronization.
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
