<#
.SYNOPSIS
    Official Vanta Toolchain Windows Packaging Script
.DESCRIPTION
    Builds, audits, and packages the native Windows x86-64 release distribution of Vanta:
    - vantac.exe (Vanta compiler, PE32+ AMD64)
    - vanta.exe (Vanta CLI driver, PE32+ AMD64)
    - lib/vanta/std/ (standard library)
    - install.ps1 and uninstall.ps1 (hardened installer and uninstaller)
    - LICENSE, LICENSE-MIT, LICENSE-APACHE (dual-licensing)
    - README.md, docs/, examples/
    - release-manifest.json (cryptographic hash inventory with per-file SHA-256 and size)
    - ZIP archive and independent SHA-256 sidecar
.PARAMETER Version
    Package version string. Defaults to version from root Cargo.toml (1.0.0-rc.2)
.PARAMETER Tag
    Release tag convention. Defaults to "v1.0-RC2"
.PARAMETER Target
    Target triple. Defaults to "x86_64-pc-windows-msvc"
.PARAMETER SkipBuild
    If specified, skips running 'cargo build --release'
#>

[CmdletBinding()]
param (
    [string]$Version = "",
    [string]$Tag = "v1.0-RC2",
    [string]$Target = "x86_64-pc-windows-msvc",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

# Helper: Strict PE32+ AMD64 Header Verification
function Test-Pe32PlusExecutable([string]$FilePath) {
    if (-not (Test-Path $FilePath)) {
        Write-Error "Executable not found: $FilePath"
        exit 1
    }
    $bytes = [System.IO.File]::ReadAllBytes($FilePath)
    if ($bytes.Length -lt 256) {
        Write-Error "File too small to be PE32+ executable: $($bytes.Length) bytes ($FilePath)"
        exit 1
    }

    # 1. DOS MZ signature (0x5A4D)
    $dosMagic = [BitConverter]::ToUInt16($bytes, 0)
    if ($dosMagic -ne 0x5A4D) {
        Write-Error ("Invalid DOS header magic: 0x{0:X4} (expected 0x5A4D) in {1}" -f $dosMagic, $FilePath)
        exit 1
    }

    # 2. e_lfanew offset to PE Header (offset 0x3C, 4 bytes)
    $e_lfanew = [BitConverter]::ToInt32($bytes, 0x3C)
    if ($e_lfanew -lt 64 -or ($e_lfanew + 120) -gt $bytes.Length) {
        Write-Error ("Out-of-bounds e_lfanew offset: {0} in {1}" -f $e_lfanew, $FilePath)
        exit 1
    }

    # 3. PE signature ("PE\0\0")
    $peSig = [System.Text.Encoding]::ASCII.GetString($bytes, $e_lfanew, 4)
    if ($peSig -ne "PE`0`0") {
        Write-Error ("Invalid PE signature: '{0}' in {1}" -f $peSig, $FilePath)
        exit 1
    }

    # 4. Machine field (offset e_lfanew + 4, 0x8664 = IMAGE_FILE_MACHINE_AMD64)
    $machine = [BitConverter]::ToUInt16($bytes, $e_lfanew + 4)
    if ($machine -ne 0x8664) {
        Write-Error ("Invalid Machine architecture: 0x{0:X4} (expected 0x8664 AMD64) in {1}" -f $machine, $FilePath)
        exit 1
    }

    # 5. Optional Header Magic (offset e_lfanew + 24, 0x020B = PE32+)
    $optMagic = [BitConverter]::ToUInt16($bytes, $e_lfanew + 24)
    if ($optMagic -ne 0x020B) {
        Write-Error ("Invalid Optional Header Magic: 0x{0:X4} (expected 0x020B PE32+) in {1}" -f $optMagic, $FilePath)
        exit 1
    }

    # 6. Windows Subsystem (offset e_lfanew + 24 + 68, 0x0003 = Windows CUI)
    $subsystem = [BitConverter]::ToUInt16($bytes, $e_lfanew + 24 + 68)
    if ($subsystem -ne 0x0003) {
        Write-Error ("Invalid Subsystem: 0x{0:X4} (expected 0x0003 Windows CUI) in {1}" -f $subsystem, $FilePath)
        exit 1
    }

    Write-Host ("--> PE32+ Header Verified: {0} (AMD64, PE32+, CUI)" -f (Split-Path -Leaf $FilePath)) -ForegroundColor Green
    return $true
}

$ScriptDir = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Definition }
$RepoRoot = if ($ScriptDir) { Split-Path -Parent $ScriptDir } else { $PWD }

