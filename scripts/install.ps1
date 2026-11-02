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

$InstalledFiles = [System.Collections.Generic.List[string]]::new()

function Rollback-Installation([System.Collections.Generic.List[string]]$files, [string]$prefixDir) {
    Write-Host "--> Rolling back partial installation..." -ForegroundColor Yellow
    foreach ($rel in $files) {
        $p = Join-Path $prefixDir ($rel.Replace('/', '\'))
        if (Test-Path -LiteralPath $p -PathType Leaf) {
            Remove-Item -LiteralPath $p -Force -ErrorAction SilentlyContinue
        }
    }
    if (Test-Path -LiteralPath $prefixDir) {
        $dirs = Get-ChildItem -LiteralPath $prefixDir -Directory -Recurse -Force -ErrorAction SilentlyContinue |
            Sort-Object -Property { $_.FullName.Length } -Descending
        foreach ($d in $dirs) {
            $rem = Get-ChildItem -LiteralPath $d.FullName -Force -ErrorAction SilentlyContinue
            if ($null -eq $rem -or $rem.Count -eq 0) {
                Remove-Item -LiteralPath $d.FullName -Force -ErrorAction SilentlyContinue
            }
        }
        $rootRem = Get-ChildItem -LiteralPath $prefixDir -Force -ErrorAction SilentlyContinue
        if ($null -eq $rootRem -or $rootRem.Count -eq 0) {
            Remove-Item -LiteralPath $prefixDir -Force -ErrorAction SilentlyContinue
        }
    }
}

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host ("  [INSTALL] Vanta Toolchain Windows Installer ({0})" -f $Version) -ForegroundColor Yellow
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

# Check for existing manifest to protect untracked user files
$existingManifestPath = Join-Path $Prefix "install-manifest.json"
$existingTracked = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
$hasExistingManifest = Test-Path -LiteralPath $existingManifestPath

if ($hasExistingManifest) {
    try {
        $prevM = Get-Content -Raw -LiteralPath $existingManifestPath -Encoding UTF8 | ConvertFrom-Json
        if ($prevM.files) {
            foreach ($f in $prevM.files) {
                $existingTracked.Add($f.Replace('/', '\')) | Out-Null
            }
        }
    } catch {}
}

function Safe-CopyFile([string]$srcFile, [string]$destFile, [string]$relPath) {
    if (Test-Path -LiteralPath $destFile -PathType Leaf) {
        $relNorm = $relPath.Replace('/', '\')
        if (-not $existingTracked.Contains($relNorm) -and -not $hasExistingManifest) {
            # Destination has pre-existing untracked files and no manifest
            Rollback-Installation $InstalledFiles $Prefix
            Write-Error ("Installation refused to overwrite untracked file at '{0}'. Prefix '{1}' contains untracked user files without an install manifest." -f $destFile, $Prefix)
            exit 1
        }
    }
    $parentDir = [System.IO.Path]::GetDirectoryName($destFile)
    if (-not (Test-Path -LiteralPath $parentDir)) {
        New-Item -ItemType Directory -Path $parentDir -Force | Out-Null
    }
    Copy-Item -LiteralPath $srcFile -Destination $destFile -Force
    $InstalledFiles.Add($relPath.Replace('\', '/'))
}

New-Item -ItemType Directory -Path $BinDest -Force | Out-Null
New-Item -ItemType Directory -Path $StdDest -Force | Out-Null

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
    Safe-CopyFile $VantacSrc (Join-Path $BinDest "vantac.exe") "bin/vantac.exe"
    Safe-CopyFile $VantaSrc (Join-Path $BinDest "vanta.exe") "bin/vanta.exe"
} else {
    # Remote download fallback
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
        Write-Error ("Failed to install Vanta: could not find local binaries, and remote release asset '{0}' is not yet published to GitHub Releases for version '{1}'. To install Vanta during the pre-release phase, download the build artifact from GitHub Actions (workflow 'windows-package.yml') or build from source with 'cargo build --release --bin vantac --bin vanta'." -f $ZipName, $Version)
        Rollback-Installation $InstalledFiles $Prefix
        exit 1
    }
}

# 2. Verify that binaries exist
$VantacInstalled = Join-Path $BinDest "vantac.exe"
$VantaInstalled = Join-Path $BinDest "vanta.exe"

if (-not (Test-Path $VantacInstalled) -or -not (Test-Path $VantaInstalled)) {
    Rollback-Installation $InstalledFiles $Prefix
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
                if (-not (Test-Path $dest)) {
                    New-Item -ItemType Directory -Path $dest -Force | Out-Null
                }
            } else {
                Safe-CopyFile $_.FullName $dest ("lib/vanta/std/$($rel.Replace('\', '/'))")
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
    Safe-CopyFile $UninstallSrc (Join-Path $Prefix "uninstall.ps1") "uninstall.ps1"
}

# 5. Copy license files and documentation if available
$LicenseCandidates = @("LICENSE", "LICENSE-MIT", "LICENSE-APACHE", "README.md")
foreach ($lic in $LicenseCandidates) {
    $cand = Join-Path $ScriptDir $lic
    if (-not (Test-Path $cand)) { $cand = Join-Path $RepoRoot $lic }
    if (Test-Path $cand) {
        Safe-CopyFile $cand (Join-Path $Prefix $lic) $lic
    }
}

# 6. Copy docs and examples if available
$ExamplesSrc = Join-Path $ScriptDir "examples"
if (-not (Test-Path $ExamplesSrc)) { $ExamplesSrc = Join-Path $RepoRoot "examples" }
if (Test-Path $ExamplesSrc) {
    $ExamplesDest = Join-Path $Prefix "examples"
    Get-ChildItem -Path $ExamplesSrc -Recurse | ForEach-Object {
        $rel = $_.FullName.Substring($ExamplesSrc.Length).TrimStart('\', '/')
        $dest = Join-Path $ExamplesDest $rel
        if ($_.PSIsContainer) {
            if (-not (Test-Path $dest)) {
                New-Item -ItemType Directory -Path $dest -Force | Out-Null
            }
        } else {
            Safe-CopyFile $_.FullName $dest ("examples/$($rel.Replace('\', '/'))")
        }
    }
}

# 7. Verification of Installed Executables (BOTH compiler and driver validated)
Write-Host "--> Verifying installed executables..." -ForegroundColor Cyan

$vantacOutFile = Join-Path $env:TEMP ("vantac_ver_" + $PID + ".txt")
$vantacErrFile = Join-Path $env:TEMP ("vantac_err_" + $PID + ".txt")
$vantacProc = Start-Process -FilePath $VantacInstalled -ArgumentList "--version" -Wait -PassThru -NoNewWindow -RedirectStandardOutput $vantacOutFile -RedirectStandardError $vantacErrFile
$vantacExit = $vantacProc.ExitCode
$vantacVerOutput = if (Test-Path $vantacOutFile) { (Get-Content -Path $vantacOutFile -Raw).Trim() } else { "" }
Remove-Item $vantacOutFile, $vantacErrFile -Force -ErrorAction SilentlyContinue

if ($vantacExit -ne 0 -or [string]::IsNullOrWhiteSpace($vantacVerOutput)) {
    Rollback-Installation $InstalledFiles $Prefix
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
    Rollback-Installation $InstalledFiles $Prefix
    Write-Error ("Verification failed: vanta.exe --version failed with exit code {0}. Output: '{1}'" -f $vantaExit, $vantaVerOutput)
    exit 1
}

# Parse version tokens from "vantac <version>" and "vanta <version>"
$vMatch = [regex]::Match($vantacVerOutput, '^(?:vantac|ventac)\s+([^\s]+)$')
if (-not $vMatch.Success) {
    Rollback-Installation $InstalledFiles $Prefix
    Write-Error ("Malformed version string from vantac.exe: '{0}'" -f $vantacVerOutput)
    exit 1
}
$vantacReportedVersion = $vMatch.Groups[1].Value

$vantaMatch = [regex]::Match($vantaVerOutput, '^(?:vanta|venta)\s+([^\s]+)$')
if (-not $vantaMatch.Success) {
    Rollback-Installation $InstalledFiles $Prefix
    Write-Error ("Malformed version string from vanta.exe: '{0}'" -f $vantaVerOutput)
    exit 1
}
$vantaReportedVersion = $vantaMatch.Groups[1].Value

# Validate BOTH reported versions against requested Version and against each other
$normExpected = Normalize-VantaVersion $Version
$normVantac = Normalize-VantaVersion $vantacReportedVersion
$normVanta = Normalize-VantaVersion $vantaReportedVersion

if ($normVantac -ne $normExpected) {
    Rollback-Installation $InstalledFiles $Prefix
    Write-Error ("Compiler version mismatch: requested version '{0}' (normalized '{1}'), but vantac.exe reported '{2}' (normalized '{3}')" -f $Version, $normExpected, $vantacReportedVersion, $normVantac)
    exit 1
}

if ($normVanta -ne $normExpected) {
    Rollback-Installation $InstalledFiles $Prefix
    Write-Error ("Driver version mismatch: requested version '{0}' (normalized '{1}'), but vanta.exe reported '{2}' (normalized '{3}')" -f $Version, $normExpected, $vantaReportedVersion, $normVanta)
    exit 1
}

if ($normVantac -ne $normVanta) {
    Rollback-Installation $InstalledFiles $Prefix
    Write-Error ("Version discrepancy between compiler and driver: vantac.exe reported '{0}', vanta.exe reported '{1}'" -f $vantacReportedVersion, $vantaReportedVersion)
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

# 9. Write install-manifest.json ONLY after all verification has passed
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
Write-Host "  [OK] Vanta Toolchain Installed Successfully on Windows!    " -ForegroundColor Green
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
