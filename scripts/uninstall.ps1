<#
.SYNOPSIS
    Official Vanta Toolchain Windows PowerShell Uninstaller
.DESCRIPTION
    Safely removes the installed Vanta toolchain binaries, libraries, and PATH configuration.
.PARAMETER Prefix
    Installation directory to remove. Defaults to "$HOME\.vanta"
#>

[CmdletBinding()]
param (
    [string]$Prefix = "$HOME\.vanta"
)

$ErrorActionPreference = "Stop"

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  🧹 Vanta Toolchain Windows Uninstaller                    " -ForegroundColor Yellow
Write-Host "==========================================================" -ForegroundColor Cyan

if (-not (Test-Path $Prefix)) {
    Write-Warning "Vanta installation prefix was not found at '$Prefix'."
    exit 0
}

$BinDir = Join-Path $Prefix "bin"
$ManifestPath = Join-Path $Prefix "install-manifest.json"

# 1. Remove tracked files from manifest if present
if (Test-Path $ManifestPath) {
    Write-Host "--> Reading install manifest: $ManifestPath" -ForegroundColor Cyan
    try {
        $Manifest = Get-Content -Raw -Path $ManifestPath -Encoding UTF8 | ConvertFrom-Json
        if ($Manifest.files) {
            foreach ($relFile in $Manifest.files) {
                $targetFile = Join-Path $Prefix ($relFile.Replace('/', '\'))
                if (Test-Path $targetFile) {
                    Remove-Item -Path $targetFile -Force -ErrorAction SilentlyContinue
                    Write-Host "    Removed: $relFile" -ForegroundColor DarkGray
                }
            }
        }
    } catch {
        Write-Warning "Could not parse install manifest: $($_.Exception.Message)"
    }
    Remove-Item -Path $ManifestPath -Force -ErrorAction SilentlyContinue
}

# 2. Clean known directories
$KnownDirs = @(
    (Join-Path $Prefix "bin"),
    (Join-Path $Prefix "lib\vanta\std"),
    (Join-Path $Prefix "lib\vanta"),
    (Join-Path $Prefix "lib"),
    (Join-Path $Prefix "examples"),
    (Join-Path $Prefix "docs")
)

foreach ($dir in $KnownDirs) {
    if (Test-Path $dir) {
        Remove-Item -Recurse -Force -Path $dir -ErrorAction SilentlyContinue
    }
}

# If Prefix is now empty or contains only empty dirs, remove it
$remaining = Get-ChildItem -Path $Prefix -Recurse -ErrorAction SilentlyContinue
if (-not $remaining -or $remaining.Count -eq 0) {
    Remove-Item -Recurse -Force -Path $Prefix -ErrorAction SilentlyContinue
    Write-Host "--> Removed prefix directory: $Prefix" -ForegroundColor Green
} else {
    Write-Host "--> Remaining custom user files preserved in: $Prefix" -ForegroundColor Yellow
}

# 3. Clean User PATH
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath) {
    $PathEntries = $UserPath -split ';' | Where-Object { $_ -ne "" }
    $NewEntries = [System.Collections.Generic.List[string]]::new()
    $RemovedFromPath = $false

    foreach ($entry in $PathEntries) {
        if ($entry.TrimEnd('\') -eq $BinDir.TrimEnd('\') -or $entry.TrimEnd('\') -eq $Prefix.TrimEnd('\')) {
            $RemovedFromPath = $true
        } else {
            $NewEntries.Add($entry)
        }
    }

    if ($RemovedFromPath) {
        $UpdatedPath = ($NewEntries -join ';')
        [Environment]::SetEnvironmentVariable("Path", $UpdatedPath, "User")
        Write-Host "--> Removed $BinDir from User PATH environment variable." -ForegroundColor Green
        
        # Update current process PATH
        $currentEntries = $env:Path -split ';' | Where-Object { $_ -ne "" -and $_.TrimEnd('\') -ne $BinDir.TrimEnd('\') }
        $env:Path = ($currentEntries -join ';')
    }
}

Write-Host ""
Write-Host "==========================================================" -ForegroundColor Green
Write-Host "  ✨ Vanta Toolchain Uninstalled Successfully!              " -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green
