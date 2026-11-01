# Install and uninstall

OpenNetworkMonitor runs from any folder, so installing is optional. Installing to Program Files is recommended for one reason: the app runs with administrator rights, and **Launch when I sign in to Windows** only agrees to start an elevated copy from a folder that ordinary programs cannot change. From a user-writable folder such as Downloads or the Desktop the setting stays off and Settings explains why.

## Install

1. Unzip the download.
2. Open PowerShell as administrator in the unzipped folder and run:

```powershell
.\install.ps1
```

It copies the app to `C:\Program Files\OpenNetworkMonitor` and adds a Start menu shortcut. Add `-WhatIf` to see what it would do without changing anything, or `-Destination <folder>` to choose another protected folder. If the app is already running from that folder, exit it from its tray icon first.

If PowerShell refuses to run the script because of its execution policy, start it with `powershell -ExecutionPolicy Bypass -File .\install.ps1`.

## Update

Open the newer build. If an older copy is running it is asked to exit, saves its data and is replaced. A saved history from an older version is upgraded on the first start of the new one. The upgrade cannot be undone: older versions refuse a history written by a newer one, so copy `usage.db` first if you may want to go back.

To update an installed copy, run `install.ps1` from the newer download again.

## Uninstall

Exit the app from its tray icon (right-click, **Exit**), then open PowerShell as administrator and run the script from the install folder or the download:

```powershell
& "C:\Program Files\OpenNetworkMonitor\uninstall.ps1"
```

It removes the sign-in startup task, the Start menu shortcut, the program folder and the notification identity. Your saved history and settings are kept unless you add `-RemoveData`, which deletes `%LOCALAPPDATA%\OpenNetworkMonitor` for the account running the script. Use `-WhatIf` first to see the list.

## Without the scripts

1. Exit the app from its tray icon.
2. Delete the program folder and, if present, the Start menu shortcut.
3. In Task Scheduler, delete the task `OpenNetworkMonitor Startup - …` if you had enabled start at sign-in.
4. To remove the saved data, delete `%LOCALAPPDATA%\OpenNetworkMonitor`.

See [PRIVACY.md](PRIVACY.md) for everything the app stores or changes.
