<#
.SYNOPSIS
    Official Vanta Toolchain Windows PowerShell Installer
.DESCRIPTION
    Installs the native Vanta compiler (vantac.exe) and CLI driver (vanta.exe),
    along with the standard library, dual licenses, and uninstaller on Windows x86-64.
.PARAMETER Prefix
    Installation directory. Defaults to "$HOME\.vanta"
.PARAMETER Version
    Release version tag or semantic version. Defaults to "v1.0-RC2"
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

# Helper function to normalize version strings for comparison
function Normalize-VantaVersion([string]$v) {
    if ([string]::IsNullOrWhiteSpace($v)) { return "" }
    $clean = $v.Trim().TrimStart('v', 'V')
    if ($clean -ieq "1.0-RC2" -or $clean -ieq "1.0.0-rc.2") {
        return "1.0.0-rc.2"
    }
    return $clean.ToLower()
}

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host ("  ⚡ Vanta Toolchain Windows Installer ({0})" -f $Version) -ForegroundColor Yellow
Write-Host "==========================================================" -ForegroundColor Cyan

# 1. Architecture Check (Strictly require 64-bit AMD64)
$arch = $env:PROCESSOR_ARCHITECTURE
$archW6432 = $env:PROCESSOR_ARCHITEW6432
if ($arch -ne "AMD64" -and $archW6432 -ne "AMD64") {
    Write-Error ("Vanta Windows native distribution requires an x86-64 (AMD64) system. Detected: {0}" -f $arch)
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
    (Join-Path $RepoRoot "dist\vanta-$Version-x86_64-pc-windows-msvc\bin"),
    (Join-Path $RepoRoot "dist\vanta-v1.0-RC2-x86_64-pc-windows-msvc\bin")
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
    Write-Host ("--> Found local Windows binaries in {0}..." -f (Split-Path -Parent $VantacSrc)) -ForegroundColor Green
    Copy-Item -Path $VantacSrc -Destination (Join-Path $BinDest "vantac.exe") -Force
    $InstalledFiles.Add("bin/vantac.exe")
    Copy-Item -Path $VantaSrc -Destination (Join-Path $BinDest "vanta.exe") -Force
    $InstalledFiles.Add("bin/vanta.exe")
} else {
    # Remote download from GitHub Releases
    $ZipName = "vanta-$Version-x86_64-pc-windows-msvc.zip"
    $ReleaseUrl = "https://github.com/Looka-info/Venta/releases/download/$Version/$ZipName"
    $TempZip = Join-Path $env:TEMP $ZipName

    Write-Host ("--> Downloading release archive from GitHub Releases ({0})..." -f $ReleaseUrl) -ForegroundColor Cyan
    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13
        Invoke-WebRequest -Uri $ReleaseUrl -OutFile $TempZip -UseBasicParsing
        Write-Host ("--> Extracting {0} into {1}..." -f $ZipName, $Prefix) -ForegroundColor Cyan
        Expand-Archive -Path $TempZip -DestinationPath $Prefix -Force
        Remove-Item $TempZip -Force -ErrorAction SilentlyContinue

        if (Test-Path (Join-Path $BinDest "vantac.exe")) {
            $InstalledFiles.Add("bin/vantac.exe")
        }
        if (Test-Path (Join-Path $BinDest "vanta.exe")) {
            $InstalledFiles.Add("bin/vanta.exe")
        }
    } catch {
        Write-Error ("Failed to install Vanta: could not find local binaries and failed to download release archive from {0} ({1})" -f $ReleaseUrl, $_.Exception.Message)
        exit 1
    }
}

# 2. Verify that binaries exist
$VantacInstalled = Join-Path $BinDest "vantac.exe"
$VantaInstalled = Join-Path $BinDest "vanta.exe"

if (-not (Test-Path $VantacInstalled) -or -not (Test-Path $VantaInstalled)) {
    Write-Error ("Installation failed: required binaries vantac.exe or vanta.exe were not placed in {0}." -f $BinDest)
    exit 1
}

