# Roadmap

## Next: prove measurement behavior

- Validate normal traffic, downloads, streaming, UDP/QUIC and local LAN copies with controlled workloads.
- Validate the user's actual VPNs, full/split tunnels, reconnects and IPv6 separately. Record both interface and transport observations and known duplication.
- Add precise process lifecycle tracking to remove the remaining PID-reuse window inside the five-second refresh; processes that start between refreshes are now named when they first send traffic.
- Record capture coverage and downtime explicitly rather than plotting missing minutes as zero.
- Stress-test high throughput and event loss; aggregate earlier in the capture path if needed.
- Roll old minute history up into hourly and daily summaries, and record capture coverage in the history; the table is already compacted and keyed for period queries. Add long-running storage/recovery checks and detail pagination.

## After measurement validation

- A least-privilege Windows service/helper so capture can start at boot while the UI remains unelevated.
- Explicit measurement layers and VPN inner/outer views with evidence-based deduplication where possible. Never silently label a sum across layers as unique usage.
- Optional dynamically loaded Npcap backend for packet-level capture when native events are insufficient. It must remain an optional install and clearly report capture scope and interface limitations.
- Connection lifetimes, per-connection speeds and export of the live socket inventory. Custom date intervals, per-app graphs with daily and monthly views, and Connections protocol and status filters are done.
- User-editable theme files and accessible keyboard/density options.
- Code-signed release packaging, which needs a signing certificate. The application icon, start-at-login preference, installer scripts and the upgrade policy (a newer release replaces a running older one) are done. Until the executable is signed, Windows SmartScreen and the administrator prompt show an unknown publisher.

## Intentionally outside the current scope

Firewall/rules, blocking applications, deep packet inspection, packet-payload storage, cloud sync, subscriptions and online IP lookup services.
