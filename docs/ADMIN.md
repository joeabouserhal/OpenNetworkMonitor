# Administrator launch

The Windows executable embeds `app.manifest` with `requestedExecutionLevel="requireAdministrator"` and `uiAccess="false"`. Launching a debug or release build through Explorer requests Windows administrator approval when the launching process is not already elevated. The app starts with the access needed for native per-application capture after that approval. Windows still controls its approval prompt; the app does not bypass or disable it.

`build.rs` uses the existing MSVC linker, without an extra Rust build dependency. `/MANIFEST:EMBED,ID=1` embeds the manifest as the executable's RT_MANIFEST resource. `/MANIFESTUAC:NO` suppresses only the linker's automatically generated `asInvoker` fragment; the explicit administrator fragment from `/MANIFESTINPUT:app.manifest` is retained when the inputs are merged. Non-MSVC Windows builds fail clearly rather than producing an executable without its administrator requirement.

The linker directives apply only to `OpenNetworkMonitor`. Unit tests are in the library target, and the binary has `test = false`; this is necessary because Cargo applies binary-specific linker arguments to a binary unit-test harness too. `cargo test` therefore runs its library tests without requesting administrator access.

A normal unelevated terminal cannot directly create a process that requires elevation. For development, either use an administrator terminal for `cargo run`, or build normally and open the resulting `.exe` through Explorer. The same applies to command-line diagnostics: run them from an administrator terminal. Compilation itself does not need Windows administrator access.

## Verify the built manifest

The Windows SDK's manifest tool reads the embedded resource without running the app. Adjust the SDK path if a different version is installed:

```powershell
$manifestTool = 'C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\mt.exe'
& $manifestTool '-inputresource:target\release\OpenNetworkMonitor.exe;#1' '-out:target\release\embedded-app.manifest'
Select-String -LiteralPath target\release\embedded-app.manifest -Pattern 'requestedExecutionLevel'
```

Expect exactly one `requestedExecutionLevel` with `level="requireAdministrator"` and `uiAccess="false"`. The same extraction can be performed against the debug binary. `mt.exe` is used only for verification, not by the build script.

Sources: [Microsoft MANIFEST](https://learn.microsoft.com/en-us/cpp/build/reference/manifest-create-side-by-side-assembly-manifest?view=msvc-170), [MANIFESTINPUT](https://learn.microsoft.com/en-us/cpp/build/reference/manifestinput-specify-manifest-input?view=msvc-170), [MANIFESTUAC](https://learn.microsoft.com/en-us/cpp/build/reference/manifestuac-embeds-uac-information-in-manifest?view=msvc-170), and [Cargo build-script linker directives](https://doc.rust-lang.org/cargo/reference/build-scripts.html#outputs-of-the-build-script).

## Local validation

The source manifest passes Windows SDK `mt.exe -validate_manifest`. A standalone Rust/MSVC smoke executable was linked using the same three linker options and inspected without launching it: its embedded RT_MANIFEST ID 1 contained exactly one `requestedExecutionLevel`, with `level="requireAdministrator"` and `uiAccess="false"`, and no `asInvoker` fragment. Generated smoke files stay in the ignored `target/` directory.

The final `target/release/OpenNetworkMonitor.exe` version 0.2.0 was also inspected directly on 2026-10-28 with the Windows SDK manifest tool. Its embedded RT_MANIFEST resource ID 1 has assembly version `0.2.0.0`, exactly one `requestedExecutionLevel` set to `requireAdministrator` with `uiAccess="false"`, and no conflicting `asInvoker` fragment. The extracted verification copy is `target/release/embedded-app.manifest`. Verification did not launch the executable or interact with Windows approval prompts.

## Optional launch at sign-in

Settings → Background monitoring → Launch when I sign in to Windows is off by default. Enabling it registers one logon task for the current Windows user, using an interactive token and HighestAvailable run level. The app must already be elevated to configure this task. No password is stored. It runs this executable with --startup, starts hidden in the tray, and keeps monitoring on battery without a task execution time limit. If the tray is unavailable, the window opens so the app stays accessible. An existing instance makes a startup launch exit quietly.

The toggle reads actual Task Scheduler state on a background thread. Disabling it removes only the app-owned task for the current user. If the executable has moved, Use this copy updates its path; keep the app in a stable location. Merely installing or opening the new build does not enable startup.

The native validation probe reads startup state and validates the task XML in memory without registering or running a task. Actual sign-in launch still requires an enabled setting and a real sign-in cycle.

See [Microsoft: task security contexts](https://learn.microsoft.com/en-us/windows/win32/taskschd/security-contexts-for-running-tasks) and [logon triggers](https://learn.microsoft.com/en-us/windows/win32/taskschd/starting-an-executable-when-a-user-logs-on).
