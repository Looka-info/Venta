<#
.SYNOPSIS
    Official Vanta Toolchain Windows PowerShell Uninstaller
.DESCRIPTION
    Safely and idempotently removes installed Vanta toolchain files and PATH configuration.
    Guarantees:
    - Fails closed: Requires valid install-manifest.json to establish file ownership.
    - Strict path containment: Rejects rooted, drive-qualified, UNC, prefix-escaping, or reparse-point paths.
    - Canonical path normalization: Correctly matches long, 8.3 short, and case-varying paths.
    - Validates bin_dir before modifying User PATH.
    - Preserves install-manifest.json if any file deletion fails.
    - Never deletes untracked user files or directories containing user data.
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
Write-Host "  [UNINSTALL] Vanta Toolchain Windows Uninstaller           " -ForegroundColor Yellow
Write-Host "==========================================================" -ForegroundColor Cyan

# -----------------------------------------------------------------------------
# Canonical Path Resolution & Equivalence Helpers
# -----------------------------------------------------------------------------

function Get-CanonicalPath([string]$rawPath) {
    if ([string]::IsNullOrWhiteSpace($rawPath)) { return "" }
    $expanded = [Environment]::ExpandEnvironmentVariables($rawPath.Trim()).Replace('/', '\')
    try {
        $full = [System.IO.Path]::GetFullPath($expanded)
    } catch {
        $full = $expanded
    }

    # Resolve 8.3 short paths to full long paths by walking existing ancestors
    $cur = $full.TrimEnd('\')
    $tail = [System.Collections.Generic.List[string]]::new()
    while ($cur -and -not (Test-Path -LiteralPath $cur)) {
        $leaf = [System.IO.Path]::GetFileName($cur)
        $parent = [System.IO.Path]::GetDirectoryName($cur)
        if (-not $parent -or $parent -eq $cur) { break }
        $tail.Insert(0, $leaf)
        $cur = $parent
    }

    if ($cur -and (Test-Path -LiteralPath $cur)) {
        try {
            $item = Get-Item -LiteralPath $cur -Force -ErrorAction SilentlyContinue
            if ($item -and $item.FullName) {
                $cur = $item.FullName
            }
        } catch {}
        if ($tail.Count -gt 0) {
            $full = [System.IO.Path]::Combine($cur, ($tail -join '\'))
        } else {
            $full = $cur
        }
    }
    return $full.TrimEnd('\')
}

function Test-PathEquivalent([string]$path1, [string]$path2) {
    if ([string]::IsNullOrWhiteSpace($path1) -or [string]::IsNullOrWhiteSpace($path2)) { return $false }
    $c1 = $path1.Trim().Replace('/', '\').TrimEnd('\')
    $c2 = $path2.Trim().Replace('/', '\').TrimEnd('\')
    if ($c1 -ieq $c2) { return $true }
    $canon1 = Get-CanonicalPath $path1
    $canon2 = Get-CanonicalPath $path2
    return ($canon1 -ieq $canon2)
}

# -----------------------------------------------------------------------------
# Reparse Point Safety Verification (Fail-Closed)
# -----------------------------------------------------------------------------

function Test-ReparsePointSafe([string]$targetPath, [string]$prefixBoundary) {
    $canonBoundary = (Get-CanonicalPath $prefixBoundary).TrimEnd('\') + '\'
    $cur = $targetPath
    $maxDepth = 32

    while ($cur -and $maxDepth -gt 0) {
        $maxDepth--
        if (Test-Path -LiteralPath $cur) {
            try {
                $item = Get-Item -LiteralPath $cur -Force -ErrorAction Stop
            } catch {
                # Cannot inspect path: fail closed
                return $false
            }

            if ($item -and ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
                $rawTarget = $null
                if ($item.PSObject.Properties['Target']) {
                    $rawTarget = $item.Target
                }
                if ($rawTarget -is [System.Collections.IEnumerable] -and $rawTarget -isnot [string]) {
                    $rawTarget = ($rawTarget | Select-Object -First 1)
                }

                if ($null -eq $rawTarget -or [string]::IsNullOrWhiteSpace($rawTarget.ToString())) {
                    # Reparse point target unresolvable: fail closed
                    return $false
                }

                $targetStr = $rawTarget.ToString()
                $resolved = if ([System.IO.Path]::IsPathRooted($targetStr)) {
                    [System.IO.Path]::GetFullPath($targetStr)
                } else {
                    $pDir = [System.IO.Path]::GetDirectoryName($cur)
                    [System.IO.Path]::GetFullPath((Join-Path $pDir $targetStr))
                }

                $canonTarget = (Get-CanonicalPath $resolved).TrimEnd('\') + '\'
                if (-not $canonTarget.StartsWith($canonBoundary, [System.StringComparison]::OrdinalIgnoreCase)) {
                    # Reparse point resolves outside installation prefix boundary
                    return $false
                }
            }
        }

        $canonCur = (Get-CanonicalPath $cur).TrimEnd('\') + '\'
        if ($canonCur -ieq $canonBoundary) { break }

        $parent = [System.IO.Path]::GetDirectoryName($cur)
        if (-not $parent -or $parent -eq $cur) { break }
        $cur = $parent
    }
    return $true
}

# -----------------------------------------------------------------------------
# 1. Normalize and Validate Installation Prefix
# -----------------------------------------------------------------------------

$fullPrefix = [System.IO.Path]::GetFullPath($Prefix).TrimEnd('\', '/')
$normPrefixWithSep = $fullPrefix + [System.IO.Path]::DirectorySeparatorChar

if (-not (Test-Path -LiteralPath $fullPrefix)) {
    Write-Host ("--> Vanta installation prefix was not found at '{0}'. Nothing to remove." -f $fullPrefix) -ForegroundColor Yellow
    exit 0
}

# Fail closed if the prefix itself is a reparse point pointing outside
if (Test-Path -LiteralPath $fullPrefix) {
    try {
        $prefixItem = Get-Item -LiteralPath $fullPrefix -Force -ErrorAction Stop
        if ($prefixItem -and ($prefixItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
            Write-Error ("Security violation: installation prefix '{0}' is itself a symbolic link or directory junction. Uninstallation aborted for safety." -f $fullPrefix)
            exit 1
        }
    } catch {
        Write-Error ("Security violation: failed to inspect installation prefix '{0}'. Uninstallation aborted for safety." -f $fullPrefix)
        exit 1
    }
}

$ManifestPath = [System.IO.Path]::Combine($fullPrefix, "install-manifest.json")

# -----------------------------------------------------------------------------
# 2. Fail-Closed Ownership Verification
# -----------------------------------------------------------------------------

if (-not (Test-Path -LiteralPath $ManifestPath -PathType Leaf)) {
    # Check if directory is completely empty
    $items = Get-ChildItem -LiteralPath $fullPrefix -Force -ErrorAction SilentlyContinue
    if ($null -eq $items -or $items.Count -eq 0) {
        Remove-Item -LiteralPath $fullPrefix -Force -ErrorAction SilentlyContinue
        Write-Host ("--> Removed empty prefix directory: {0}" -f $fullPrefix) -ForegroundColor Green
        exit 0
    }

    Write-Error ("Uninstallation failed: missing install-manifest.json in '{0}'. Cannot establish file ownership. No files were deleted." -f $fullPrefix)
    exit 1
}

# -----------------------------------------------------------------------------
# 3. Parse and Validate Manifest Schema
# -----------------------------------------------------------------------------

try {
    $Manifest = Get-Content -Raw -LiteralPath $ManifestPath -Encoding UTF8 | ConvertFrom-Json
} catch {
    Write-Error ("Uninstallation failed: install-manifest.json in '{0}' is invalid JSON: {1}. No files were deleted." -f $fullPrefix, $_.Exception.Message)
    exit 1
}

if ($null -eq $Manifest -or $null -eq $Manifest.files) {
    Write-Error ("Uninstallation failed: install-manifest.json in '{0}' has invalid schema or missing 'files' list. No files were deleted." -f $fullPrefix)
    exit 1
}

# -----------------------------------------------------------------------------
# 4. Validate bin_dir for PATH Removal
# -----------------------------------------------------------------------------

$expectedBinDir = [System.IO.Path]::Combine($fullPrefix, "bin")
$ShouldRemovePath = $false
$BinDirToRemove = $expectedBinDir

if ($Manifest.path_added -eq $true) {
    if ($Manifest.bin_dir) {
        if (Test-PathEquivalent $Manifest.bin_dir $expectedBinDir) {
            $ShouldRemovePath = $true
            $BinDirToRemove = $expectedBinDir
        } else {
            Write-Warning ("install-manifest.json contains invalid or mismatching bin_dir: '{0}' (expected '{1}'). User PATH will NOT be modified." -f $Manifest.bin_dir, $expectedBinDir)
        }
    }
}

# -----------------------------------------------------------------------------
# 5. Strict Pre-Validation of All Manifest Paths
# -----------------------------------------------------------------------------

Write-Host ("--> Validating install manifest: {0}" -f $ManifestPath) -ForegroundColor Cyan
$validatedTargets = [System.Collections.Generic.List[object]]::new()
$canonPrefixWithSep = (Get-CanonicalPath $fullPrefix).TrimEnd('\') + '\'

foreach ($relFile in $Manifest.files) {
    if ([string]::IsNullOrWhiteSpace($relFile)) { continue }

    # Reject rooted, drive-qualified, or UNC paths
    if ([System.IO.Path]::IsPathRooted($relFile) -or $relFile -match '^[a-zA-Z]:' -or $relFile.StartsWith('\\') -or $relFile.StartsWith('//')) {
        Write-Error ("Security violation: manifest contains rooted, drive-qualified, or UNC path: '{0}'. Uninstallation aborted with no files deleted." -f $relFile)
        exit 1
    }

    $combined = [System.IO.Path]::Combine($fullPrefix, $relFile.Replace('/', '\'))
    $candidate = [System.IO.Path]::GetFullPath($combined)
    $canonCandidate = Get-CanonicalPath $candidate

    # Enforce strict directory boundary containment (checking both normalized and canonical forms)
    if (-not $candidate.StartsWith($normPrefixWithSep, [System.StringComparison]::OrdinalIgnoreCase) -or
        -not ($canonCandidate + '\').StartsWith($canonPrefixWithSep, [System.StringComparison]::OrdinalIgnoreCase)) {
        Write-Error ("Security violation: manifest path escapes installation prefix: '{0}' resolves to '{1}'. Uninstallation aborted with no files deleted." -f $relFile, $candidate)
        exit 1
    }

    # Reparse point escape check
    if (-not (Test-ReparsePointSafe $candidate $fullPrefix)) {
        Write-Error ("Security violation: manifest path '{0}' traverses a reparse point (symlink/junction) pointing outside prefix. Uninstallation aborted with no files deleted." -f $relFile)
        exit 1
    }

    $validatedTargets.Add([PSCustomObject]@{
        Relative = $relFile
        FullPath = $candidate
    })
}

# -----------------------------------------------------------------------------
# 6. Execute File Deletions
# -----------------------------------------------------------------------------

$failedDeletions = [System.Collections.Generic.List[string]]::new()
$deletedCount = 0

foreach ($item in $validatedTargets) {
    if (Test-Path -LiteralPath $item.FullPath -PathType Leaf) {
        try {
            Remove-Item -LiteralPath $item.FullPath -Force -ErrorAction Stop
            Write-Host ("    Removed: {0}" -f $item.Relative) -ForegroundColor DarkGray
            $deletedCount++
        } catch {
            $failedDeletions.Add(("{0}: {1}" -f $item.Relative, $_.Exception.Message))
        }
    }
}

# If any file deletion failed, fail closed and PRESERVE the manifest
if ($failedDeletions.Count -gt 0) {
    Write-Error ("Uninstallation failed: {0} file(s) could not be removed:`n{1}`nManifest was preserved so uninstallation can be retried safely." -f $failedDeletions.Count, ($failedDeletions -join "`n"))
    exit 1
}

# -----------------------------------------------------------------------------
# 7. Remove Manifest Only After All Files Succeeded
# -----------------------------------------------------------------------------

Remove-Item -LiteralPath $ManifestPath -Force -ErrorAction Stop
Write-Host "    Removed: install-manifest.json" -ForegroundColor DarkGray

# -----------------------------------------------------------------------------
# 8. Bottom-Up Directory Cleanup (Only if completely empty)
# -----------------------------------------------------------------------------

$allDirs = Get-ChildItem -LiteralPath $fullPrefix -Directory -Recurse -Force -ErrorAction SilentlyContinue |
    Sort-Object -Property { $_.FullName.Length } -Descending

foreach ($d in $allDirs) {
    if (Test-Path -LiteralPath $d.FullName) {
        $rem = Get-ChildItem -LiteralPath $d.FullName -Force -ErrorAction SilentlyContinue
        if ($null -eq $rem -or $rem.Count -eq 0) {
            Remove-Item -LiteralPath $d.FullName -Force -ErrorAction SilentlyContinue
        }
    }
}

# Check root prefix directory
$remainingRoot = Get-ChildItem -LiteralPath $fullPrefix -Force -ErrorAction SilentlyContinue
if ($null -eq $remainingRoot -or $remainingRoot.Count -eq 0) {
    Remove-Item -LiteralPath $fullPrefix -Force -ErrorAction SilentlyContinue
    Write-Host ("--> Removed empty prefix directory: {0}" -f $fullPrefix) -ForegroundColor Green
} else {
    Write-Host ("--> Preserved directory containing user-created files: {0}" -f $fullPrefix) -ForegroundColor Yellow
    foreach ($r in $remainingRoot) {
        Write-Host ("    Preserved: {0}" -f $r.Name) -ForegroundColor DarkYellow
    }
}

# -----------------------------------------------------------------------------
# 9. Clean User PATH (Idempotent, Canonical Comparison)
# -----------------------------------------------------------------------------

if ($ShouldRemovePath) {
    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($UserPath) {
        $PathEntries = $UserPath -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
        $NewEntries = [System.Collections.Generic.List[string]]::new()
        $Removed = $false

        foreach ($entry in $PathEntries) {
            if (Test-PathEquivalent $entry $BinDirToRemove) {
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
            $currentEntries = $env:Path -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) -and -not (Test-PathEquivalent $_ $BinDirToRemove) }
            $env:Path = ($currentEntries -join ';')
        }
    }
} else {
    Write-Host "--> Preserved User PATH (entry was not added by this installation or was not validated)." -ForegroundColor Cyan
}

Write-Host ""
Write-Host "==========================================================" -ForegroundColor Green
Write-Host "  [OK] Vanta Toolchain Uninstalled Successfully!            " -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green
