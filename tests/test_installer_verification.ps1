# Test Suite: Vanta Installer Verification & Version Matching (Milestone 27C)
$ErrorActionPreference = "Stop"

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  [TEST] Running Vanta Installer Verification Tests        " -ForegroundColor Yellow
Write-Host "==========================================================" -ForegroundColor Cyan

$ScriptDir = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Definition }
$RepoRoot = if ($ScriptDir) { Split-Path -Parent $ScriptDir } else { $PWD }

$TestRoot = Join-Path $env:TEMP ("vanta_installer_test_" + (Get-Random))
New-Item -ItemType Directory -Path $TestRoot -Force | Out-Null

$origUserPath = [Environment]::GetEnvironmentVariable("Path", "User")

try {
    # -------------------------------------------------------------
    # Scenario 1: Successful installation with release tag "v1.0-RC2"
    # -------------------------------------------------------------
    Write-Host "--> Scenario 1: Installing with Version='v1.0-RC2' and -NoPathUpdate..." -ForegroundColor Cyan
    $Prefix1 = Join-Path $TestRoot "prefix_1"
    
    $proc1 = Start-Process -FilePath "powershell.exe" -ArgumentList "-ExecutionPolicy Bypass -File scripts\install.ps1 -Prefix `"$Prefix1`" -Version `"v1.0-RC2`" -NoPathUpdate" -Wait -PassThru -NoNewWindow
    if ($proc1.ExitCode -ne 0) {
        throw ("Scenario 1 failed: install.ps1 exited with code {0}" -f $proc1.ExitCode)
    }

    $manifestPath1 = Join-Path $Prefix1 "install-manifest.json"
    if (-not (Test-Path $manifestPath1)) {
        throw "Scenario 1 failed: install-manifest.json not created"
    }
    $m1 = Get-Content -Raw -Path $manifestPath1 | ConvertFrom-Json
    if ($m1.version -ne "1.0.0-rc.2") {
        throw ("Scenario 1 failed: Expected manifest version '1.0.0-rc.2', got '{0}'" -f $m1.version)
    }
    if ($m1.path_added -ne $false) {
        throw "Scenario 1 failed: path_added should be false when -NoPathUpdate is passed"
    }
    Write-Host "    Scenario 1 Passed: Version v1.0-RC2 installed and normalized successfully." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 2: Successful installation with semver "1.0.0-rc.2"
    # -------------------------------------------------------------
    Write-Host "--> Scenario 2: Installing with Version='1.0.0-rc.2' and -NoPathUpdate..." -ForegroundColor Cyan
    $Prefix2 = Join-Path $TestRoot "prefix_2"

    $proc2 = Start-Process -FilePath "powershell.exe" -ArgumentList "-ExecutionPolicy Bypass -File scripts\install.ps1 -Prefix `"$Prefix2`" -Version `"1.0.0-rc.2`" -NoPathUpdate" -Wait -PassThru -NoNewWindow
    if ($proc2.ExitCode -ne 0) {
        throw ("Scenario 2 failed: install.ps1 exited with code {0}" -f $proc2.ExitCode)
    }
    Write-Host "    Scenario 2 Passed: Semver 1.0.0-rc.2 accepted." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 3: Version mismatch failure (Version="2.0.0")
    # -------------------------------------------------------------
    Write-Host "--> Scenario 3: Testing hard failure on version mismatch (Version='2.0.0')..." -ForegroundColor Cyan
    $Prefix3 = Join-Path $TestRoot "prefix_3"

    $errFile3 = Join-Path $TestRoot "err3.txt"
    $proc3 = Start-Process -FilePath "powershell.exe" -ArgumentList "-ExecutionPolicy Bypass -File scripts\install.ps1 -Prefix `"$Prefix3`" -Version `"2.0.0`" -NoPathUpdate" -Wait -PassThru -NoNewWindow -RedirectStandardError $errFile3
    
    if ($proc3.ExitCode -eq 0) {
        throw "Scenario 3 failed: install.ps1 should have failed on version mismatch, but returned exit code 0"
    }
    Write-Host ("    Scenario 3 Passed: Version mismatch rejected with exit code {0}." -f $proc3.ExitCode) -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 4: Non-AMD64 architecture rejection
    # -------------------------------------------------------------
    Write-Host "--> Scenario 4: Testing rejection of non-AMD64 architectures..." -ForegroundColor Cyan
    $Prefix4 = Join-Path $TestRoot "prefix_4"
    $errFile4 = Join-Path $TestRoot "err4.txt"

    $proc4 = Start-Process -FilePath "powershell.exe" -ArgumentList "-Command `"`$env:PROCESSOR_ARCHITECTURE = 'ARM64'; `$env:PROCESSOR_ARCHITEW6432 = ''; & scripts\install.ps1 -Prefix `"$Prefix4`" -NoPathUpdate`"" -Wait -PassThru -NoNewWindow -RedirectStandardError $errFile4

    if ($proc4.ExitCode -eq 0) {
        throw "Scenario 4 failed: install.ps1 should have failed on non-AMD64, but returned exit code 0"
    }
    Write-Host ("    Scenario 4 Passed: Non-AMD64 architecture rejected with exit code {0}." -f $proc4.ExitCode) -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 5: Full install with PATH update and safe uninstallation
    # -------------------------------------------------------------
    Write-Host "--> Scenario 5: Testing User PATH tracking and subsequent uninstallation..." -ForegroundColor Cyan
    $Prefix5 = Join-Path $TestRoot "prefix_5"
    $Bin5 = Join-Path $Prefix5 "bin"

    $proc5 = Start-Process -FilePath "powershell.exe" -ArgumentList "-ExecutionPolicy Bypass -File scripts\install.ps1 -Prefix `"$Prefix5`" -Version `"v1.0-RC2`"" -Wait -PassThru -NoNewWindow
    if ($proc5.ExitCode -ne 0) {
        throw ("Scenario 5 failed: install.ps1 exited with code {0}" -f $proc5.ExitCode)
    }

    $m5 = Get-Content -Raw -Path (Join-Path $Prefix5 "install-manifest.json") | ConvertFrom-Json
    if ($m5.path_added -ne $true) {
        throw "Scenario 5 failed: path_added should be true when installed without -NoPathUpdate"
    }

    $curPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($curPath -notlike "*$Bin5*") {
        throw "Scenario 5 failed: User PATH was not updated with $Bin5"
    }

    # Now run uninstaller from prefix
    $uninstProc = Start-Process -FilePath "powershell.exe" -ArgumentList "-ExecutionPolicy Bypass -File `"$Prefix5\uninstall.ps1`" -Prefix `"$Prefix5`"" -Wait -PassThru -NoNewWindow
    if ($uninstProc.ExitCode -ne 0) {
        throw ("Scenario 5 uninstallation failed with exit code {0}" -f $uninstProc.ExitCode)
    }

    $curPathAfter = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($curPathAfter -like "*$Bin5*") {
        throw "Scenario 5 failed: User PATH still contains $Bin5 after uninstallation"
    }
    Write-Host "    Scenario 5 Passed: PATH successfully added, tracked, and safely removed by uninstaller." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 6: Driver-Version Mismatch (Valid Compiler, Wrong Driver Version)
    # -------------------------------------------------------------
    Write-Host "--> Scenario 6: Testing driver-version mismatch rejection (valid compiler, wrong driver)..." -ForegroundColor Cyan
    $MockDist = Join-Path $TestRoot "mock_driver_mismatch_dist"
    $MockBin = Join-Path $MockDist "bin"
    New-Item -ItemType Directory -Path $MockBin -Force | Out-Null

    # Copy real vantac.exe (which reports 1.0.0-rc.2)
    $realVantac = Join-Path $RepoRoot "target\release\vantac.exe"
    if (-not (Test-Path $realVantac)) { $realVantac = "target\release\vantac.exe" }
    Copy-Item $realVantac (Join-Path $MockBin "vantac.exe") -Force

    # Compile a mock vanta.exe that reports "vanta 2.0.0"
    $mockDriverExe = Join-Path $MockBin "vanta.exe"
    $csharpCode = @"
using System;
class MockDriver {
    public static void Main(string[] args) {
        Console.WriteLine("vanta 2.0.0");
    }
}
"@
    Add-Type -TypeDefinition $csharpCode -OutputAssembly $mockDriverExe -OutputType ConsoleApplication

    # Copy install.ps1 and uninstall.ps1 into mock dist
    Copy-Item "scripts\install.ps1" (Join-Path $MockDist "install.ps1") -Force
    Copy-Item "scripts\uninstall.ps1" (Join-Path $MockDist "uninstall.ps1") -Force

    $Prefix6 = Join-Path $TestRoot "prefix_6"
    $errFile6 = Join-Path $TestRoot "err6.txt"
    $proc6 = Start-Process -FilePath "powershell.exe" -ArgumentList "-ExecutionPolicy Bypass -File `"$MockDist\install.ps1`" -Prefix `"$Prefix6`" -Version `"v1.0-RC2`" -NoPathUpdate" -Wait -PassThru -NoNewWindow -RedirectStandardError $errFile6

    if ($proc6.ExitCode -eq 0) {
        throw "Scenario 6 failed: installer should have rejected driver version mismatch, but returned exit code 0"
    }
    $errOutput6 = if (Test-Path $errFile6) { Get-Content -Raw $errFile6 } else { "" }
    if ($errOutput6 -notlike "*Driver version mismatch*" -and $errOutput6 -notlike "*Version discrepancy*") {
        throw ("Scenario 6 failed: Error output did not contain driver version mismatch: {0}" -f $errOutput6)
    }
    if (Test-Path (Join-Path $Prefix6 "install-manifest.json")) {
        throw "Scenario 6 failed: install-manifest.json was created despite version mismatch!"
    }
    Write-Host ("    Scenario 6 Passed: Driver-version mismatch rejected with exit code {0}." -f $proc6.ExitCode) -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 7: Untracked User File Overwrite Protection
    # -------------------------------------------------------------
    Write-Host "--> Scenario 7: Testing protection against overwriting untracked user files in prefix..." -ForegroundColor Cyan
    $Prefix7 = Join-Path $TestRoot "prefix_7"
    $Bin7 = Join-Path $Prefix7 "bin"
    New-Item -ItemType Directory -Path $Bin7 -Force | Out-Null

    # Place an untracked user file at bin/vantac.exe without an install-manifest.json
    $UserUntrackedBinary = Join-Path $Bin7 "vantac.exe"
    Set-Content -Path $UserUntrackedBinary -Value "USER_CUSTOM_TOOL_NOT_VANTA"

    $errFile7 = Join-Path $TestRoot "err7.txt"
    $proc7 = Start-Process -FilePath "powershell.exe" -ArgumentList "-ExecutionPolicy Bypass -File scripts\install.ps1 -Prefix `"$Prefix7`" -Version `"v1.0-RC2`" -NoPathUpdate" -Wait -PassThru -NoNewWindow -RedirectStandardError $errFile7

    if ($proc7.ExitCode -eq 0) {
        throw "Scenario 7 failed: installer silently overwrote untracked user file in prefix!"
    }
    $fileContent7 = Get-Content -Raw $UserUntrackedBinary
    if ($fileContent7 -notlike "*USER_CUSTOM_TOOL_NOT_VANTA*") {
        throw "Catastrophic data-loss: user custom binary was overwritten during failed installation!"
    }
    Write-Host "    Scenario 7 Passed: Untracked user file in prefix protected against silent overwrite." -ForegroundColor Green

} finally {
    Remove-Item -Recurse -Force $TestRoot -ErrorAction SilentlyContinue
    if ($origUserPath) {
        [Environment]::SetEnvironmentVariable("Path", $origUserPath, "User")
    }
}

Write-Host ""
Write-Host "==========================================================" -ForegroundColor Green
Write-Host "  [OK] All 7 Installer Verification Tests Passed!           " -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green
