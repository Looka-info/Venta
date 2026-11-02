<#
.SYNOPSIS
    Official Vanta Toolchain Windows PowerShell Uninstaller
.DESCRIPTION
    Safely and idempotently removes installed Vanta toolchain files and PATH configuration.
    Guarantees:
    - Fails closed: Requires valid install-manifest.json to establish file ownership.
    - Strict path containment: Rejects rooted, drive-qualified, UNC, prefix-escaping, or reparse-point paths.
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

# 1. Normalize prefix
$fullPrefix = [System.IO.Path]::GetFullPath($Prefix).TrimEnd('\', '/')
$normPrefixWithSep = $fullPrefix + [System.IO.Path]::DirectorySeparatorChar

if (-not (Test-Path -LiteralPath $fullPrefix)) {
    Write-Host ("--> Vanta installation prefix was not found at '{0}'. Nothing to remove." -f $fullPrefix) -ForegroundColor Yellow
    exit 0
}

$ManifestPath = [System.IO.Path]::Combine($fullPrefix, "install-manifest.json")

# Helper: Test for reparse points (symlinks/junctions) resolving outside prefix
function Test-ReparsePointSafe([string]$targetPath, [string]$prefixBoundary) {
    $cur = $targetPath
    $boundaryLen = $prefixBoundary.TrimEnd('\').Length
    while ($cur -and $cur.Length -ge $boundaryLen) {
        if (Test-Path -LiteralPath $cur) {
            $item = Get-Item -LiteralPath $cur -Force -ErrorAction SilentlyContinue
            if ($item -and ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
                $target = $item.Target
                if ($target) {
                    $targetNorm = [System.IO.Path]::GetFullPath($target).TrimEnd('\', '/') + '\'
                    if (-not $targetNorm.StartsWith($prefixBoundary, [System.StringComparison]::OrdinalIgnoreCase)) {
                        return $false
                    }
                }
            }
        }
        $parent = [System.IO.Path]::GetDirectoryName($cur)
        if ($parent -eq $cur) { break }
        $cur = $parent
    }
    return $true
}

# 2. Fail-closed ownership verification
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

# 3. Parse manifest
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

# 4. Validate bin_dir for PATH removal
$expectedBinDir = [System.IO.Path]::Combine($fullPrefix, "bin")
$ShouldRemovePath = $false
$BinDirToRemove = $expectedBinDir

if ($Manifest.path_added -eq $true) {
    if ($Manifest.bin_dir) {
        $normManifestBin = [System.IO.Path]::GetFullPath($Manifest.bin_dir).TrimEnd('\', '/')
        if ($normManifestBin -ieq $expectedBinDir) {
            $ShouldRemovePath = $true
            $BinDirToRemove = $normManifestBin
        } else {
            Write-Warning ("install-manifest.json contains invalid or mismatching bin_dir: '{0}' (expected '{1}'). User PATH will NOT be modified." -f $Manifest.bin_dir, $expectedBinDir)
        }
    }
}

# 5. Strict pre-validation of all manifest paths (Path containment & traversal defense)
Write-Host ("--> Validating install manifest: {0}" -f $ManifestPath) -ForegroundColor Cyan
$validatedTargets = [System.Collections.Generic.List[object]]::new()

foreach ($relFile in $Manifest.files) {
    if ([string]::IsNullOrWhiteSpace($relFile)) { continue }

    # Reject rooted, drive-qualified, or UNC paths
    if ([System.IO.Path]::IsPathRooted($relFile) -or $relFile -match '^[a-zA-Z]:' -or $relFile.StartsWith('\\') -or $relFile.StartsWith('//')) {
        Write-Error ("Security violation: manifest contains rooted, drive-qualified, or UNC path: '{0}'. Uninstallation aborted with no files deleted." -f $relFile)
        exit 1
    }

    $combined = [System.IO.Path]::Combine($fullPrefix, $relFile.Replace('/', '\'))
    $candidate = [System.IO.Path]::GetFullPath($combined)

    # Enforce strict directory boundary containment
    if (-not $candidate.StartsWith($normPrefixWithSep, [System.StringComparison]::OrdinalIgnoreCase)) {
        Write-Error ("Security violation: manifest path escapes installation prefix: '{0}' resolves to '{1}'. Uninstallation aborted with no files deleted." -f $relFile, $candidate)
        exit 1
    }

    # Reparse point escape check
    if (-not (Test-ReparsePointSafe $candidate $normPrefixWithSep)) {
        Write-Error ("Security violation: manifest path '{0}' traverses a reparse point (symlink/junction) pointing outside prefix. Uninstallation aborted with no files deleted." -f $relFile)
        exit 1
    }

    $validatedTargets.Add([PSCustomObject]@{
        Relative = $relFile
        FullPath = $candidate
    })
}

# 6. Execute file deletions
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

# 7. Remove the manifest itself only after all files succeeded
Remove-Item -LiteralPath $ManifestPath -Force -ErrorAction Stop
Write-Host "    Removed: install-manifest.json" -ForegroundColor DarkGray

# 8. Bottom-up directory cleanup: Only delete directories if completely empty
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

# 9. Clean User PATH ONLY if validated and path_added = true
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
    Write-Host "--> Preserved User PATH (entry was not added by this installation or was not validated)." -ForegroundColor Cyan
}

Write-Host ""
Write-Host "==========================================================" -ForegroundColor Green
Write-Host "  [OK] Vanta Toolchain Uninstalled Successfully!            " -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green
