# Roadmap

## Next: prove measurement behavior

- Validate normal traffic, downloads, streaming, UDP/QUIC and local LAN copies with controlled workloads.
- Validate the user's actual VPNs, full/split tunnels, reconnects and IPv6 separately. Record both interface and transport observations and known duplication.
- Add precise process lifecycle tracking to remove the five-second process-name/PID reuse window.
- Record capture coverage and downtime explicitly rather than plotting missing minutes as zero.
- Stress-test high throughput and event loss; aggregate earlier in the capture path if needed.
- Add database migrations, long-running storage/recovery checks, detail pagination and query indexes based on measured workloads.

## After measurement validation

- A least-privilege Windows service/helper so capture can start at boot while the UI remains unelevated.
- Explicit measurement layers and VPN inner/outer views with evidence-based deduplication where possible. Never silently label a sum across layers as unique usage.
- Optional dynamically loaded Npcap backend for packet-level capture when native events are insufficient. It must remain an optional install and clearly report capture scope and interface limitations.
- Custom date intervals, daily/monthly summaries, per-app graphs, connection lifetimes, protocol filters and export of live socket inventory.
- User-editable theme files and accessible keyboard/density options.
- Signed portable/release packaging, application icon resource, start-at-login preference and upgrade policy.

## Intentionally outside the current scope

Firewall/rules, blocking applications, deep packet inspection, packet-payload storage, cloud sync, subscriptions and online IP lookup services.
