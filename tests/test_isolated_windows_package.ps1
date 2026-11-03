# Test Suite: Isolated Windows Package Verification Gate (Milestone 27B)
# This test verifies that the packaged ZIP distribution can be extracted, verified,
# installed, used to compile standard library programs, and uninstalled in complete
# isolation from the git repository and local dev environment.

$ErrorActionPreference = "Stop"

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  [TEST] Running Isolated Windows Package Release Gate     " -ForegroundColor Yellow
Write-Host "==========================================================" -ForegroundColor Cyan

$ScriptDir = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Definition }
$RepoRoot = if ($ScriptDir) { Split-Path -Parent $ScriptDir } else { $PWD }

$ZipPath = Join-Path $RepoRoot "dist\vanta-v1.0-RC2-x86_64-pc-windows-msvc.zip"
$Sha256Path = "$ZipPath.sha256"

if (-not (Test-Path $ZipPath)) {
    throw "Release archive not found at $ZipPath. Run scripts\package_release_windows.ps1 first."
}
if (-not (Test-Path $Sha256Path)) {
    throw "SHA-256 sidecar not found at $Sha256Path."
}

# Helper: Strict PE32+ AMD64 Header Verification
function Assert-Pe32PlusExecutable([string]$FilePath) {
    if (-not (Test-Path $FilePath)) {
        throw "Executable not found: $FilePath"
    }
    $bytes = [System.IO.File]::ReadAllBytes($FilePath)
    if ($bytes.Length -lt 256) {
        throw "File too small to be PE32+ executable: $($bytes.Length) bytes ($FilePath)"
    }

    $dosMagic = [BitConverter]::ToUInt16($bytes, 0)
    if ($dosMagic -ne 0x5A4D) {
        throw ("Invalid DOS header magic: 0x{0:X4} (expected 0x5A4D) in {1}" -f $dosMagic, $FilePath)
    }

    $e_lfanew = [BitConverter]::ToInt32($bytes, 0x3C)
    if ($e_lfanew -lt 64 -or ($e_lfanew + 120) -gt $bytes.Length) {
        throw ("Out-of-bounds e_lfanew offset: {0} in {1}" -f $e_lfanew, $FilePath)
    }

    $peSig = [System.Text.Encoding]::ASCII.GetString($bytes, $e_lfanew, 4)
    if ($peSig -ne "PE`0`0") {
        throw ("Invalid PE signature: '{0}' in {1}" -f $peSig, $FilePath)
    }

    $machine = [BitConverter]::ToUInt16($bytes, $e_lfanew + 4)
    if ($machine -ne 0x8664) {
        throw ("Invalid Machine architecture: 0x{0:X4} (expected 0x8664 AMD64) in {1}" -f $machine, $FilePath)
    }

    $optMagic = [BitConverter]::ToUInt16($bytes, $e_lfanew + 24)
    if ($optMagic -ne 0x020B) {
        throw ("Invalid Optional Header Magic: 0x{0:X4} (expected 0x020B PE32+) in {1}" -f $optMagic, $FilePath)
    }

    $subsystem = [BitConverter]::ToUInt16($bytes, $e_lfanew + 24 + 68)
    if ($subsystem -ne 0x0003) {
        throw ("Invalid Subsystem: 0x{0:X4} (expected 0x0003 Windows CUI) in {1}" -f $subsystem, $FilePath)
    }

    Write-Host ("    PE32+ Header Verified: {0} (AMD64, PE32+, CUI)" -f (Split-Path -Leaf $FilePath)) -ForegroundColor Green
}

$IsolatedRoot = Join-Path $env:TEMP ("vanta_isolated_gate_" + (Get-Random))
New-Item -ItemType Directory -Path $IsolatedRoot -Force | Out-Null

$origUserPath = [Environment]::GetEnvironmentVariable("Path", "User")

