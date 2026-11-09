# Building and developing

## Build

You need Windows, Rust 1.95+ (MSVC toolchain), and Visual Studio C++ Build Tools.

From the project folder, run one of:

```powershell
.\build.ps1
# or, from Command Prompt:
.\build.cmd
```

The script checks formatting, runs tests and strict Clippy, builds a release, and puts the single `OpenNetworkMonitor.exe` in `dist/`.

**Handy options**

| Command | What it does |
| --- | --- |
| `.\build.ps1 -SkipChecks` | Rebuild quickly once checks have already passed |
| `.\build.ps1 -Configuration Debug` | Build a debug executable (`dist/OpenNetworkMonitor-debug.exe`) |

The script only writes to project-local folders. It doesn't install toolchains or change system settings. Compiling doesn't need administrator access.

<details>
<summary>Using Cargo directly</summary>

```powershell
cargo build --locked --release
.\target\release\OpenNetworkMonitor.exe
```

</details>

## Running it

- **Launch it from Explorer.** The executable asks for administrator approval automatically, because per-application monitoring needs that access. Use an elevated terminal only for `cargo run` or command-line diagnostics.
- **Only one copy runs at a time.** If you open a newer build while an older one is running, the older copy is asked to save and exit, and the new one takes over. If the older copy predates this hand-over, exit it from the tray first.
- **Closing the window hides it to the tray.** Left-click the tray icon to reopen. Right-click → **Exit** saves final data and stops the app. If the tray can't be created, closing the window exits instead.
- **The process must keep running to record traffic.**
- **No driver, service, or firewall rule is installed.** Optional launch at sign-in lives in **Settings → Background monitoring** (see the [README](../README.md)). For the installer script, see [UNINSTALL.md](UNINSTALL.md).

## Diagnostics and verification

```powershell
cargo fmt --check
cargo test
cargo clippy --locked --all-targets --features ui-preview -- -D warnings
cargo run -- --diagnose 8
# In an elevated terminal:
cargo run -- --self-test-capture
# Optional: a small HTTP request to 1.1.1.1:80
cargo run -- --self-test-public
```

| Command | What it checks |
| --- | --- |
| `--diagnose` | Adapter inventory, connection and process counts, per-protocol events, byte deltas, and loss counters |
| `--self-test-capture` | Known local TCP and UDP payloads over IPv4 and IPv6, checking delivery and endpoint attribution |
| `--self-test-public` | A small fixed HTTP request to 1.1.1.1:80, checking TCP endpoints, adapter, WAN classification, and byte counts |

The diagnostic commands don't write usage history, though a running monitor can record their traffic. None of them change network settings or install anything.

Build and test status, including anything that couldn't be completed, is recorded in [docs/VALIDATION.md](docs/VALIDATION.md).

## UI development

The preview uses generated sample data. It has no settings, capture, tray, or database access.

```powershell
cargo run --example ui_preview --features ui-preview -- apps --small
# Other screens: overview, connections, adapters, settings. Add --light for Light.
```

For an offline review of the real egui layout, with no desktop input:

```powershell
cargo test --features ui-preview export_ui_review -- --ignored
python scripts/render_ui.py
```

The renderer needs NumPy and Pillow, and writes PNGs to the git-ignored `target/ui-review` folder. Default builds don't include the preview feature.
