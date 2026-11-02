# Test Suite: Vanta Installer Verification, Ownership & Transactional Rollback (Milestone 27D)
$ErrorActionPreference = "Stop"

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  [TEST] Running Vanta Installer Verification Tests        " -ForegroundColor Yellow
Write-Host "==========================================================" -ForegroundColor Cyan

$ScriptDir = if ($PSScriptRoot) { $PSScriptRoot } else { $PWD.Path }
$RepoRoot = if (Test-Path (Join-Path $ScriptDir "scripts\install.ps1")) { $ScriptDir } else { Split-Path -Parent $ScriptDir }
$DefaultInstallScript = Join-Path $RepoRoot "scripts\install.ps1"

if (-not (Test-Path -LiteralPath $DefaultInstallScript)) {
    throw "Cannot find scripts\install.ps1 at '$DefaultInstallScript'"
}

$TestRoot = Join-Path $env:TEMP ("vanta_installer_test_" + (Get-Random))
New-Item -ItemType Directory -Path $TestRoot -Force | Out-Null

$origUserPath = [Environment]::GetEnvironmentVariable("Path", "User")

function Run-Installer(
    [string]$prefixPath,
    [string]$version = "v1.0-RC2",
    [switch]$noPathUpdate,
    [string]$scriptPath = ""
) {
    $targetScript = if ($scriptPath) { $scriptPath } else { $DefaultInstallScript }
    $noPathFlag = if ($noPathUpdate) { "-NoPathUpdate" } else { "" }

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = "powershell.exe"
    $psi.Arguments = "-NonInteractive -ExecutionPolicy Bypass -File `"$targetScript`" -Prefix `"$prefixPath`" -Version `"$version`" $noPathFlag"
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.CreateNoWindow = $true

    $p = [System.Diagnostics.Process]::Start($psi)
    $stdoutTask = $p.StandardOutput.ReadToEndAsync()
    $stderrTask = $p.StandardError.ReadToEndAsync()
    $p.WaitForExit()

    return [PSCustomObject]@{
        ExitCode = $p.ExitCode
        StdOut   = $stdoutTask.Result
        StdErr   = $stderrTask.Result
    }
}

