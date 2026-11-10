# Privacy and data

OpenNetworkMonitor is local-first. It has no account, no telemetry and no online service. Everything it records stays on your PC.

## What is stored, and where

All files are in `%LOCALAPPDATA%\OpenNetworkMonitor` for the Windows user who runs the app.

| File | Contents |
| --- | --- |
| `usage.db` (with `usage.db-wal` and `usage.db-shm` while the app runs) | The usage history, described below. |
| `preferences.ron` | Your settings: theme, filters, units, interface size, alert choices, the country-database path, window position and whether the welcome card was dismissed. |
| `alerts.json` | The last data-cap warning you were given this month, so a restart does not repeat it. |
| `running-build.txt` | A number that lets a newer build replace a running older one. |

The usage history keeps, for every minute: the network adapter, the application's executable path, its process ID, the remote IP address, the protocol and port-based traffic type (for example HTTPS or DNS), whether the destination is local (LAN) or public (WAN), the country if you supplied a country database, the name of the Wi-Fi network (SSID) when one was connected, and the number of bytes sent and received. It also keeps a list of the adapters and Wi-Fi network names it has seen, and per-second adapter counters for the last 24 hours.

It does **not** store packet contents, web addresses, page titles, DNS query names or the host names shown in the lists. Host names are looked up when displayed and are only held in memory.

Older history is merged on a schedule you choose in Settings → Storage: after 30 days by default, per-minute rows become hourly totals per app and adapter, and optionally daily ones later. Merging drops the process ID, remote address and country from those rows. Each remote IP address keeps one running total (bytes sent and received, first and last time seen, country) so lifetime per-host usage stays available. Nothing is ever deleted otherwise; history has no expiry and is removed only when you delete it.

## What leaves your PC

- **Reverse DNS.** To show host names, the app asks your Windows-configured DNS resolver for the name of the remote IP addresses that are on screen. That sends those IP addresses to your resolver, as any lookup would. No other service is contacted.
- **Nothing else.** Country lookup uses a database file you provide and works offline. There are no update checks, crash reports or analytics.

## What the app changes on your system

- A short-lived Windows event-tracing session named `OpenNetworkMonitor-<process id>-<time>` while it runs, which is how per-application traffic is observed. It is stopped when the app exits. If the app is killed, the next start stops any such session whose process is gone.
- Optional, only if you turn them on in Settings:
  - A scheduled task named `OpenNetworkMonitor Startup - <your account id>` that launches the app when you sign in.
  - Toast notifications for alerts. Windows needs an identity for them, which the app registers for your user under `HKCU\Software\Classes\AppUserModelId\OpenNetworkMonitor.App`.
- CSV files you export, in the place you choose.

## Deleting your data

Exit the app from its tray icon, then delete the `%LOCALAPPDATA%\OpenNetworkMonitor` folder. The uninstall script can do the same with `-RemoveData`; see [UNINSTALL.md](UNINSTALL.md). Exported CSV files are yours and are not touched.

The CSV export contains application paths, remote IP addresses and Wi-Fi network names. Review it before sharing.
