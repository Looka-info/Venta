<#
.SYNOPSIS
    Official Vanta Toolchain Windows PowerShell Uninstaller
.DESCRIPTION
    Safely and idempotently removes installed Vanta toolchain files and PATH configuration.
    Guarantees:
    - Never deletes untracked user files.
    - Never recursively removes directories containing user data.
    - Only removes parent directories when completely empty.
    - Only removes User PATH entries that were added by this specific installation.
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
    Write-Host ("--> Vanta installation prefix was not found at '{0}'. Nothing to remove." -f $Prefix) -ForegroundColor Yellow
    exit 0
}

$BinDir = Join-Path $Prefix "bin"
$ManifestPath = Join-Path $Prefix "install-manifest.json"

$ShouldRemovePath = $false
$BinDirToRemove = $BinDir

# 1. Parse install-manifest.json and remove ONLY tracked files
if (Test-Path $ManifestPath) {
    Write-Host ("--> Reading install manifest: {0}" -f $ManifestPath) -ForegroundColor Cyan
    try {
        $Manifest = Get-Content -Raw -Path $ManifestPath -Encoding UTF8 | ConvertFrom-Json
        if ($Manifest.path_added -eq $true) {
            $ShouldRemovePath = $true
            if ($Manifest.bin_dir) {
                $BinDirToRemove = $Manifest.bin_dir
            }
        }

        if ($Manifest.files) {
            foreach ($relFile in $Manifest.files) {
                $targetFile = Join-Path $Prefix ($relFile.Replace('/', '\'))
                # Path traversal guard: ensure target is within Prefix
                $fullTarget = [System.IO.Path]::GetFullPath($targetFile)
                $fullPrefix = [System.IO.Path]::GetFullPath($Prefix)
                if (-not $fullTarget.StartsWith($fullPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
                    Write-Warning ("Skipping invalid manifest path outside prefix: {0}" -f $relFile)
                    continue
                }

                if (Test-Path -LiteralPath $targetFile -PathType Leaf) {
                    Remove-Item -LiteralPath $targetFile -Force -ErrorAction SilentlyContinue
                    Write-Host ("    Removed: {0}" -f $relFile) -ForegroundColor DarkGray
                }
            }
        }
    } catch {
        Write-Warning ("Could not parse install manifest: {0}" -f $_.Exception.Message)
    }

    # Remove the manifest itself
    Remove-Item -LiteralPath $ManifestPath -Force -ErrorAction SilentlyContinue
} else {
    Write-Warning "No install-manifest.json found. Cleaning only standard known Vanta binaries."
    $DefaultFiles = @(
        (Join-Path $BinDir "vantac.exe"),
        (Join-Path $BinDir "vanta.exe"),
        (Join-Path $Prefix "uninstall.ps1")
    )
    foreach ($df in $DefaultFiles) {
        if (Test-Path -LiteralPath $df -PathType Leaf) {
            Remove-Item -LiteralPath $df -Force -ErrorAction SilentlyContinue
            Write-Host ("    Removed: {0}" -f (Split-Path -Leaf $df)) -ForegroundColor DarkGray
        }
    }
}

# 2. Bottom-up directory cleanup: NEVER recursively delete directories!
# Only delete a directory if it is completely empty. Untracked user files are strictly preserved.
if (Test-Path $Prefix) {
    $allDirs = Get-ChildItem -Path $Prefix -Directory -Recurse -Force -ErrorAction SilentlyContinue |
        Sort-Object -Property { $_.FullName.Length } -Descending

    foreach ($d in $allDirs) {
        if (Test-Path -LiteralPath $d.FullName) {
            $items = Get-ChildItem -LiteralPath $d.FullName -Force -ErrorAction SilentlyContinue
            if ($null -eq $items -or $items.Count -eq 0) {
                Remove-Item -LiteralPath $d.FullName -Force -ErrorAction SilentlyContinue
            }
        }
    }

    # Check root prefix directory
    $remainingRoot = Get-ChildItem -LiteralPath $Prefix -Force -ErrorAction SilentlyContinue
    if ($null -eq $remainingRoot -or $remainingRoot.Count -eq 0) {
        Remove-Item -LiteralPath $Prefix -Force -ErrorAction SilentlyContinue
        Write-Host ("--> Removed empty prefix directory: {0}" -f $Prefix) -ForegroundColor Green
    } else {
        Write-Host ("--> Preserved directory containing user-created files: {0}" -f $Prefix) -ForegroundColor Yellow
        foreach ($rem in $remainingRoot) {
            Write-Host ("    Preserved: {0}" -f $rem.Name) -ForegroundColor DarkYellow
        }
    }
}

# 3. Clean User PATH ONLY if this installation actually added it
if ($ShouldRemovePath) {
    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($UserPath) {
        $PathEntries = $UserPath -split ';' | Where-Object { $_ -ne "" }
        $NewEntries = [System.Collections.Generic.List[string]]::new()
        $Removed = $false

        foreach ($entry in $PathEntries) {
            if ($entry.TrimEnd('\') -ieq $BinDirToRemove.TrimEnd('\')) {
                $Removed = $true
            } else {
                $NewEntries.Add($entry)
            }
        }

        if ($Removed) {
            $UpdatedPath = ($NewEntries -join ';')
            [Environment]::SetEnvironmentVariable("Path", $UpdatedPath, "User")
            Write-Host ("--> Removed {0} from User PATH environment variable." -f $BinDirToRemove) -ForegroundColor Green

            # Update current session PATH
            $currentEntries = $env:Path -split ';' | Where-Object { $_ -ne "" -and $_.TrimEnd('\') -ine $BinDirToRemove.TrimEnd('\') }
            $env:Path = ($currentEntries -join ';')
        }
    }
} else {
    Write-Host "--> Preserved User PATH (entry was not added by this installation)." -ForegroundColor Cyan
}

Write-Host ""
Write-Host "==========================================================" -ForegroundColor Green
Write-Host "  ✨ Vanta Toolchain Uninstalled Successfully!              " -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green
