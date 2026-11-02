<#
.SYNOPSIS
    Official Vanta Toolchain Windows Packaging Script
.DESCRIPTION
    Builds and packages the native Windows x86-64 release distribution of Vanta:
    - vantac.exe (Vanta compiler)
    - vanta.exe (Vanta CLI driver)
    - lib/vanta/std/ (standard library)
    - install.ps1 and uninstall.ps1
    - documentation and examples
    - release-manifest.json
    - ZIP archive and SHA-256 checksum file
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
Write-Host ("  Packaging Vanta {0} ({1}) for {2}" -f $Version, $Tag, $Target) -ForegroundColor Yellow
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

Write-Host "--> Found vantac.exe at: $VantacExe" -ForegroundColor Green
Write-Host "--> Found vanta.exe at:  $VantaExe" -ForegroundColor Green

# 3. Prepare clean staging directory layout
Write-Host "--> Preparing staging layout in $StageDir..." -ForegroundColor Cyan
if (Test-Path $StageDir) {
    Remove-Item -Recurse -Force $StageDir
}
New-Item -ItemType Directory -Path (Join-Path $StageDir "bin") -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $StageDir "lib\vanta\std") -Force | Out-Null

$PackagedFiles = [System.Collections.Generic.List[string]]::new()

# Copy binaries
Copy-Item -Path $VantacExe -Destination (Join-Path $StageDir "bin\vantac.exe") -Force
$PackagedFiles.Add("bin/vantac.exe")
Copy-Item -Path $VantaExe -Destination (Join-Path $StageDir "bin\vanta.exe") -Force
$PackagedFiles.Add("bin/vanta.exe")

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
            $PackagedFiles.Add("lib/vanta/std/$($rel.Replace('\', '/'))")
        }
    }
} else {
    Write-Warning "Standard library source directory not found at $StdSource"
}

# Copy installer and uninstaller scripts
$InstallScript = Join-Path $RepoRoot "scripts\install.ps1"
if (Test-Path $InstallScript) {
    Copy-Item -Path $InstallScript -Destination (Join-Path $StageDir "install.ps1") -Force
    $PackagedFiles.Add("install.ps1")
}

$UninstallScript = Join-Path $RepoRoot "scripts\uninstall.ps1"
if (Test-Path $UninstallScript) {
    Copy-Item -Path $UninstallScript -Destination (Join-Path $StageDir "uninstall.ps1") -Force
    $PackagedFiles.Add("uninstall.ps1")
}

# Copy documentation and examples
$ReadmeFile = Join-Path $RepoRoot "README.md"
if (Test-Path $ReadmeFile) {
    Copy-Item -Path $ReadmeFile -Destination (Join-Path $StageDir "README.md") -Force
    $PackagedFiles.Add("README.md")
}

$LicenseFile = Join-Path $RepoRoot "LICENSE"
if (Test-Path $LicenseFile) {
    Copy-Item -Path $LicenseFile -Destination (Join-Path $StageDir "LICENSE") -Force
    $PackagedFiles.Add("LICENSE")
}

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
            $PackagedFiles.Add("docs/$($rel.Replace('\', '/'))")
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
            $PackagedFiles.Add("examples/$($rel.Replace('\', '/'))")
        }
    }
}

# 4. Generate release-manifest.json
$ManifestObj = [PSCustomObject]@{
    name = "vanta"
    version = $Version
    tag = $Tag
    target = $Target
    binary_format = "PE32+ AMD64"
    created_at = (Get-Date -Format "o")
    binaries = @("bin/vantac.exe", "bin/vanta.exe")
    files = ($PackagedFiles | Select-Object -Unique)
}
$ManifestPath = Join-Path $StageDir "release-manifest.json"
$ManifestObj | ConvertTo-Json -Depth 5 | Set-Content -Path $ManifestPath -Encoding UTF8
$PackagedFiles.Add("release-manifest.json")

# Also copy manifest into $DistDir
Copy-Item -Path $ManifestPath -Destination (Join-Path $DistDir "release-manifest.json") -Force

# 5. Compress into ZIP archive
Write-Host "--> Creating ZIP package: $ZipPath..." -ForegroundColor Cyan
if (Test-Path $ZipPath) {
    Remove-Item -Force $ZipPath
}

# Create zip with all files inside $StageDir preserving relative paths
[System.Reflection.Assembly]::LoadWithPartialName("System.IO.Compression.FileSystem") | Out-Null
[System.IO.Compression.ZipFile]::CreateFromDirectory($StageDir, $ZipPath, [System.IO.Compression.CompressionLevel]::Optimal, $false)

$ZipSize = (Get-Item $ZipPath).Length
Write-Host ("--> Created {0} ({1} bytes)" -f $ZipPath, $ZipSize) -ForegroundColor Green

# 6. Generate SHA-256 Checksum
Write-Host "--> Generating SHA-256 checksum..." -ForegroundColor Cyan
$Hash = (Get-FileHash -Path $ZipPath -Algorithm SHA256).Hash.ToLower()
$zipFileName = [System.IO.Path]::GetFileName($ZipPath)
$Sha256Content = "{0}  {1}`n" -f $Hash, $zipFileName
[System.IO.File]::WriteAllText($Sha256Path, $Sha256Content, [System.Text.Encoding]::UTF8)
Write-Host "--> SHA-256: $Hash" -ForegroundColor Green

# 7. Self-Verification of Package
Write-Host "--> Verifying package contents..." -ForegroundColor Cyan
$VerifyDir = Join-Path $env:TEMP ("vanta_pkg_verify_" + $PID)
if (Test-Path $VerifyDir) {
    Remove-Item -Recurse -Force $VerifyDir
}
New-Item -ItemType Directory -Path $VerifyDir -Force | Out-Null

try {
    Expand-Archive -Path $ZipPath -DestinationPath $VerifyDir -Force

    $CheckFiles = @(
        "bin\vantac.exe",
        "bin\vanta.exe",
        "lib\vanta\std\mod.va",
        "install.ps1",
        "uninstall.ps1",
        "release-manifest.json"
    )

    foreach ($f in $CheckFiles) {
        $p = Join-Path $VerifyDir $f
        if (-not (Test-Path $p)) {
            Write-Error "Package verification failed: missing $f in extracted archive"
            exit 1
        }
    }

    # Verify execution of extracted binaries
    $vantacVerify = Join-Path $VerifyDir "bin\vantac.exe"
    $vantaVerify = Join-Path $VerifyDir "bin\vanta.exe"
    $ver1 = & $vantacVerify --version
    $ver2 = & $vantaVerify --version
    Write-Host "    Verified extracted vantac: $ver1" -ForegroundColor Green
    Write-Host "    Verified extracted vanta:  $ver2" -ForegroundColor Green
} finally {
    Remove-Item -Recurse -Force $VerifyDir -ErrorAction SilentlyContinue
}

Write-Host ""
Write-Host "==========================================================" -ForegroundColor Green
Write-Host "  ✅ Vanta Windows Release Package Created Successfully!     " -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green
Write-Host "Archive:  $ZipPath"
Write-Host "Checksum: $Sha256Path"
Write-Host "Size:     $ZipSize bytes"
Write-Host "Hash:     $Hash"