# Auto-detect version from Cargo.toml if not provided
if (-not $Version) {
    $CargoToml = Join-Path $RepoRoot "Cargo.toml"
    if (Test-Path $CargoToml) {
        $content = Get-Content -Path $CargoToml -Raw
        if ($content -match '(?m)^version\s*=\s*"([^"]+)"') {
            $Version = $Matches[1]
        }
    }
    if (-not $Version) {
        $Version = "1.0.0-rc.2"
    }
}

$DistDir = Join-Path $RepoRoot "dist"
$PkgBaseName = "vanta-$Tag-$Target"
$StageDir = Join-Path $DistDir $PkgBaseName
$ZipPath = Join-Path $DistDir "$PkgBaseName.zip"
$Sha256Path = "$ZipPath.sha256"

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host ("  [PKG] Packaging Vanta {0} ({1}) for {2}" -f $Version, $Tag, $Target) -ForegroundColor Yellow
Write-Host "==========================================================" -ForegroundColor Cyan

# 1. Build release binaries if not skipped
if (-not $SkipBuild) {
    Write-Host "--> Building release binaries with Cargo (vantac, vanta)..." -ForegroundColor Cyan
    Push-Location $RepoRoot
    try {
        & cargo build --release --bin vantac --bin vanta
        if ($LASTEXITCODE -ne 0) {
            Write-Error "Cargo build failed with exit code $LASTEXITCODE"
            exit $LASTEXITCODE
        }
    } finally {
        Pop-Location
    }
}

# 2. Locate built release binaries
$ReleaseDir = Join-Path $RepoRoot "target\release"
$VantacExe = Join-Path $ReleaseDir "vantac.exe"
$VantaExe = Join-Path $ReleaseDir "vanta.exe"

if (-not (Test-Path $VantacExe) -or -not (Test-Path $VantaExe)) {
    # Check alternate target release dir if custom target was passed
    $AltReleaseDir = Join-Path $RepoRoot "target\$Target\release"
    if (Test-Path (Join-Path $AltReleaseDir "vantac.exe")) {
        $VantacExe = Join-Path $AltReleaseDir "vantac.exe"
        $VantaExe = Join-Path $AltReleaseDir "vanta.exe"
    } else {
        Write-Error "Release binaries not found in $ReleaseDir or $AltReleaseDir. Run with build enabled first."
        exit 1
    }
}

# Audit PE32+ headers of release binaries before staging
Test-Pe32PlusExecutable $VantacExe
Test-Pe32PlusExecutable $VantaExe

# 3. Validate Dual-License Files
Write-Host "--> Validating license files..." -ForegroundColor Cyan
$LicenseMit = Join-Path $RepoRoot "LICENSE-MIT"
$LicenseApache = Join-Path $RepoRoot "LICENSE-APACHE"
$LicenseRoot = Join-Path $RepoRoot "LICENSE"

if (-not (Test-Path $LicenseMit) -or (Get-Item $LicenseMit).Length -eq 0) {
    Write-Error "Packaging failed: LICENSE-MIT is missing or empty."
    exit 1
}
if (-not (Test-Path $LicenseApache) -or (Get-Item $LicenseApache).Length -eq 0) {
    Write-Error "Packaging failed: LICENSE-APACHE is missing or empty."
    exit 1
}
if (-not (Test-Path $LicenseRoot) -or (Get-Item $LicenseRoot).Length -eq 0) {
    Write-Error "Packaging failed: LICENSE pointer file is missing or empty."
    exit 1
}

# 4. Prepare clean staging directory layout
Write-Host "--> Preparing staging layout in $StageDir..." -ForegroundColor Cyan
if (Test-Path $StageDir) {
    Remove-Item -Recurse -Force $StageDir
}
New-Item -ItemType Directory -Path (Join-Path $StageDir "bin") -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $StageDir "lib\vanta\std") -Force | Out-Null

# Copy binaries
Copy-Item -Path $VantacExe -Destination (Join-Path $StageDir "bin\vantac.exe") -Force
Copy-Item -Path $VantaExe -Destination (Join-Path $StageDir "bin\vanta.exe") -Force

