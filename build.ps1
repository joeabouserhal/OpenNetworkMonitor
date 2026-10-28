#Requires -Version 5.1
[CmdletBinding()]
param(
    [ValidateSet('Release', 'Debug')]
    [string]$Configuration = 'Release',
    [switch]$SkipChecks
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath($PSScriptRoot)
$targetRoot = Join-Path $projectRoot 'target'
$distRoot = Join-Path $projectRoot 'dist'

function Assert-ProjectOutput([string]$Path) {
    $absolute = [IO.Path]::GetFullPath($Path)
    foreach ($allowedRoot in @($targetRoot, $distRoot)) {
        if ($absolute.StartsWith($allowedRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { return }
    }
    throw "Refusing to change a file outside the project's target/ or dist/ directories: $absolute"
}

function Preserve-LockedExecutable([string]$Path) {
    Assert-ProjectOutput $Path
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return }
    try {
        $probe = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::Read)
        $probe.Dispose()
    } catch [IO.IOException] {
        Move-PreviousExecutable $Path
    } catch [UnauthorizedAccessException] {
        Move-PreviousExecutable $Path
    }
}

function Move-PreviousExecutable([string]$Path) {
    $backupRoot = Join-Path $targetRoot 'previous-builds'
    $stamp = (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [Guid]::NewGuid().ToString('N').Substring(0, 8)
    $backup = Join-Path $backupRoot ('OpenNetworkMonitor-' + $stamp + '.exe')
    Assert-ProjectOutput $Path
    Assert-ProjectOutput $backup
    New-Item -ItemType Directory -Path $backupRoot -Force | Out-Null
    Move-Item -LiteralPath $Path -Destination $backup
    Write-Host "Preserved the in-use executable: $backup"
}

function Invoke-Cargo([string]$Label, [string[]]$Arguments) {
    Write-Host "`n$Label" -ForegroundColor Cyan
    & cargo @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Label failed (exit $LASTEXITCODE)." }
}

Push-Location -LiteralPath $projectRoot
try {
    if ($env:OS -ne 'Windows_NT') { throw 'OpenNetworkMonitor currently builds on Windows with the Rust MSVC toolchain.' }
    Get-Command cargo, rustc -ErrorAction Stop | Out-Null
    $rustInfo = (& rustc -vV) -join "`n"
    if ($LASTEXITCODE -ne 0) { throw 'Could not read the Rust toolchain.' }
    $hostMatch = [regex]::Match($rustInfo, '(?m)^host: (\S+)')
    $versionMatch = [regex]::Match($rustInfo, '(?m)^release: (\d+\.\d+\.\d+)')
    if (-not $hostMatch.Success -or -not $hostMatch.Groups[1].Value.EndsWith('-pc-windows-msvc')) {
        throw 'Use a Windows MSVC Rust toolchain and install Visual Studio C++ Build Tools.'
    }
    if (-not $versionMatch.Success -or [version]$versionMatch.Groups[1].Value -lt [version]'1.95.0') {
        throw 'Rust 1.95 or newer is required.'
    }
    $rustHost = $hostMatch.Groups[1].Value
    $buildTarget = if ([string]::IsNullOrEmpty($env:CARGO_BUILD_TARGET)) { $rustHost } else { $env:CARGO_BUILD_TARGET }
    if ($buildTarget -ne $rustHost) { throw 'This script packages native builds. Run with CARGO_BUILD_TARGET unset or matching the Rust host.' }
    $architecture = ($rustHost -split '-')[0]
    $manifest = Get-Content -LiteralPath (Join-Path $projectRoot 'Cargo.toml') -Raw
    $packageVersion = [regex]::Match($manifest, '(?m)^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+[A-Za-z0-9.+-]*)"').Groups[1].Value
    if (-not $packageVersion) { throw 'Could not read the package version from Cargo.toml.' }

    $outputRoot = $targetRoot
    if (-not [string]::IsNullOrEmpty($env:CARGO_BUILD_TARGET)) { $outputRoot = Join-Path $outputRoot $buildTarget }
    $profile = $Configuration.ToLowerInvariant()
    $executable = Join-Path (Join-Path $outputRoot $profile) 'OpenNetworkMonitor.exe'
    $buildArguments = @('build', '--locked', '--bin', 'OpenNetworkMonitor', '--target-dir', $targetRoot)
    if ($Configuration -eq 'Release') { $buildArguments += '--release' }

    if (-not $SkipChecks) {
        Invoke-Cargo 'Checking formatting' @('fmt', '--check')
        Invoke-Cargo 'Running tests' @('test', '--locked', '--target-dir', $targetRoot)
        Invoke-Cargo 'Checking code quality' @('clippy', '--locked', '--all-targets', '--features', 'ui-preview', '--target-dir', $targetRoot, '--', '-D', 'warnings')
    }
    Preserve-LockedExecutable $executable
    # Cargo may hard-link the top-level executable to deps/. Preserve a mapped
    # artifact there too so rebuilding the same version can link successfully.
    $dependencyOutput = Join-Path (Join-Path $outputRoot $profile) 'deps'
    if (Test-Path -LiteralPath $dependencyOutput -PathType Container) {
        foreach ($artifact in Get-ChildItem -LiteralPath $dependencyOutput -Filter 'OpenNetworkMonitor*.exe' -File) {
            Preserve-LockedExecutable $artifact.FullName
        }
    }
    Invoke-Cargo "Building $Configuration" $buildArguments
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) { throw "Build output was not found at $executable. Check project-local Cargo target configuration." }

    $packageName = "OpenNetworkMonitor-v$packageVersion-windows-$architecture"
    if ($Configuration -eq 'Debug') { $packageName += '-debug' }
    $packageRoot = Join-Path $distRoot $packageName
    $packagedExe = Join-Path $packageRoot 'OpenNetworkMonitor.exe'
    Assert-ProjectOutput $packagedExe
    New-Item -ItemType Directory -Path $packageRoot -Force | Out-Null
    Preserve-LockedExecutable $packagedExe
    Copy-Item -LiteralPath $executable -Destination $packagedExe -Force
    foreach ($name in @('README.md', 'LICENSE', 'THIRD_PARTY.md')) {
        Copy-Item -LiteralPath (Join-Path $projectRoot $name) -Destination (Join-Path $packageRoot $name) -Force
    }
    $docsRoot = Join-Path $packageRoot 'docs'
    New-Item -ItemType Directory -Path $docsRoot -Force | Out-Null
    foreach ($name in @('ADMIN.md', 'ARCHITECTURE.md', 'ROADMAP.md', 'VALIDATION.md')) {
        Copy-Item -LiteralPath (Join-Path $projectRoot ('docs/' + $name)) -Destination (Join-Path $docsRoot $name) -Force
    }
    $screenshotsRoot = Join-Path $packageRoot 'screenshots'
    foreach ($name in @('overview.png', 'app-traffic.png')) {
        $source = Join-Path $projectRoot ('screenshots/' + $name)
        if (Test-Path -LiteralPath $source -PathType Leaf) {
            New-Item -ItemType Directory -Path $screenshotsRoot -Force | Out-Null
            Copy-Item -LiteralPath $source -Destination (Join-Path $screenshotsRoot $name) -Force
        }
    }
    $archive = Join-Path $distRoot ($packageName + '.zip')
    Assert-ProjectOutput $archive
    Compress-Archive -Path (Join-Path $packageRoot '*') -DestinationPath $archive -Force
    $hasher = [Security.Cryptography.SHA256]::Create()
    $archiveStream = [IO.File]::OpenRead($archive)
    try {
        $hash = [BitConverter]::ToString($hasher.ComputeHash($archiveStream)).Replace('-', '').ToLowerInvariant()
    } finally {
        $archiveStream.Dispose()
        $hasher.Dispose()
    }
    Set-Content -LiteralPath ($archive + '.sha256') -Value ($hash + '  ' + [IO.Path]::GetFileName($archive)) -Encoding Ascii
    Write-Host "`nBuild complete." -ForegroundColor Green
    Write-Host "App: $packagedExe"
    Write-Host "ZIP: $archive"
    Write-Host "SHA256: $hash"
    Write-Host 'Exit any older running copy from its tray before opening the new executable.'
} finally {
    Pop-Location
}
