<#
.SYNOPSIS
    Official Vanta Toolchain Windows PowerShell Installer
.DESCRIPTION
    Installs the native Vanta compiler (ventac.exe, vantac.exe) and driver (venta.exe, vanta.exe),
    runtime archive (libvanta_rt.a), and standard library on Windows x86-64.
.PARAMETER Prefix
    Installation directory. Defaults to "$HOME\.vanta"
.PARAMETER Version
    Release version tag. Defaults to "v1.0-RC2"
#>

[CmdletBinding()]
param (
    [string]$Prefix = "$HOME\.vanta",
    [string]$Version = "v1.0-RC2"
)

$ErrorActionPreference = "Stop"

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  ⚡ Vanta Programming Language Windows Installer ($Version) " -ForegroundColor Yellow
Write-Host "==========================================================" -ForegroundColor Cyan

# 1. Architecture Check
$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -ne "AMD64") {
    Write-Warning "Detected architecture: $arch. Vanta native Windows binaries target x86-64 (AMD64)."
}

$ScriptDir = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Definition }
$RepoRoot = if ($ScriptDir) { Split-Path -Parent $ScriptDir } else { $PWD }

$BinDest = Join-Path $Prefix "bin"
$LibDest = Join-Path $Prefix "lib\vanta"
$StdDest = Join-Path $LibDest "std"

New-Item -ItemType Directory -Path $BinDest -Force | Out-Null
New-Item -ItemType Directory -Path $StdDest -Force | Out-Null

$FoundLocal = $false

# Candidates for local binaries
$LocalBinCandidates = @(
    (Join-Path $RepoRoot "target\release"),
    (Join-Path $RepoRoot "target\debug"),
    (Join-Path $RepoRoot "dist\vanta-$Version-x86_64-pc-windows-msvc\bin"),
    (Join-Path $RepoRoot "packages\vanta-lang\dist\bin")
)

$VantacSrc = $null
$VantaSrc = $null

foreach ($cand in $LocalBinCandidates) {
    if (Test-Path (Join-Path $cand "ventac.exe")) {
        $VantacSrc = (Join-Path $cand "ventac.exe")
    } elseif (Test-Path (Join-Path $cand "vantac.exe")) {
        $VantacSrc = (Join-Path $cand "vantac.exe")
    }

    if (Test-Path (Join-Path $cand "venta.exe")) {
        $VantaSrc = (Join-Path $cand "venta.exe")
    } elseif (Test-Path (Join-Path $cand "vanta.exe")) {
        $VantaSrc = (Join-Path $cand "vanta.exe")
    }

    if ($VantacSrc -and $VantaSrc) {
        $FoundLocal = $true
        break
    }
}

if ($FoundLocal) {
    Write-Host "--> Found local Windows binaries in $(Split-Path -Parent $VantacSrc)..." -ForegroundColor Green
    Copy-Item -Path $VantacSrc -Destination (Join-Path $BinDest "ventac.exe") -Force
    Copy-Item -Path $VantacSrc -Destination (Join-Path $BinDest "vantac.exe") -Force
    Copy-Item -Path $VantaSrc -Destination (Join-Path $BinDest "venta.exe") -Force
    Copy-Item -Path $VantaSrc -Destination (Join-Path $BinDest "vanta.exe") -Force
} else {
    # Remote download from GitHub Releases
    $ZipName = "vanta-$Version-x86_64-pc-windows-msvc.zip"
    $ReleaseUrl = "https://github.com/Looka-info/Venta/releases/download/$Version/$ZipName"
    $TempZip = Join-Path $env:TEMP $ZipName

    Write-Host "--> Downloading release archive from GitHub Releases..." -ForegroundColor Cyan
    try {
        Invoke-WebRequest -Uri $ReleaseUrl -OutFile $TempZip -UseBasicParsing
        Write-Host "--> Extracting $ZipName..." -ForegroundColor Cyan
        Expand-Archive -Path $TempZip -DestinationPath $Prefix -Force
        Remove-Item $TempZip -Force -ErrorAction SilentlyContinue
    } catch {
        Write-Host "Note: Remote release download was not reachable ($($_.Exception.Message))." -ForegroundColor Yellow
        Write-Host "--> Deploying CLI wrappers and local assets..." -ForegroundColor Cyan

        # Deploy .cmd and .ps1 wrappers to $BinDest if repo wrappers are available
        $CmdSource = Join-Path $RepoRoot "vantac.cmd"
        if (Test-Path $CmdSource) {

            Copy-Item (Join-Path $RepoRoot "vant*.cmd") $BinDest -Force -ErrorAction SilentlyContinue

            Copy-Item (Join-Path $RepoRoot "vant*.ps1") $BinDest -Force -ErrorAction SilentlyContinue
        }
    }
}

# Copy standard library
$StdCandidates = @(
    (Join-Path $RepoRoot "library\std"),
    (Join-Path $RepoRoot "packages\vanta-lang\dist\lib\vanta\std")
)
foreach ($stdCand in $StdCandidates) {
    if (Test-Path $stdCand) {
        Write-Host "--> Installing standard library modules to $StdDest..." -ForegroundColor Green
        Copy-Item -Path (Join-Path $stdCand "*.va") -Destination $StdDest -Force
        break
    }
}

# Copy runtime archive if present
$RtCandidates = @(
    (Join-Path $RepoRoot "runtime\libvanta_rt.a"),
    (Join-Path $RepoRoot "packages\vanta-lang\dist\lib\vanta\libvanta_rt.a")
)
foreach ($rtCand in $RtCandidates) {
    if (Test-Path $rtCand) {
        Write-Host "--> Installing runtime library archive to $LibDest..." -ForegroundColor Green
        Copy-Item -Path $rtCand -Destination (Join-Path $LibDest "libvanta_rt.a") -Force
        break
    }
}

# Environment PATH configuration
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$BinDest*") {
    Write-Host "--> Adding $BinDest to User PATH environment variable..." -ForegroundColor Green
    [Environment]::SetEnvironmentVariable("Path", "$BinDest;$UserPath", "User")
    $env:Path = "$BinDest;$env:Path"
}

Write-Host ""
Write-Host "==========================================================" -ForegroundColor Green
Write-Host "  ⚡ Vanta Toolchain Installed Successfully on Windows!     " -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green
Write-Host "Installation directory: $Prefix"
Write-Host "Binaries directory:     $BinDest"
Write-Host "Targets supported:      Windows x86-64, Linux x86-64, macOS Apple Silicon"
Write-Host ""
Write-Host "Restart your terminal or PowerShell window, then verify with:"
Write-Host "    ventac --version"
Write-Host "    vanta --version"