# 3. Standard Library Installation
$StdCandidates = @(
    (Join-Path $ScriptDir "lib\vanta\std"),
    (Join-Path (Split-Path -Parent $ScriptDir) "lib\vanta\std"),
    (Join-Path $RepoRoot "library\std")
)

$InstalledStd = $false
foreach ($stdCand in $StdCandidates) {
    if (Test-Path (Join-Path $stdCand "mod.va")) {
        Write-Host ("--> Installing standard library modules from {0}..." -f $stdCand) -ForegroundColor Green
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

# 4. Copy Uninstaller into prefix
$UninstallSrc = Join-Path $ScriptDir "uninstall.ps1"
if (-not (Test-Path $UninstallSrc)) {
    $UninstallSrc = Join-Path $RepoRoot "scripts\uninstall.ps1"
}
if (Test-Path $UninstallSrc) {
    Copy-Item -Path $UninstallSrc -Destination (Join-Path $Prefix "uninstall.ps1") -Force
    $InstalledFiles.Add("uninstall.ps1")
}

# 5. Copy license files and documentation if available
$LicenseCandidates = @("LICENSE", "LICENSE-MIT", "LICENSE-APACHE", "README.md")
foreach ($lic in $LicenseCandidates) {
    $cand = Join-Path $ScriptDir $lic
    if (-not (Test-Path $cand)) { $cand = Join-Path $RepoRoot $lic }
    if (Test-Path $cand) {
        Copy-Item -Path $cand -Destination (Join-Path $Prefix $lic) -Force
        $InstalledFiles.Add($lic)
    }
}

# 6. Copy docs and examples if available
$ExamplesSrc = Join-Path $ScriptDir "examples"
if (-not (Test-Path $ExamplesSrc)) { $ExamplesSrc = Join-Path $RepoRoot "examples" }
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

# 7. Verification of Installed Executables (Exit code inspection and version validation)
Write-Host "--> Verifying installed executables..." -ForegroundColor Cyan

$vantacOutFile = Join-Path $env:TEMP ("vantac_ver_" + $PID + ".txt")
$vantacErrFile = Join-Path $env:TEMP ("vantac_err_" + $PID + ".txt")
$vantacProc = Start-Process -FilePath $VantacInstalled -ArgumentList "--version" -Wait -PassThru -NoNewWindow -RedirectStandardOutput $vantacOutFile -RedirectStandardError $vantacErrFile
$vantacExit = $vantacProc.ExitCode
$vantacVerOutput = if (Test-Path $vantacOutFile) { (Get-Content -Path $vantacOutFile -Raw).Trim() } else { "" }
Remove-Item $vantacOutFile, $vantacErrFile -Force -ErrorAction SilentlyContinue

if ($vantacExit -ne 0 -or [string]::IsNullOrWhiteSpace($vantacVerOutput)) {
    Write-Error ("Verification failed: vantac.exe --version failed with exit code {0}. Output: '{1}'" -f $vantacExit, $vantacVerOutput)
    exit 1
}

$vantaOutFile = Join-Path $env:TEMP ("vanta_ver_" + $PID + ".txt")
$vantaErrFile = Join-Path $env:TEMP ("vanta_err_" + $PID + ".txt")
$vantaProc = Start-Process -FilePath $VantaInstalled -ArgumentList "--version" -Wait -PassThru -NoNewWindow -RedirectStandardOutput $vantaOutFile -RedirectStandardError $vantaErrFile
$vantaExit = $vantaProc.ExitCode
$vantaVerOutput = if (Test-Path $vantaOutFile) { (Get-Content -Path $vantaOutFile -Raw).Trim() } else { "" }
Remove-Item $vantaOutFile, $vantaErrFile -Force -ErrorAction SilentlyContinue

if ($vantaExit -ne 0 -or [string]::IsNullOrWhiteSpace($vantaVerOutput)) {
    Write-Error ("Verification failed: vanta.exe --version failed with exit code {0}. Output: '{1}'" -f $vantaExit, $vantaVerOutput)
    exit 1
}

# Parse version token from "vantac <version>" and "vanta <version>"
$vMatch = [regex]::Match($vantacVerOutput, '^(?:vantac|ventac)\s+([^\s]+)$')
if (-not $vMatch.Success) {
    Write-Error ("Malformed version string from vantac.exe: '{0}'" -f $vantacVerOutput)
    exit 1
}
$vantacReportedVersion = $vMatch.Groups[1].Value

$vantaMatch = [regex]::Match($vantaVerOutput, '^(?:vanta|venta)\s+([^\s]+)$')
if (-not $vantaMatch.Success) {
    Write-Error ("Malformed version string from vanta.exe: '{0}'" -f $vantaVerOutput)
    exit 1
}
$vantaReportedVersion = $vantaMatch.Groups[1].Value

# Validate against requested Version
$normExpected = Normalize-VantaVersion $Version
$normReported = Normalize-VantaVersion $vantacReportedVersion

if ($normReported -ne $normExpected) {
    Write-Error ("Version mismatch: requested version '{0}' (normalized '{1}'), but installed binary reported '{2}' (normalized '{3}')" -f $Version, $normExpected, $vantacReportedVersion, $normReported)
    exit 1
}

Write-Host ("    {0}" -f $vantacVerOutput) -ForegroundColor Green
Write-Host ("    {0}" -f $vantaVerOutput) -ForegroundColor Green

# 8. User PATH configuration (Idempotent and safely tracked)
$PathAdded = $false
if (-not $NoPathUpdate) {
    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($null -eq $UserPath) { $UserPath = "" }
    $PathEntries = $UserPath -split ';' | Where-Object { $_ -ne "" }
    $AlreadyInPath = $false
    foreach ($entry in $PathEntries) {
        if ($entry.TrimEnd('\') -ieq $BinDest.TrimEnd('\')) {
            $AlreadyInPath = $true
            break
        }
    }

    if (-not $AlreadyInPath) {
        Write-Host ("--> Adding {0} to User PATH environment variable..." -f $BinDest) -ForegroundColor Green
        $NewUserPath = if ([string]::IsNullOrEmpty($UserPath)) { $BinDest } else { "$BinDest;$UserPath" }
        [Environment]::SetEnvironmentVariable("Path", $NewUserPath, "User")
        $env:Path = "$BinDest;$env:Path"
        $PathAdded = $true
    }
}

# 9. Write install-manifest.json
$ManifestPath = Join-Path $Prefix "install-manifest.json"
$ManifestObj = [PSCustomObject]@{
    name = "vanta"
    version = $vantacReportedVersion
    target = "x86_64-pc-windows-msvc"
    installed_at = (Get-Date -Format "o")
    prefix = $Prefix
    bin_dir = $BinDest
    path_added = $PathAdded
    binaries = @("bin/vantac.exe", "bin/vanta.exe")
    files = ($InstalledFiles | Select-Object -Unique)
}
$ManifestObj | ConvertTo-Json -Depth 5 | Set-Content -Path $ManifestPath -Encoding UTF8
Write-Host ("--> Wrote installation manifest: {0}" -f $ManifestPath) -ForegroundColor Green

Write-Host ""
Write-Host "==========================================================" -ForegroundColor Green
Write-Host "  ⚡ Vanta Toolchain Installed Successfully on Windows!     " -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green
Write-Host ("Installation prefix: {0}" -f $Prefix)
Write-Host ("Binaries path:       {0}" -f $BinDest)
Write-Host ("Standard library:    {0}" -f $StdDest)
Write-Host ("Manifest:            {0}" -f $ManifestPath)
Write-Host ("PATH modified:       {0}" -f $PathAdded)
Write-Host ""
Write-Host "To use Vanta immediately in current session:"
Write-Host ("    `$env:Path = `"{0};`$env:Path`"" -f $BinDest)
Write-Host "Verify with:"
Write-Host "    vantac --version"
Write-Host "    vanta --version"
