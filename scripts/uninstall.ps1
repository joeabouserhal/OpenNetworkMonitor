#Requires -Version 5.1
<#
.SYNOPSIS
Removes OpenNetworkMonitor from this PC.

.DESCRIPTION
Removes the sign-in startup task, the Start menu shortcut and the program folder. Your
saved usage history is kept unless you pass -RemoveData.

Run from an elevated PowerShell:  .\uninstall.ps1  [-RemoveData]
Use -WhatIf to see what would happen without changing anything.
#>
[CmdletBinding(SupportsShouldProcess, ConfirmImpact = 'High')]
param(
    [string]$InstallFolder = (Join-Path $env:ProgramFiles 'OpenNetworkMonitor'),
    # Also delete this user's saved history, settings and alert memory.
    [switch]$RemoveData
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$isAdmin = ([Security.Principal.WindowsPrincipal]$identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin -and -not $WhatIfPreference) {
    throw 'Run this script from an elevated PowerShell (Run as administrator).'
}

if (Get-Process -Name OpenNetworkMonitor -ErrorAction SilentlyContinue) {
    throw 'OpenNetworkMonitor is running. Exit it from its tray icon (right-click, Exit) first, so it can save its data, then run this again.'
}

# Startup tasks are created per user and named for their account.
$tasks = Get-ScheduledTask -ErrorAction SilentlyContinue | Where-Object { $_.TaskName -like 'OpenNetworkMonitor Startup - *' }
foreach ($task in $tasks) {
    if ($PSCmdlet.ShouldProcess($task.TaskName, 'Remove startup task')) {
        Unregister-ScheduledTask -TaskName $task.TaskName -Confirm:$false
    }
}

$shortcut = Join-Path ([Environment]::GetFolderPath('CommonPrograms')) 'OpenNetworkMonitor.lnk'
if ((Test-Path -LiteralPath $shortcut) -and $PSCmdlet.ShouldProcess($shortcut, 'Remove shortcut')) {
    Remove-Item -LiteralPath $shortcut -Force
}

$InstallFolder = [IO.Path]::GetFullPath($InstallFolder)
if (Test-Path -LiteralPath $InstallFolder) {
    # Only a folder that really holds the app is removed, never an arbitrary path.
    if (-not (Test-Path -LiteralPath (Join-Path $InstallFolder 'OpenNetworkMonitor.exe') -PathType Leaf)) {
        throw "$InstallFolder does not contain OpenNetworkMonitor.exe, so it was left alone."
    }
    if ($PSCmdlet.ShouldProcess($InstallFolder, 'Remove program folder')) {
        Remove-Item -LiteralPath $InstallFolder -Recurse -Force
    }
}

# The notification identity that alerts register for the current user.
$identityKey = 'HKCU:\Software\Classes\AppUserModelId\OpenNetworkMonitor.App'
if ((Test-Path -LiteralPath $identityKey) -and $PSCmdlet.ShouldProcess($identityKey, 'Remove notification identity')) {
    Remove-Item -LiteralPath $identityKey -Recurse -Force
}

$data = Join-Path $env:LOCALAPPDATA 'OpenNetworkMonitor'
if (Test-Path -LiteralPath $data) {
    if ($RemoveData) {
        if ($PSCmdlet.ShouldProcess($data, 'Delete saved history and settings')) {
            Remove-Item -LiteralPath $data -Recurse -Force
        }
    } else {
        Write-Host "Your saved history was kept in $data. Delete that folder to remove it, or run this script again with -RemoveData."
    }
}
Write-Host 'Done.'