try {
    # -------------------------------------------------------------
    # Step 1: Verify ZIP SHA-256 sidecar
    # -------------------------------------------------------------
    Write-Host "--> Step 1: Verifying ZIP SHA-256 sidecar integrity..." -ForegroundColor Cyan
    $actualZipHash = (Get-FileHash -Path $ZipPath -Algorithm SHA256).Hash.ToLower()
    $sidecarRaw = (Get-Content -Path $Sha256Path -Raw).Trim()
    $expectedZipHash = ($sidecarRaw -split '\s+')[0].ToLower()

    if ($actualZipHash -ne $expectedZipHash) {
        throw ("SHA-256 checksum mismatch for {0}: expected {1}, calculated {2}" -f $ZipPath, $expectedZipHash, $actualZipHash)
    }
    Write-Host ("    ZIP Checksum Matches Sidecar: {0}" -f $actualZipHash) -ForegroundColor Green

    # -------------------------------------------------------------
    # Step 2: Extract to completely isolated directory
    # -------------------------------------------------------------
    $ExtractDir = Join-Path $IsolatedRoot "extracted"
    Write-Host ("--> Step 2: Extracting archive into isolated folder: {0}..." -f $ExtractDir) -ForegroundColor Cyan
    Expand-Archive -Path $ZipPath -DestinationPath $ExtractDir -Force

    # -------------------------------------------------------------
    # Step 3: Validate archive directory layout and required files
    # -------------------------------------------------------------
    Write-Host "--> Step 3: Validating archive directory layout and non-empty files..." -ForegroundColor Cyan
    $RequiredFiles = @(
        "bin\vantac.exe",
        "bin\vanta.exe",
        "lib\vanta\std\mod.va",
        "lib\vanta\std\option.va",
        "lib\vanta\std\result.va",
        "LICENSE",
        "README.md",
        "release-manifest.json"
    )

    foreach ($rf in $RequiredFiles) {
        $p = Join-Path $ExtractDir $rf
        if (-not (Test-Path $p)) {
            throw "Layout validation failed: missing expected file: $rf"
        }
        $len = (Get-Item $p).Length
        if ($len -eq 0) {
            throw "Layout validation failed: file is empty (0 bytes): $rf"
        }
    }
    Write-Host ("    All {0} required core files present and non-empty." -f $RequiredFiles.Count) -ForegroundColor Green

    # Strict Zero-Script Assertion: Verify that NO scripts (.ps1, .bat, .cmd, .sh) are packaged
    $foundProhibitedScripts = Get-ChildItem -Path $ExtractDir -Recurse -Include *.ps1, *.bat, *.cmd, *.sh
    if ($foundProhibitedScripts.Count -gt 0) {
        throw ("Prohibited scripts detected in extracted package: {0}" -f ($foundProhibitedScripts -join ', '))
    }
    Write-Host "    Verified zero-scripts policy: no .ps1, .bat, .cmd, or .sh scripts in archive." -ForegroundColor Green

    # -------------------------------------------------------------
    # Step 4: Validate cryptographic hash inventory in release-manifest.json
    # -------------------------------------------------------------
    Write-Host "--> Step 4: Validating per-file SHA-256 hashes against release-manifest.json..." -ForegroundColor Cyan
    $manifestPath = Join-Path $ExtractDir "release-manifest.json"
    $manifest = Get-Content -Raw -Path $manifestPath -Encoding UTF8 | ConvertFrom-Json

    if ($manifest.files.Count -eq 0) {
        throw "release-manifest.json contains no tracked files"
    }

    foreach ($item in $manifest.files) {
        $itemPath = Join-Path $ExtractDir ($item.path.Replace('/', '\'))
        if (-not (Test-Path $itemPath)) {
            throw "Manifest validation failed: tracked file missing: $($item.path)"
        }
        $actualItemSize = (Get-Item $itemPath).Length
        if ($actualItemSize -ne $item.size_bytes) {
            throw "Manifest validation failed: size mismatch for $($item.path): expected $($item.size_bytes), got $actualItemSize"
        }
        $actualItemHash = (Get-FileHash -Path $itemPath -Algorithm SHA256).Hash.ToLower()
        if ($actualItemHash -ne $item.sha256) {
            throw "Manifest validation failed: hash mismatch for $($item.path): expected $($item.sha256), got $actualItemHash"
        }
    }
    Write-Host ("    Verified all {0} files match release-manifest.json cryptographic hashes." -f $manifest.files.Count) -ForegroundColor Green

    # -------------------------------------------------------------
    # Step 5: Validate PE32+ headers for extracted binaries
    # -------------------------------------------------------------
    Write-Host "--> Step 5: Validating PE32+ AMD64 executable headers..." -ForegroundColor Cyan
    $ExtractedVantac = Join-Path $ExtractDir "bin\vantac.exe"
    $ExtractedVanta = Join-Path $ExtractDir "bin\vanta.exe"
    Assert-Pe32PlusExecutable $ExtractedVantac
    Assert-Pe32PlusExecutable $ExtractedVanta

    # -------------------------------------------------------------
    # Step 6: Execute --version on extracted binaries
    # -------------------------------------------------------------
    Write-Host "--> Step 6: Testing extracted executable execution..." -ForegroundColor Cyan
    $p1 = Start-Process -FilePath $ExtractedVantac -ArgumentList "--version" -Wait -PassThru -NoNewWindow
    if ($p1.ExitCode -ne 0) {
        throw ("vantac.exe --version failed with exit code {0}" -f $p1.ExitCode)
    }
    $p2 = Start-Process -FilePath $ExtractedVanta -ArgumentList "--version" -Wait -PassThru -NoNewWindow
    if ($p2.ExitCode -ne 0) {
        throw ("vanta.exe --version failed with exit code {0}" -f $p2.ExitCode)
    }
    Write-Host "    Extracted vantac.exe and vanta.exe executed cleanly." -ForegroundColor Green

    # -------------------------------------------------------------
    # Step 7: Install into isolated prefix using scripts/install.ps1 with extracted package
    # -------------------------------------------------------------
    $InstallPrefix = Join-Path $IsolatedRoot "vanta_install"
    Write-Host ("--> Step 7: Installing via scripts/install.ps1 into {0}..." -f $InstallPrefix) -ForegroundColor Cyan
    $repoInstallScript = Join-Path $RepoRoot "scripts\install.ps1"
    
    $instProc = Start-Process -FilePath "powershell.exe" -ArgumentList "-ExecutionPolicy Bypass -File `"$repoInstallScript`" -Prefix `"$InstallPrefix`" -Version `"v1.0-RC2`" -NoPathUpdate" -WorkingDirectory $ExtractDir -Wait -PassThru -NoNewWindow
    if ($instProc.ExitCode -ne 0) {
        throw ("scripts/install.ps1 failed with exit code {0}" -f $instProc.ExitCode)
    }

    $InstalledVantac = Join-Path $InstallPrefix "bin\vantac.exe"
    $InstalledVanta = Join-Path $InstallPrefix "bin\vanta.exe"
    $InstalledStdDir = Join-Path $InstallPrefix "lib\vanta\std"

    if (-not (Test-Path $InstalledVantac) -or -not (Test-Path $InstalledVanta)) {
        throw "Installation prefix does not contain expected binaries"
    }
    if (-not (Test-Path (Join-Path $InstalledStdDir "mod.va"))) {
        throw "Installation prefix does not contain standard library mod.va"
    }
    Write-Host "    Installation into isolated prefix completed." -ForegroundColor Green

    # -------------------------------------------------------------
    # Step 8: Compile and run a standard library program in isolated workspace
    # -------------------------------------------------------------
    $WorkDir = Join-Path $IsolatedRoot "workspace"
    New-Item -ItemType Directory -Path $WorkDir -Force | Out-Null

    $SourceFile = Join-Path $WorkDir "test_app.va"
    $ExeFile = Join-Path $WorkDir "test_app.exe"

    # Vanta program importing std::option::Option and returning 42
    $VantaProgram = @"
import std::option::Option;

fn main() -> i64 {
    let opt: Option<i64> = Option::Some(42);
    if opt.is_some() {
        return opt.unwrap_or(0);
    }
    return 1;
}
"@
    [System.IO.File]::WriteAllText($SourceFile, $VantaProgram, [System.Text.UTF8Encoding]::new($false))
    Write-Host "--> Step 8: Compiling standard library test program in isolated workspace..." -ForegroundColor Cyan

    # Step 8: Compile using installed vantac.exe with AUTOMATIC standard library discovery
    # VANTA_STD_PATH and VANTA_HOME are explicitly unset; no --std-path argument is passed.
    $compileOut = Join-Path $WorkDir "compile_out.txt"
    $compileErr = Join-Path $WorkDir "compile_err.txt"

    Write-Host "--> Step 8: Compiling standard library test program with automatic stdlib discovery..." -ForegroundColor Cyan
    $compileCmd = "Remove-Item Env:\VANTA_STD_PATH -ErrorAction SilentlyContinue; Remove-Item Env:\VANTA_HOME -ErrorAction SilentlyContinue; & `"$InstalledVantac`" `"$SourceFile`" -o `"$ExeFile`" --target x86_64-pc-windows-msvc"
    $compProc = Start-Process -FilePath "powershell.exe" -ArgumentList "-Command `"$compileCmd`"" -WorkingDirectory $WorkDir -Wait -PassThru -NoNewWindow -RedirectStandardOutput $compileOut -RedirectStandardError $compileErr

    if ($compProc.ExitCode -ne 0) {
        $errText = if (Test-Path $compileErr) { Get-Content -Raw $compileErr } else { "" }
        throw ("Compilation of standard library test program failed with exit code {0}: {1}" -f $compProc.ExitCode, $errText)
    }

    if (-not (Test-Path $ExeFile)) {
        throw "Compiler reported exit code 0 but target executable was not produced: $ExeFile"
    }

    # Verify PE32+ header of compiled user program
    Assert-Pe32PlusExecutable $ExeFile

    # Run compiled executable and assert exit code 42
    Write-Host "--> Step 9: Running compiled program and asserting exit code 42..." -ForegroundColor Cyan
    $runProc = Start-Process -FilePath $ExeFile -Wait -PassThru -NoNewWindow
    $runExit = $runProc.ExitCode

    if ($runExit -ne 42) {
        throw ("Execution of compiled test program returned exit code {0} (expected 42)" -f $runExit)
    }
    Write-Host ("    Compiled standalone program executed successfully: exit code {0}" -f $runExit) -ForegroundColor Green

    # -------------------------------------------------------------
    # Step 10: Safe Uninstallation and Data Preservation in Isolated Prefix
    # -------------------------------------------------------------
    Write-Host "--> Step 10: Testing safe uninstallation with untracked files preserved..." -ForegroundColor Cyan
    # Plant a user file inside the installation prefix
    $untrackedUserFile = Join-Path (Join-Path $InstallPrefix "bin") "untracked_user_tool.txt"
    Set-Content -Path $untrackedUserFile -Value "USER_PRESERVED_CONTENT"

    $uninstallScript = Join-Path $InstallPrefix "uninstall.ps1"
    $uninstProc = Start-Process -FilePath "powershell.exe" -ArgumentList "-ExecutionPolicy Bypass -File `"$uninstallScript`" -Prefix `"$InstallPrefix`"" -Wait -PassThru -NoNewWindow
    if ($uninstProc.ExitCode -ne 0) {
        throw ("Uninstallation failed with exit code {0}" -f $uninstProc.ExitCode)
    }

    # Vanta binaries must be removed
    if (Test-Path $InstalledVantac) { throw "vantac.exe was not removed by uninstaller" }
    if (Test-Path $InstalledVanta) { throw "vanta.exe was not removed by uninstaller" }

    # Untracked user file must survive
    if (-not (Test-Path $untrackedUserFile)) {
        throw "Data-loss bug: untracked user file was deleted during uninstallation!"
    }
    Write-Host "    Uninstallation succeeded: Vanta binaries removed, user files preserved." -ForegroundColor Green

} finally {
    Remove-Item -Recurse -Force $IsolatedRoot -ErrorAction SilentlyContinue
    if ($origUserPath) {
        [Environment]::SetEnvironmentVariable("Path", $origUserPath, "User")
    }
}

Write-Host ""
Write-Host "==========================================================" -ForegroundColor Green
Write-Host "  [OK] Isolated Windows Package Release Gate Passed!        " -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green