# Copy standard library recursively (preserving collections/ etc.)
$StdSource = Join-Path $RepoRoot "library\std"
if (Test-Path (Join-Path $StdSource "mod.va")) {
    Write-Host "--> Packaging standard library from $StdSource..." -ForegroundColor Cyan
    $StdDest = Join-Path $StageDir "lib\vanta\std"
    Get-ChildItem -Path $StdSource -Recurse | ForEach-Object {
        $rel = $_.FullName.Substring($StdSource.Length).TrimStart('\', '/')
        $dest = Join-Path $StdDest $rel
        if ($_.PSIsContainer) {
            New-Item -ItemType Directory -Path $dest -Force | Out-Null
        } else {
            Copy-Item -Path $_.FullName -Destination $dest -Force
        }
    }
} else {
    Write-Error "Standard library source directory not found at $StdSource"
    exit 1
}

# Copy installer and uninstaller scripts
$InstallScript = Join-Path $RepoRoot "scripts\install.ps1"
if (-not (Test-Path $InstallScript)) {
    Write-Error "Missing scripts\install.ps1"
    exit 1
}
Copy-Item -Path $InstallScript -Destination (Join-Path $StageDir "install.ps1") -Force

$UninstallScript = Join-Path $RepoRoot "scripts\uninstall.ps1"
if (-not (Test-Path $UninstallScript)) {
    Write-Error "Missing scripts\uninstall.ps1"
    exit 1
}
Copy-Item -Path $UninstallScript -Destination (Join-Path $StageDir "uninstall.ps1") -Force

# Copy dual-license files and README
Copy-Item -Path $LicenseRoot -Destination (Join-Path $StageDir "LICENSE") -Force
Copy-Item -Path $LicenseMit -Destination (Join-Path $StageDir "LICENSE-MIT") -Force
Copy-Item -Path $LicenseApache -Destination (Join-Path $StageDir "LICENSE-APACHE") -Force

$ReadmeFile = Join-Path $RepoRoot "README.md"
if (Test-Path $ReadmeFile) {
    Copy-Item -Path $ReadmeFile -Destination (Join-Path $StageDir "README.md") -Force
}

# Copy documentation and examples
$DocsSource = Join-Path $RepoRoot "docs"
if (Test-Path $DocsSource) {
    $DocsDest = Join-Path $StageDir "docs"
    New-Item -ItemType Directory -Path $DocsDest -Force | Out-Null
    Get-ChildItem -Path $DocsSource -Recurse | ForEach-Object {
        $rel = $_.FullName.Substring($DocsSource.Length).TrimStart('\', '/')
        $dest = Join-Path $DocsDest $rel
        if ($_.PSIsContainer) {
            New-Item -ItemType Directory -Path $dest -Force | Out-Null
        } else {
            Copy-Item -Path $_.FullName -Destination $dest -Force
        }
    }
}

$ExamplesSource = Join-Path $RepoRoot "examples"
if (Test-Path $ExamplesSource) {
    $ExamplesDest = Join-Path $StageDir "examples"
    New-Item -ItemType Directory -Path $ExamplesDest -Force | Out-Null
    Get-ChildItem -Path $ExamplesSource -Recurse | ForEach-Object {
        $rel = $_.FullName.Substring($ExamplesSource.Length).TrimStart('\', '/')
        $dest = Join-Path $ExamplesDest $rel
        if ($_.PSIsContainer) {
            New-Item -ItemType Directory -Path $dest -Force | Out-Null
        } else {
            Copy-Item -Path $_.FullName -Destination $dest -Force
        }
    }
}

# 5. Generate Cryptographic File-Integrity Manifest (release-manifest.json)
# Calculate SHA-256 and byte sizes for ALL payload files before creating the manifest.
# The manifest itself is intentionally omitted from the list to avoid recursive self-hashing.
Write-Host "--> Computing SHA-256 file inventory for release manifest..." -ForegroundColor Cyan
$ManifestFiles = [System.Collections.Generic.List[object]]::new()
Get-ChildItem -Path $StageDir -Recurse -File | ForEach-Object {
    $rel = $_.FullName.Substring($StageDir.Length).TrimStart('\', '/').Replace('\', '/')
    $hash = (Get-FileHash -Path $_.FullName -Algorithm SHA256).Hash.ToLower()
    $size = $_.Length
    $ManifestFiles.Add([PSCustomObject]@{
        path = $rel
        sha256 = $hash
        size_bytes = $size
    })
}

$SortedManifestFiles = $ManifestFiles | Sort-Object -Property path

$ManifestObj = [PSCustomObject]@{
    name = "vanta"
    version = $Version
    tag = $Tag
    target = $Target
    binary_format = "PE32+ AMD64"
    created_at = (Get-Date -Format "o")
    compiler_executable = "bin/vantac.exe"
    driver_executable = "bin/vanta.exe"
    payload_hash_algorithm = "SHA-256"
    note = "This manifest provides a cryptographic hash inventory for all payload files in this release archive. The manifest file itself is omitted to prevent recursive hashing."
    files = @($SortedManifestFiles)
}

$ManifestPath = Join-Path $StageDir "release-manifest.json"
$ManifestObj | ConvertTo-Json -Depth 6 | Set-Content -Path $ManifestPath -Encoding UTF8
Write-Host ("--> Wrote release-manifest.json with {0} tracked payload files" -f $SortedManifestFiles.Count) -ForegroundColor Green

# Also mirror manifest into dist/ root
if (-not (Test-Path $DistDir)) { New-Item -ItemType Directory -Path $DistDir -Force | Out-Null }
Copy-Item -Path $ManifestPath -Destination (Join-Path $DistDir "release-manifest.json") -Force

# 6. Compress into ZIP archive with normalized forward-slash entry separators
Write-Host "--> Creating ZIP package with normalized forward-slash separators: $ZipPath..." -ForegroundColor Cyan
if (Test-Path $ZipPath) {
    Remove-Item -Force $ZipPath
}

[System.Reflection.Assembly]::LoadWithPartialName("System.IO.Compression") | Out-Null
[System.Reflection.Assembly]::LoadWithPartialName("System.IO.Compression.FileSystem") | Out-Null

$zipStream = [System.IO.File]::Open($ZipPath, [System.IO.FileMode]::Create)
$zipArchive = [System.IO.Compression.ZipArchive]::new($zipStream, [System.IO.Compression.ZipArchiveMode]::Create)

try {
    $stagePrefix = $StageDir.TrimEnd('\', '/')
    Get-ChildItem -Path $StageDir -Recurse -File | ForEach-Object {
        $fullPath = $_.FullName
        $relPath = $fullPath.Substring($stagePrefix.Length).TrimStart('\', '/')
        # Standards compliance: force forward slashes for all ZIP entry paths
        $entryName = $relPath.Replace('\', '/')
        
        $entry = $zipArchive.CreateEntry($entryName, [System.IO.Compression.CompressionLevel]::Optimal)
        $entryStream = $entry.Open()
        $fileStream = [System.IO.File]::OpenRead($fullPath)
        try {
            $fileStream.CopyTo($entryStream)
        } finally {
            $fileStream.Dispose()
            $entryStream.Dispose()
        }
    }
} finally {
    $zipArchive.Dispose()
    $zipStream.Dispose()
}

$ZipSize = (Get-Item $ZipPath).Length
Write-Host ("--> Created {0} ({1} bytes)" -f $ZipPath, $ZipSize) -ForegroundColor Green

# 7. Generate SHA-256 Checksum Sidecar
Write-Host "--> Generating SHA-256 checksum sidecar..." -ForegroundColor Cyan
$ZipHash = (Get-FileHash -Path $ZipPath -Algorithm SHA256).Hash.ToLower()
$zipFileName = [System.IO.Path]::GetFileName($ZipPath)
$Sha256Content = "{0}  {1}`n" -f $ZipHash, $zipFileName
[System.IO.File]::WriteAllText($Sha256Path, $Sha256Content, [System.Text.Encoding]::UTF8)
Write-Host ("--> SHA-256 Sidecar: {0}" -f $ZipHash) -ForegroundColor Green

# 8. Self-Verification of Package
Write-Host "--> Self-verifying package integrity and contents..." -ForegroundColor Cyan
# Verify sidecar checksum can be independently recomputed
$RecomputedZipHash = (Get-FileHash -Path $ZipPath -Algorithm SHA256).Hash.ToLower()
if ($RecomputedZipHash -ne $ZipHash) {
    Write-Error "ZIP checksum verification failed: recomputed hash does not match sidecar!"
    exit 1
}

# Verify that all ZIP entry names strictly use forward slashes (no Windows backslashes)
$zipInspect = [System.IO.Compression.ZipFile]::OpenRead($ZipPath)
try {
    foreach ($entry in $zipInspect.Entries) {
        if ($entry.FullName.Contains('\')) {
            Write-Error ("Archive entry contains Windows backslash: '{0}'" -f $entry.FullName)
            exit 1
        }
    }
    Write-Host ("--> ZIP entry separator check passed: all {0} entries use forward slashes" -f $zipInspect.Entries.Count) -ForegroundColor Green
} finally {
    $zipInspect.Dispose()
}

$VerifyDir = Join-Path $env:TEMP ("vanta_pkg_verify_" + $PID)
if (Test-Path $VerifyDir) {
    Remove-Item -Recurse -Force $VerifyDir
}
New-Item -ItemType Directory -Path $VerifyDir -Force | Out-Null

try {
    Expand-Archive -Path $ZipPath -DestinationPath $VerifyDir -Force

    # Verify presence of critical files
    $CheckFiles = @(
        "bin\vantac.exe",
        "bin\vanta.exe",
        "lib\vanta\std\mod.va",
        "install.ps1",
        "uninstall.ps1",
        "LICENSE",
        "LICENSE-MIT",
        "LICENSE-APACHE",
        "release-manifest.json"
    )

    foreach ($f in $CheckFiles) {
        $p = Join-Path $VerifyDir $f
        if (-not (Test-Path $p)) {
            Write-Error "Package verification failed: missing $f in extracted archive"
            exit 1
        }
        if ((Get-Item $p).Length -eq 0) {
            Write-Error ("Package verification failed: {0} in extracted archive is empty" -f $f)
            exit 1
        }
    }

    # Verify all manifest file hashes and sizes
    $ExtractedManifest = Get-Content -Raw -Path (Join-Path $VerifyDir "release-manifest.json") -Encoding UTF8 | ConvertFrom-Json
    foreach ($entry in $ExtractedManifest.files) {
        $targetFile = Join-Path $VerifyDir ($entry.path.Replace('/', '\'))
        if (-not (Test-Path $targetFile)) {
            Write-Error ("Manifest verification failed: file '{0}' not found in archive" -f $entry.path)
            exit 1
        }
        $actualSize = (Get-Item $targetFile).Length
        if ($actualSize -ne $entry.size_bytes) {
            Write-Error ("Manifest size mismatch for '{0}': expected {1}, got {2}" -f $entry.path, $entry.size_bytes, $actualSize)
            exit 1
        }
        $actualHash = (Get-FileHash -Path $targetFile -Algorithm SHA256).Hash.ToLower()
        if ($actualHash -ne $entry.sha256) {
            Write-Error ("Manifest hash mismatch for '{0}': expected {1}, got {2}" -f $entry.path, $entry.sha256, $actualHash)
            exit 1
        }
    }
    Write-Host ("--> Manifest verified: all {0} payload files match their SHA-256 hashes and sizes" -f $ExtractedManifest.files.Count) -ForegroundColor Green

    # Verify PE32+ headers of extracted binaries
    Test-Pe32PlusExecutable (Join-Path $VerifyDir "bin\vantac.exe")
    Test-Pe32PlusExecutable (Join-Path $VerifyDir "bin\vanta.exe")

    # Verify execution of extracted binaries with exit code check
    $vantacVerify = Join-Path $VerifyDir "bin\vantac.exe"
    $vantaVerify = Join-Path $VerifyDir "bin\vanta.exe"
    
    $p1 = Start-Process -FilePath $vantacVerify -ArgumentList "--version" -Wait -PassThru -NoNewWindow
    if ($p1.ExitCode -ne 0) {
        Write-Error ("vantac.exe --version failed with exit code {0}" -f $p1.ExitCode)
        exit 1
    }
    $p2 = Start-Process -FilePath $vantaVerify -ArgumentList "--version" -Wait -PassThru -NoNewWindow
    if ($p2.ExitCode -ne 0) {
        Write-Error ("vanta.exe --version failed with exit code {0}" -f $p2.ExitCode)
        exit 1
    }

    Write-Host "    Verified execution of extracted vantac and vanta" -ForegroundColor Green
} finally {
    Remove-Item -Recurse -Force $VerifyDir -ErrorAction SilentlyContinue
}

Write-Host ""
Write-Host "==========================================================" -ForegroundColor Green
Write-Host "  [OK] Vanta Windows Release Package Created Successfully!   " -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green
Write-Host ("Archive:  {0}" -f $ZipPath)
Write-Host ("Checksum: {0}" -f $Sha256Path)
Write-Host ("Size:     {0} bytes" -f $ZipSize)
Write-Host ("Hash:     {0}" -f $ZipHash)
