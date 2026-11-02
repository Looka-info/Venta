<#
.SYNOPSIS
    Official Vanta Toolchain Windows PowerShell Installer
.DESCRIPTION
    Installs the native Vanta compiler (vantac.exe) and CLI driver (vanta.exe),
    along with the standard library on Windows x86-64.
.PARAMETER Prefix
    Installation directory. Defaults to "$HOME\.vanta"
.PARAMETER Version
    Release version tag. Defaults to "v1.0-RC2"
.PARAMETER NoPathUpdate
    If specified, skips modifying the User PATH environment variable.
#>

[CmdletBinding()]
param (
    [string]$Prefix = "$HOME\.vanta",
    [string]$Version = "v1.0-RC2",
    [switch]$NoPathUpdate
)

$ErrorActionPreference = "Stop"

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  ⚡ Vanta Toolchain Windows Installer ($Version)           " -ForegroundColor Yellow
Write-Host "==========================================================" -ForegroundColor Cyan

# 1. Architecture Check (Require 64-bit AMD64)
$arch = $env:PROCESSOR_ARCHITECTURE
$archW6432 = $env:PROCESSOR_ARCHITEW6432
if ($arch -ne "AMD64" -and $archW6432 -ne "AMD64") {
    Write-Error "Vanta Windows native distribution requires an x86-64 (AMD64) system. Detected: $arch"
    exit 1
}

$ScriptDir = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Definition }
$RepoRoot = if ($ScriptDir) { Split-Path -Parent $ScriptDir } else { $PWD }

$BinDest = Join-Path $Prefix "bin"
$LibDest = Join-Path $Prefix "lib\vanta"
$StdDest = Join-Path $LibDest "std"

New-Item -ItemType Directory -Path $BinDest -Force | Out-Null
New-Item -ItemType Directory -Path $StdDest -Force | Out-Null

$InstalledFiles = [System.Collections.Generic.List[string]]::new()

$VantacSrc = $null
$VantaSrc = $null

# Candidates for local binaries
$LocalBinCandidates = @(
    (Join-Path $ScriptDir "bin"),
    (Join-Path (Split-Path -Parent $ScriptDir) "bin"),
    (Join-Path $RepoRoot "target\release"),
    (Join-Path $RepoRoot "target\x86_64-pc-windows-msvc\release"),
    (Join-Path $RepoRoot "target\x86_64-pc-windows-gnu\release"),
    (Join-Path $RepoRoot "dist\vanta-$Version-x86_64-pc-windows-msvc\bin"),
    (Join-Path $RepoRoot "packages\vanta-lang\dist\bin")
)

foreach ($cand in $LocalBinCandidates) {
    if (Test-Path (Join-Path $cand "vantac.exe")) {
        $VantacSrc = Join-Path $cand "vantac.exe"
    } elseif (Test-Path (Join-Path $cand "ventac.exe")) {
        $VantacSrc = Join-Path $cand "ventac.exe"
    }

    if (Test-Path (Join-Path $cand "vanta.exe")) {
        $VantaSrc = Join-Path $cand "vanta.exe"
    } elseif (Test-Path (Join-Path $cand "venta.exe")) {
        $VantaSrc = Join-Path $cand "venta.exe"
    }

    if ($VantacSrc -and $VantaSrc) {
        break
    }
}

if ($VantacSrc -and $VantaSrc) {
    Write-Host "--> Found local Windows binaries in $(Split-Path -Parent $VantacSrc)..." -ForegroundColor Green
    Copy-Item -Path $VantacSrc -Destination (Join-Path $BinDest "vantac.exe") -Force
    $InstalledFiles.Add("bin/vantac.exe")
    Copy-Item -Path $VantaSrc -Destination (Join-Path $BinDest "vanta.exe") -Force
    $InstalledFiles.Add("bin/vanta.exe")
} else {
    # Remote download from GitHub Releases
    $ZipName = "vanta-$Version-x86_64-pc-windows-msvc.zip"
    $ReleaseUrl = "https://github.com/Looka-info/Venta/releases/download/$Version/$ZipName"
    $TempZip = Join-Path $env:TEMP $ZipName

    Write-Host "--> Downloading release archive from GitHub Releases ($ReleaseUrl)..." -ForegroundColor Cyan
    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13
        Invoke-WebRequest -Uri $ReleaseUrl -OutFile $TempZip -UseBasicParsing
        Write-Host "--> Extracting $ZipName into $Prefix..." -ForegroundColor Cyan
        Expand-Archive -Path $TempZip -DestinationPath $Prefix -Force
        Remove-Item $TempZip -Force -ErrorAction SilentlyContinue

        if (Test-Path (Join-Path $BinDest "vantac.exe")) {
            $InstalledFiles.Add("bin/vantac.exe")
        }
        if (Test-Path (Join-Path $BinDest "vanta.exe")) {
            $InstalledFiles.Add("bin/vanta.exe")
        }
    } catch {
        Write-Error "Failed to install Vanta: could not find local binaries and failed to download release archive from $ReleaseUrl ($($_.Exception.Message))"
        exit 1
    }
}

# 2. Verify that binaries exist - FAIL HARD if missing
$VantacInstalled = Join-Path $BinDest "vantac.exe"
$VantaInstalled = Join-Path $BinDest "vanta.exe"

