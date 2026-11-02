#Requires -Version 5.1
<#
.SYNOPSIS
Installs OpenNetworkMonitor for every user of this PC.

.DESCRIPTION
Copies the built app to a protected location (Program Files by default) and
adds a Start menu shortcut. Only a folder that ordinary programs cannot change is a safe
home for an app that runs with administrator rights, and it is the only place the
"Launch when I sign in" setting will agree to use.

Run from an elevated PowerShell:  .\install.ps1
Use -WhatIf to see what would happen without changing anything.
#>
[CmdletBinding(SupportsShouldProcess)]
param(
    [string]$Destination = (Join-Path $env:ProgramFiles 'OpenNetworkMonitor'),
    # The executable to install. By default the one next to this script, or the newest build in dist/.
    [string]$Executable = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not $Executable) {
    $Executable = Join-Path $PSScriptRoot 'OpenNetworkMonitor.exe'
    if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) {
        $Executable = Join-Path $PSScriptRoot '..\dist\OpenNetworkMonitor.exe'
    }
}
if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) {
    throw "OpenNetworkMonitor.exe was not found. Build it first, or pass -Executable <path>."
}
$Executable = [IO.Path]::GetFullPath($Executable)

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$isAdmin = ([Security.Principal.WindowsPrincipal]$identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin -and -not $WhatIfPreference) {
    throw 'Run this script from an elevated PowerShell (Run as administrator).'
}

$Destination = [IO.Path]::GetFullPath($Destination)
$running = Get-Process -Name OpenNetworkMonitor -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -and ([IO.Path]::GetFullPath($_.Path) -eq (Join-Path $Destination 'OpenNetworkMonitor.exe')) }
if ($running) {
    throw 'OpenNetworkMonitor is running from the install folder. Exit it from its tray icon first, so it can save its data.'
}

$shortcut = Join-Path ([Environment]::GetFolderPath('CommonPrograms')) 'OpenNetworkMonitor.lnk'

if ($PSCmdlet.ShouldProcess($Destination, 'Copy OpenNetworkMonitor')) {
    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
    Copy-Item -LiteralPath $Executable -Destination (Join-Path $Destination 'OpenNetworkMonitor.exe') -Force
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'uninstall.ps1') -Destination $Destination -Force
}
if ($PSCmdlet.ShouldProcess($shortcut, 'Create Start menu shortcut')) {
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($shortcut)
    $link.TargetPath = Join-Path $Destination 'OpenNetworkMonitor.exe'
    $link.WorkingDirectory = $Destination
    $link.Description = 'Local network usage monitor'
    $link.Save()
}

if (-not $WhatIfPreference) {
    Write-Host "Installed to $Destination"
    Write-Host 'Open it from the Start menu. In Settings you can now turn on "Launch when I sign in to Windows".'
}