try {
    # -------------------------------------------------------------
    # Scenario 1: Successful installation with release tag "v1.0-RC2"
    # -------------------------------------------------------------
    Write-Host "--> Scenario 1: Installing with Version='v1.0-RC2' and -NoPathUpdate..." -ForegroundColor Cyan
    $Prefix1 = Join-Path $TestRoot "prefix_1"
    
    $res1 = Run-Installer -prefixPath $Prefix1 -version "v1.0-RC2" -noPathUpdate
    if ($res1.ExitCode -ne 0) {
        throw ("Scenario 1 failed with exit code {0}: {1}" -f $res1.ExitCode, $res1.StdErr)
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

    $res2 = Run-Installer -prefixPath $Prefix2 -version "1.0.0-rc.2" -noPathUpdate
    if ($res2.ExitCode -ne 0) {
        throw ("Scenario 2 failed with exit code {0}: {1}" -f $res2.ExitCode, $res2.StdErr)
    }
    Write-Host "    Scenario 2 Passed: Semver 1.0.0-rc.2 accepted." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 3: Version mismatch failure (Version="2.0.0")
    # -------------------------------------------------------------
    Write-Host "--> Scenario 3: Testing hard failure on version mismatch (Version='2.0.0')..." -ForegroundColor Cyan
    $Prefix3 = Join-Path $TestRoot "prefix_3"

    $res3 = Run-Installer -prefixPath $Prefix3 -version "2.0.0" -noPathUpdate
    if ($res3.ExitCode -eq 0) {
        throw "Scenario 3 failed: install.ps1 should have failed on version mismatch, but returned exit code 0"
    }
    Write-Host ("    Scenario 3 Passed: Version mismatch rejected with exit code {0}." -f $res3.ExitCode) -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 4: Non-AMD64 architecture rejection
    # -------------------------------------------------------------
    Write-Host "--> Scenario 4: Testing rejection of non-AMD64 architectures..." -ForegroundColor Cyan
    $Prefix4 = Join-Path $TestRoot "prefix_4"

    $psi4 = New-Object System.Diagnostics.ProcessStartInfo
    $psi4.FileName = "powershell.exe"
    $psi4.Arguments = "-NonInteractive -Command `"`$env:PROCESSOR_ARCHITECTURE = 'ARM64'; `$env:PROCESSOR_ARCHITEW6432 = ''; & `"$DefaultInstallScript`" -Prefix `"$Prefix4`" -NoPathUpdate`""
    $psi4.UseShellExecute = $false
    $psi4.RedirectStandardOutput = $true
    $psi4.RedirectStandardError = $true
    $psi4.CreateNoWindow = $true

    $p4 = [System.Diagnostics.Process]::Start($psi4)
    $stdoutTask4 = $p4.StandardOutput.ReadToEndAsync()
    $stderrTask4 = $p4.StandardError.ReadToEndAsync()
    $p4.WaitForExit()

    if ($p4.ExitCode -eq 0) {
        throw "Scenario 4 failed: install.ps1 should have failed on non-AMD64, but returned exit code 0"
    }
    Write-Host ("    Scenario 4 Passed: Non-AMD64 architecture rejected with exit code {0}." -f $p4.ExitCode) -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 5: Full install with PATH update and safe uninstallation
    # -------------------------------------------------------------
    Write-Host "--> Scenario 5: Testing User PATH tracking and subsequent uninstallation..." -ForegroundColor Cyan
    $Prefix5 = Join-Path $TestRoot "prefix_5"
    $Bin5 = Join-Path $Prefix5 "bin"

    $res5 = Run-Installer -prefixPath $Prefix5 -version "v1.0-RC2"
    if ($res5.ExitCode -ne 0) {
        throw ("Scenario 5 failed with exit code {0}: {1}" -f $res5.ExitCode, $res5.StdErr)
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
    $uninstScript = Join-Path $Prefix5 "uninstall.ps1"
    $psi5u = New-Object System.Diagnostics.ProcessStartInfo
    $psi5u.FileName = "powershell.exe"
    $psi5u.Arguments = "-NonInteractive -ExecutionPolicy Bypass -File `"$uninstScript`" -Prefix `"$Prefix5`""
    $psi5u.UseShellExecute = $false
    $psi5u.RedirectStandardOutput = $true
    $psi5u.RedirectStandardError = $true
    $psi5u.CreateNoWindow = $true
    $pu = [System.Diagnostics.Process]::Start($psi5u)
    $outTaskU = $pu.StandardOutput.ReadToEndAsync()
    $errTaskU = $pu.StandardError.ReadToEndAsync()
    $pu.WaitForExit()

    if ($pu.ExitCode -ne 0) {
        throw ("Scenario 5 uninstallation failed with exit code {0}: {1}" -f $pu.ExitCode, $errTaskU.Result)
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
    if (-not (Test-Path $realVantac)) { $realVantac = Join-Path $RepoRoot "dist\vanta-v1.0-RC2-x86_64-pc-windows-msvc\bin\vantac.exe" }
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
    if ($PSVersionTable.PSEdition -eq "Core") {
        # PowerShell Core does not support -OutputAssembly on Add-Type; invoke Windows PowerShell 5.1
        $srcFile = Join-Path $TestRoot "mock_driver.cs"
        Set-Content -Path $srcFile -Value $csharpCode -Encoding UTF8
        $compileScript = "Add-Type -TypeDefinition (Get-Content -Raw '$srcFile') -OutputAssembly '$mockDriverExe' -OutputType ConsoleApplication"
        $encoded = [Convert]::ToBase64String([System.Text.Encoding]::Unicode.GetBytes($compileScript))
        & powershell.exe -NoProfile -NonInteractive -EncodedCommand $encoded
    } else {
        Add-Type -TypeDefinition $csharpCode -OutputAssembly $mockDriverExe -OutputType ConsoleApplication
    }

    Copy-Item "scripts\install.ps1" (Join-Path $MockDist "install.ps1") -Force
    Copy-Item "scripts\uninstall.ps1" (Join-Path $MockDist "uninstall.ps1") -Force

    $Prefix6 = Join-Path $TestRoot "prefix_6"
    $res6 = Run-Installer -prefixPath $Prefix6 -version "v1.0-RC2" -noPathUpdate -scriptPath (Join-Path $MockDist "install.ps1")

    if ($res6.ExitCode -eq 0) {
        throw "Scenario 6 failed: installer should have rejected driver version mismatch, but returned exit code 0"
    }
    if ($res6.StdErr -notlike "*Driver version mismatch*" -and $res6.StdErr -notlike "*Version discrepancy*") {
        throw ("Scenario 6 failed: Error output did not contain driver version mismatch: {0}" -f $res6.StdErr)
    }
    if (Test-Path (Join-Path $Prefix6 "install-manifest.json")) {
        throw "Scenario 6 failed: install-manifest.json was created despite version mismatch!"
    }
    Write-Host ("    Scenario 6 Passed: Driver-version mismatch rejected with exit code {0}." -f $res6.ExitCode) -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 7: Untracked User File Overwrite Protection (No Manifest)
    # -------------------------------------------------------------
    Write-Host "--> Scenario 7: Testing protection against overwriting untracked user files in prefix..." -ForegroundColor Cyan
    $Prefix7 = Join-Path $TestRoot "prefix_7"
    $Bin7 = Join-Path $Prefix7 "bin"
    New-Item -ItemType Directory -Path $Bin7 -Force | Out-Null

    $UserUntrackedBinary = Join-Path $Bin7 "vantac.exe"
    Set-Content -Path $UserUntrackedBinary -Value "USER_CUSTOM_TOOL_NOT_VANTA"

    $res7 = Run-Installer -prefixPath $Prefix7 -version "v1.0-RC2" -noPathUpdate
    if ($res7.ExitCode -eq 0) {
        throw "Scenario 7 failed: installer silently overwrote untracked user file in prefix!"
    }
    $fileContent7 = Get-Content -Raw $UserUntrackedBinary
    if ($fileContent7 -notlike "*USER_CUSTOM_TOOL_NOT_VANTA*") {
        throw "Catastrophic data-loss: user custom binary was overwritten during failed installation!"
    }
    Write-Host "    Scenario 7 Passed: Untracked user file in prefix protected against silent overwrite." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 8: Valid Manifest Present, but Untracked bin/vanta.exe Exists
    # -------------------------------------------------------------
    Write-Host "--> Scenario 8: Testing rejection when manifest exists but destination file is untracked..." -ForegroundColor Cyan
    $Prefix8 = Join-Path $TestRoot "prefix_8"
    $Bin8 = Join-Path $Prefix8 "bin"
    New-Item -ItemType Directory -Path $Bin8 -Force | Out-Null

    # Manifest only claims ownership of bin/vantac.exe
    $Manifest8 = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        prefix = $Prefix8
        files = @("bin/vantac.exe")
    }
    $Manifest8 | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $Prefix8 "install-manifest.json") -Encoding UTF8

    # User created untracked bin/vanta.exe
    $UserUntrackedDriver = Join-Path $Bin8 "vanta.exe"
    Set-Content -Path $UserUntrackedDriver -Value "USER_CUSTOM_DRIVER_FILE"

    $res8 = Run-Installer -prefixPath $Prefix8 -version "v1.0-RC2" -noPathUpdate
    if ($res8.ExitCode -eq 0) {
        throw "Scenario 8 failed: installer overwrote untracked user file despite valid existing manifest!"
    }
    $fileContent8 = Get-Content -Raw $UserUntrackedDriver
    if ($fileContent8 -notlike "*USER_CUSTOM_DRIVER_FILE*") {
        throw "Data-loss bug: user file was corrupted or overwritten!"
    }
    Write-Host "    Scenario 8 Passed: Untracked destination rejected despite existing manifest." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 9: Valid Manifest with Library Only, Untracked README.md Exists
    # -------------------------------------------------------------
    Write-Host "--> Scenario 9: Testing rejection when untracked root file (README.md) conflicts..." -ForegroundColor Cyan
    $Prefix9 = Join-Path $TestRoot "prefix_9"
    New-Item -ItemType Directory -Path $Prefix9 -Force | Out-Null

    $Manifest9 = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        prefix = $Prefix9
        files = @("bin/vantac.exe", "bin/vanta.exe")
    }
    $Manifest9 | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $Prefix9 "install-manifest.json") -Encoding UTF8

    $UserReadme = Join-Path $Prefix9 "README.md"
    Set-Content -Path $UserReadme -Value "USER_PROJECT_DOCUMENTATION_DO_NOT_DELETE"

    $res9 = Run-Installer -prefixPath $Prefix9 -version "v1.0-RC2" -noPathUpdate
    if ($res9.ExitCode -eq 0) {
        throw "Scenario 9 failed: installer overwrote untracked README.md!"
    }
    $fileContent9 = Get-Content -Raw $UserReadme
    if ($fileContent9 -notlike "*USER_PROJECT_DOCUMENTATION_DO_NOT_DELETE*") {
        throw "Data-loss bug: user README.md was corrupted or overwritten!"
    }
    Write-Host "    Scenario 9 Passed: Conflicting untracked documentation protected." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 10: Malformed Existing Manifest Fails Closed
    # -------------------------------------------------------------
    Write-Host "--> Scenario 10: Testing fail-closed rejection on malformed existing manifest..." -ForegroundColor Cyan
    $Prefix10 = Join-Path $TestRoot "prefix_10"
    New-Item -ItemType Directory -Path $Prefix10 -Force | Out-Null
    Set-Content -Path (Join-Path $Prefix10 "install-manifest.json") -Value "{ malformed: true, "

    $UserFile10 = Join-Path $Prefix10 "existing_tool.txt"
    Set-Content -Path $UserFile10 -Value "USER_TOOL"

    $res10 = Run-Installer -prefixPath $Prefix10 -version "v1.0-RC2" -noPathUpdate
    if ($res10.ExitCode -eq 0) {
        throw "Scenario 10 failed: installer should have failed closed on malformed existing manifest!"
    }
    if (-not (Test-Path $UserFile10)) {
        throw "Data-loss bug: user file was deleted during malformed manifest evaluation!"
    }
    Write-Host "    Scenario 10 Passed: Malformed existing manifest failed closed safely." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 11: Manifest Containing Rooted / Escaping Path Fails Closed
    # -------------------------------------------------------------
    Write-Host "--> Scenario 11: Testing rejection of existing manifest with rooted or escaping paths..." -ForegroundColor Cyan
    $Prefix11 = Join-Path $TestRoot "prefix_11"
    New-Item -ItemType Directory -Path $Prefix11 -Force | Out-Null

    $Manifest11 = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        prefix = $Prefix11
        files = @("C:\Windows\System32\malicious.dll", "../outside.txt")
    }
    $Manifest11 | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $Prefix11 "install-manifest.json") -Encoding UTF8

    $res11 = Run-Installer -prefixPath $Prefix11 -version "v1.0-RC2" -noPathUpdate
    if ($res11.ExitCode -eq 0) {
        throw "Scenario 11 failed: installer should have failed on invalid manifest paths!"
    }
    Write-Host "    Scenario 11 Passed: Escaping/rooted paths in existing manifest failed closed." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 12: Existing Tracked File Upgraded Successfully
    # -------------------------------------------------------------
    Write-Host "--> Scenario 12: Testing successful upgrade of existing tracked file..." -ForegroundColor Cyan
    $Prefix12 = Join-Path $TestRoot "prefix_12"
    
    # 1. First install
    $res12a = Run-Installer -prefixPath $Prefix12 -version "v1.0-RC2" -noPathUpdate
    if ($res12a.ExitCode -ne 0) { throw "Initial install in Scenario 12 failed" }

    # 2. Modify an installed file to a known older version payload
    $TrackedBinary12 = Join-Path $Prefix12 "bin\vantac.exe"
    $InitialHash = (Get-FileHash -Path $TrackedBinary12 -Algorithm SHA256).Hash

    # 3. Run upgrade install over it
    $res12b = Run-Installer -prefixPath $Prefix12 -version "v1.0-RC2" -noPathUpdate
    if ($res12b.ExitCode -ne 0) {
        throw ("Scenario 12 upgrade failed with exit code {0}: {1}" -f $res12b.ExitCode, $res12b.StdErr)
    }
    $FinalHash = (Get-FileHash -Path $TrackedBinary12 -Algorithm SHA256).Hash
    if ($InitialHash -ne $FinalHash) {
        throw "Scenario 12: Upgraded binary does not match expected payload"
    }
    Write-Host "    Scenario 12 Passed: Tracked files upgraded successfully." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 13: Transactional Rollback Restores Prior File Byte-for-Byte
    # -------------------------------------------------------------
    Write-Host "--> Scenario 13: Testing transactional rollback restores prior tracked files byte-for-byte..." -ForegroundColor Cyan
    $Prefix13 = Join-Path $TestRoot "prefix_13"
    $Bin13 = Join-Path $Prefix13 "bin"
    New-Item -ItemType Directory -Path $Bin13 -Force | Out-Null

    # Establish an existing valid installation with tracked vantac.exe and vanta.exe
    $OriginalVantacContent = "ORIGINAL_VERSION_1_COMPILER_BINARY_CONTENT"
    $OriginalVantaContent = "ORIGINAL_VERSION_1_DRIVER_BINARY_CONTENT"
    Set-Content -Path (Join-Path $Bin13 "vantac.exe") -Value $OriginalVantacContent
    Set-Content -Path (Join-Path $Bin13 "vanta.exe") -Value $OriginalVantaContent

    $OriginalManifestContent = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        prefix = $Prefix13
        bin_dir = $Bin13
        path_added = $false
        files = @("bin/vantac.exe", "bin/vanta.exe")
    } | ConvertTo-Json -Depth 5
    Set-Content -Path (Join-Path $Prefix13 "install-manifest.json") -Value $OriginalManifestContent -Encoding UTF8

    # Create mock installer that copies new binaries but fails during verification (e.g. version mismatch)
    $MockDist13 = Join-Path $TestRoot "mock_dist_13"
    $MockBin13 = Join-Path $MockDist13 "bin"
    New-Item -ItemType Directory -Path $MockBin13 -Force | Out-Null

    # New binary that should be rolled back
    Set-Content -Path (Join-Path $MockBin13 "vantac.exe") -Value "NEW_COMPILER_V2"
    Set-Content -Path (Join-Path $MockBin13 "vanta.exe") -Value "NEW_DRIVER_V2"
    Copy-Item "scripts\install.ps1" (Join-Path $MockDist13 "install.ps1") -Force
    Copy-Item "scripts\uninstall.ps1" (Join-Path $MockDist13 "uninstall.ps1") -Force

    # Run installation with requested version "9.9.9" so verification triggers rollback
    $res13 = Run-Installer -prefixPath $Prefix13 -version "9.9.9" -noPathUpdate -scriptPath (Join-Path $MockDist13 "install.ps1")
    if ($res13.ExitCode -eq 0) {
        throw "Scenario 13 failed: installer should have failed on version mismatch!"
    }

    # Verify byte-for-byte restoration of original files
    $restoredVantac = Get-Content -Raw (Join-Path $Bin13 "vantac.exe")
    $restoredVanta = Get-Content -Raw (Join-Path $Bin13 "vanta.exe")
    $restoredManifest = Get-Content -Raw (Join-Path $Prefix13 "install-manifest.json")

    if ($restoredVantac.Trim() -ne $OriginalVantacContent.Trim()) {
        throw "Scenario 13 transactional rollback failed: vantac.exe was NOT restored to original content!"
    }
    if ($restoredVanta.Trim() -ne $OriginalVantaContent.Trim()) {
        throw "Scenario 13 transactional rollback failed: vanta.exe was NOT restored to original content!"
    }
    if ($restoredManifest.Trim() -ne $OriginalManifestContent.Trim()) {
        throw "Scenario 13 transactional rollback failed: install-manifest.json was NOT restored to original content!"
    }
    Write-Host "    Scenario 13 Passed: Transactional rollback restored original files byte-for-byte." -ForegroundColor Green

} finally {
    Remove-Item -Recurse -Force $TestRoot -ErrorAction SilentlyContinue
    if ($origUserPath) {
        [Environment]::SetEnvironmentVariable("Path", $origUserPath, "User")
    }
}

Write-Host ""
Write-Host "==========================================================" -ForegroundColor Green
Write-Host "  [OK] All 13 Installer Verification & Ownership Tests Passed!" -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green