if (-not (Test-Path $VantacInstalled) -or -not (Test-Path $VantaInstalled)) {
    Write-Error "Installation failed: required binaries vantac.exe or vanta.exe were not placed in $BinDest."
    exit 1
}

# 3. Standard Library Installation
$StdCandidates = @(
    (Join-Path $ScriptDir "lib\vanta\std"),
    (Join-Path (Split-Path -Parent $ScriptDir) "lib\vanta\std"),
    (Join-Path $RepoRoot "library\std"),
    (Join-Path $RepoRoot "packages\vanta-lang\dist\lib\vanta\std")
)

$InstalledStd = $false
foreach ($stdCand in $StdCandidates) {
    if (Test-Path (Join-Path $stdCand "mod.va")) {
        Write-Host "--> Installing standard library modules from $stdCand..." -ForegroundColor Green
        # Copy all items recursively so subdirectories like collections are preserved
        Get-ChildItem -Path $stdCand -Recurse | ForEach-Object {
            $rel = $_.FullName.Substring($stdCand.Length).TrimStart('\', '/')
            $dest = Join-Path $StdDest $rel
            if ($_.PSIsContainer) {
                New-Item -ItemType Directory -Path $dest -Force | Out-Null
            } else {
                Copy-Item -Path $_.FullName -Destination $dest -Force
                $InstalledFiles.Add("lib/vanta/std/$($rel.Replace('\', '/'))")
            }
        }
        $InstalledStd = $true
        break
    }
}

if (-not $InstalledStd -and -not (Test-Path (Join-Path $StdDest "mod.va"))) {
    Write-Warning "Standard library mod.va not found in candidate paths. Embedded compiler stdlib fallback will be used."
}

# 4. Copy docs and examples if available locally
$ExamplesSrc = Join-Path $RepoRoot "examples"
if (-not (Test-Path $ExamplesSrc)) {
    $ExamplesSrc = Join-Path $ScriptDir "examples"
}
if (Test-Path $ExamplesSrc) {
    $ExamplesDest = Join-Path $Prefix "examples"
    New-Item -ItemType Directory -Path $ExamplesDest -Force | Out-Null
    Get-ChildItem -Path $ExamplesSrc -Recurse | ForEach-Object {
        $rel = $_.FullName.Substring($ExamplesSrc.Length).TrimStart('\', '/')
        $dest = Join-Path $ExamplesDest $rel
        if ($_.PSIsContainer) {
            New-Item -ItemType Directory -Path $dest -Force | Out-Null
        } else {
            Copy-Item -Path $_.FullName -Destination $dest -Force
            $InstalledFiles.Add("examples/$($rel.Replace('\', '/'))")
        }
    }
}

# 5. Write install-manifest.json
$ManifestPath = Join-Path $Prefix "install-manifest.json"
$ManifestObj = [PSCustomObject]@{
    version = $Version
    target = "x86_64-pc-windows-msvc"
    installed_at = (Get-Date -Format "o")
    prefix = $Prefix
    binaries = @("bin/vantac.exe", "bin/vanta.exe")
    files = ($InstalledFiles | Select-Object -Unique)
}
$ManifestObj | ConvertTo-Json -Depth 5 | Set-Content -Path $ManifestPath -Encoding UTF8
Write-Host "--> Wrote installation manifest: $ManifestPath" -ForegroundColor Green

# 6. User PATH configuration
if (-not $NoPathUpdate) {
    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($null -eq $UserPath) { $UserPath = "" }
    $PathEntries = $UserPath -split ';' | Where-Object { $_ -ne "" }
    $AlreadyInPath = $false
    foreach ($entry in $PathEntries) {
        if ($entry.TrimEnd('\') -eq $BinDest.TrimEnd('\')) {
            $AlreadyInPath = $true
            break
        }
    }

    if (-not $AlreadyInPath) {
        Write-Host "--> Adding $BinDest to User PATH environment variable..." -ForegroundColor Green
        $NewUserPath = "$BinDest;$UserPath".TrimEnd(';')
        [Environment]::SetEnvironmentVariable("Path", $NewUserPath, "User")
        $env:Path = "$BinDest;$env:Path"
    }
}

# 7. Verification of Installed Executables
Write-Host "--> Verifying installed executables..." -ForegroundColor Cyan
try {
    $vantacVer = & $VantacInstalled --version
    $vantaVer = & $VantaInstalled --version
    Write-Host "    $vantacVer" -ForegroundColor Green
    Write-Host "    $vantaVer" -ForegroundColor Green
} catch {
    Write-Error "Verification failed while running installed binaries: $($_.Exception.Message)"
    exit 1
}

Write-Host ""
Write-Host "==========================================================" -ForegroundColor Green
Write-Host "  ⚡ Vanta Toolchain Installed Successfully on Windows!     " -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green
Write-Host "Installation prefix: $Prefix"
Write-Host "Binaries path:       $BinDest"
Write-Host "Standard library:    $StdDest"
Write-Host "Manifest:            $ManifestPath"
Write-Host ""
Write-Host "To use Vanta immediately in current session:"
Write-Host "    `$env:Path = `"$BinDest;`$env:Path`""
Write-Host "Verify with:"
Write-Host "    vantac --version"
Write-Host "    vanta --version"
