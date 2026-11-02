# Test Suite: Vanta Uninstaller Data-Safety, Containment & Fail-Closed Behavior
$ErrorActionPreference = "Stop"

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  [TEST] Running Uninstaller Data-Safety & Containment     " -ForegroundColor Yellow
Write-Host "==========================================================" -ForegroundColor Cyan

$ScriptDir = if ($PSScriptRoot) { $PSScriptRoot } else { $PWD.Path }
$RepoRoot = if (Test-Path (Join-Path $ScriptDir "scripts\uninstall.ps1")) { $ScriptDir } else { Split-Path -Parent $ScriptDir }
$UninstallScript = Join-Path $RepoRoot "scripts\uninstall.ps1"
if (-not (Test-Path -LiteralPath $UninstallScript)) {
    throw "Cannot find scripts\uninstall.ps1 at '$UninstallScript'"
}

$TestRoot = Join-Path $env:TEMP ("vanta_uninst_safety_" + (Get-Random))
New-Item -ItemType Directory -Path $TestRoot -Force | Out-Null

$origUserPath = [Environment]::GetEnvironmentVariable("Path", "User")

function Remove-JunctionSafe([string]$path) {
    if (Test-Path -LiteralPath $path) {
        try {
            [System.IO.Directory]::Delete($path, $false)
        } catch {
            Remove-Item -LiteralPath $path -Force -Recurse -ErrorAction SilentlyContinue
        }
    }
}

function Run-Uninstaller([string]$prefixPath) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = "powershell.exe"
    $psi.Arguments = "-NonInteractive -ExecutionPolicy Bypass -File `"$UninstallScript`" -Prefix `"$prefixPath`""
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
    # Scenario A: Standard Installation with Untracked Files and PATH Rollback
    # -------------------------------------------------------------
    Write-Host "--> Scenario A: Verifying preservation of untracked files and PATH rollback..." -ForegroundColor Cyan
    $PrefixA = Join-Path $TestRoot "prefix_a"
    $BinA = Join-Path $PrefixA "bin"
    $LibStdA = Join-Path $PrefixA "lib\vanta\std"
    $ExamplesA = Join-Path $PrefixA "examples"

    New-Item -ItemType Directory -Path $BinA, $LibStdA, $ExamplesA -Force | Out-Null

    Set-Content -Path (Join-Path $BinA "vantac.exe") -Value "MZ_MOCK_COMPILER"
    Set-Content -Path (Join-Path $BinA "vanta.exe") -Value "MZ_MOCK_DRIVER"
    Set-Content -Path (Join-Path $LibStdA "mod.va") -Value "pub mod std;"
    Set-Content -Path (Join-Path $PrefixA "LICENSE") -Value "Dual License"

    $UserFileBin = Join-Path $BinA "my_custom_tool.exe"
    Set-Content -Path $UserFileBin -Value "USER_CUSTOM_TOOL"
    $UserFileExamples = Join-Path $ExamplesA "my_game_prototype.va"
    Set-Content -Path $UserFileExamples -Value "USER_GAME_SOURCE"
    $UserFileRoot = Join-Path $PrefixA "custom_project_notes.txt"
    Set-Content -Path $UserFileRoot -Value "USER_NOTES"

    $ManifestA = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        target = "x86_64-pc-windows-msvc"
        installed_at = (Get-Date -Format "o")
        prefix = $PrefixA
        bin_dir = $BinA
        path_added = $true
        binaries = @("bin/vantac.exe", "bin/vanta.exe")
        files = @(
            "bin/vantac.exe",
            "bin/vanta.exe",
            "lib/vanta/std/mod.va",
            "LICENSE"
        )
    }
    $ManifestA | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $PrefixA "install-manifest.json") -Encoding UTF8

    $mockPreExisting = "C:\UnrelatedTool\bin;C:\ExistingSdk\bin"
    [Environment]::SetEnvironmentVariable("Path", "$BinA;$mockPreExisting", "User")

    $resA = Run-Uninstaller $PrefixA
    if ($resA.ExitCode -ne 0) { throw ("Scenario A failed with exit code {0}: {1}" -f $resA.ExitCode, $resA.StdErr) }

    if (Test-Path (Join-Path $BinA "vantac.exe")) { throw "Assertion failed: vantac.exe was NOT removed" }
    if (Test-Path (Join-Path $BinA "vanta.exe")) { throw "Assertion failed: vanta.exe was NOT removed" }
    if (Test-Path (Join-Path $LibStdA "mod.va")) { throw "Assertion failed: mod.va was NOT removed" }
    if (Test-Path (Join-Path $PrefixA "install-manifest.json")) { throw "Assertion failed: install-manifest.json was NOT removed" }

    if (-not (Test-Path $UserFileBin)) { throw "Data-loss bug: $UserFileBin was deleted!" }
    if (-not (Test-Path $UserFileExamples)) { throw "Data-loss bug: $UserFileExamples was deleted!" }
    if (-not (Test-Path $UserFileRoot)) { throw "Data-loss bug: $UserFileRoot was deleted!" }
    if (Test-Path $LibStdA) { throw "Assertion failed: empty directory $LibStdA was NOT cleaned up" }

    $pathAfterA = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($pathAfterA -like "*$BinA*") {
        Write-Host "DIAGNOSTIC FAILURE DETAILS FOR SCENARIO A:" -ForegroundColor Red
        Write-Host "  Manifest bin_dir:     $($ManifestA.bin_dir)"
        Write-Host "  PrefixA:              $PrefixA"
        Write-Host "  BinA:                 $BinA"
        Write-Host "  User:                 $env:USERNAME"
        Write-Host "  User PATH before:     $BinA;$mockPreExisting"
        Write-Host "  User PATH after:      $pathAfterA"
        Write-Host "  Uninstaller StdOut:   $($resA.StdOut)"
        Write-Host "  Uninstaller StdErr:   $($resA.StdErr)"
        throw "Assertion failed: $BinA was NOT removed from User PATH"
    }
    if ($pathAfterA -notlike "*C:\UnrelatedTool\bin*" -or $pathAfterA -notlike "*C:\ExistingSdk\bin*") {
        throw "Data-loss bug: Unrelated User PATH entries were corrupted: $pathAfterA"
    }
    Write-Host "    Scenario A Passed: Untracked files survived, empty dirs removed, PATH safely cleaned." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario B: Pre-existing PATH entries preserved (path_added = false)
    # -------------------------------------------------------------
    Write-Host "--> Scenario B: Verifying pre-existing PATH entries are NOT deleted..." -ForegroundColor Cyan
    $PrefixB = Join-Path $TestRoot "prefix_b"
    $BinB = Join-Path $PrefixB "bin"
    New-Item -ItemType Directory -Path $BinB -Force | Out-Null
    Set-Content -Path (Join-Path $BinB "vantac.exe") -Value "MZ"

    $ManifestB = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        prefix = $PrefixB
        bin_dir = $BinB
        path_added = $false
        files = @("bin/vantac.exe")
    }
    $ManifestB | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $PrefixB "install-manifest.json") -Encoding UTF8

    [Environment]::SetEnvironmentVariable("Path", "$BinB;C:\ExistingTools\bin", "User")

    $resB = Run-Uninstaller $PrefixB
    if ($resB.ExitCode -ne 0) { throw ("Scenario B failed with exit code {0}: {1}" -f $resB.ExitCode, $resB.StdErr) }

    $pathAfterB = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($pathAfterB -notlike "*$BinB*") {
        Write-Host "DIAGNOSTIC FAILURE DETAILS FOR SCENARIO B:" -ForegroundColor Red
        Write-Host "  Manifest bin_dir:     $($ManifestB.bin_dir)"
        Write-Host "  PrefixB:              $PrefixB"
        Write-Host "  BinB:                 $BinB"
        Write-Host "  User PATH after:      $pathAfterB"
        throw "Data-loss bug: Pre-existing PATH entry $BinB was deleted even though path_added was false!"
    }
    Write-Host "    Scenario B Passed: Pre-existing PATH entry was preserved." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 1: Path Containment - Sibling prefix escape
    # -------------------------------------------------------------
    Write-Host "--> Scenario 1: Testing path containment against sibling prefix escape (../prefix_1_sibling)..." -ForegroundColor Cyan
    $Prefix1 = Join-Path $TestRoot "prefix_1"
    $Prefix1Sibling = Join-Path $TestRoot "prefix_1_sibling"
    New-Item -ItemType Directory -Path $Prefix1, $Prefix1Sibling -Force | Out-Null

    $SecretDoc = Join-Path $Prefix1Sibling "confidential.txt"
    Set-Content -Path $SecretDoc -Value "TOP_SECRET_USER_DATA"

    $Manifest1 = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        prefix = $Prefix1
        files = @("../prefix_1_sibling/confidential.txt")
    }
    $Manifest1 | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $Prefix1 "install-manifest.json") -Encoding UTF8

    $res1 = Run-Uninstaller $Prefix1
    if ($res1.ExitCode -eq 0) {
        throw "Security failure: uninstaller did NOT exit with error code when manifest attempted sibling escape"
    }
    if (-not (Test-Path $SecretDoc)) {
        throw "Critical data destruction: outside file $SecretDoc was deleted!"
    }
    Write-Host "    Scenario 1 Passed: Sibling directory escape rejected, outside file preserved." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 2: Path Containment - Absolute path outside prefix
    # -------------------------------------------------------------
    Write-Host "--> Scenario 2: Testing rejection of absolute path outside prefix..." -ForegroundColor Cyan
    $Prefix2 = Join-Path $TestRoot "prefix_2"
    $OutsideDir2 = Join-Path $TestRoot "outside_user_data_2"
    New-Item -ItemType Directory -Path $Prefix2, $OutsideDir2 -Force | Out-Null

    $TargetFile2 = Join-Path $OutsideDir2 "important_work.va"
    Set-Content -Path $TargetFile2 -Value "IMPORTANT_SOURCE_CODE"

    $Manifest2 = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        prefix = $Prefix2
        files = @($TargetFile2)
    }
    $Manifest2 | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $Prefix2 "install-manifest.json") -Encoding UTF8

    $res2 = Run-Uninstaller $Prefix2
    if ($res2.ExitCode -eq 0) {
        throw "Security failure: absolute path outside prefix was NOT rejected!"
    }
    if (-not (Test-Path $TargetFile2)) {
        throw "Critical data destruction: outside file $TargetFile2 was deleted!"
    }
    Write-Host "    Scenario 2 Passed: Absolute path outside prefix rejected." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 3: Path Containment - Rooted drive path
    # -------------------------------------------------------------
    Write-Host "--> Scenario 3: Testing rejection of rooted drive path..." -ForegroundColor Cyan
    $Prefix3 = Join-Path $TestRoot "prefix_3"
    New-Item -ItemType Directory -Path $Prefix3 -Force | Out-Null

    $Manifest3 = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        prefix = $Prefix3
        files = @("C:\Windows\System32\drivers\etc\hosts")
    }
    $Manifest3 | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $Prefix3 "install-manifest.json") -Encoding UTF8

    $res3 = Run-Uninstaller $Prefix3
    if ($res3.ExitCode -eq 0) {
        throw "Security failure: rooted Windows drive path was NOT rejected!"
    }
    Write-Host "    Scenario 3 Passed: Rooted drive path rejected." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 4: Path Containment - UNC Path
    # -------------------------------------------------------------
    Write-Host "--> Scenario 4: Testing rejection of UNC path..." -ForegroundColor Cyan
    $Prefix4 = Join-Path $TestRoot "prefix_4"
    New-Item -ItemType Directory -Path $Prefix4 -Force | Out-Null

    $Manifest4 = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        prefix = $Prefix4
        files = @("\\evil-server\share\payload.dll")
    }
    $Manifest4 | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $Prefix4 "install-manifest.json") -Encoding UTF8

    $res4 = Run-Uninstaller $Prefix4
    if ($res4.ExitCode -eq 0) {
        throw "Security failure: UNC network path was NOT rejected!"
    }
    Write-Host "    Scenario 4 Passed: UNC path rejected." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 5: User PATH Security - Malicious bin_dir
    # -------------------------------------------------------------
    Write-Host "--> Scenario 5: Testing validation of bin_dir before modifying User PATH..." -ForegroundColor Cyan
    $Prefix5 = Join-Path $TestRoot "prefix_5"
    New-Item -ItemType Directory -Path $Prefix5 -Force | Out-Null

    $Manifest5 = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        prefix = $Prefix5
        bin_dir = "C:\Windows\System32"
        path_added = $true
        files = @()
    }
    $Manifest5 | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $Prefix5 "install-manifest.json") -Encoding UTF8

    [Environment]::SetEnvironmentVariable("Path", "C:\Windows\System32;C:\CustomSdk\bin", "User")

    $res5 = Run-Uninstaller $Prefix5
    $pathAfter5 = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($pathAfter5 -notlike "*C:\Windows\System32*") {
        throw "Security failure: uninstaller removed an unauthorized directory from User PATH based on malicious bin_dir!"
    }
    Write-Host "    Scenario 5 Passed: Malicious bin_dir was rejected; unrelated PATH preserved." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 6: Fail-Closed on Invalid JSON Manifest
    # -------------------------------------------------------------
    Write-Host "--> Scenario 6: Testing fail-closed behavior on invalid JSON manifest..." -ForegroundColor Cyan
    $Prefix6 = Join-Path $TestRoot "prefix_6"
    $Bin6 = Join-Path $Prefix6 "bin"
    New-Item -ItemType Directory -Path $Bin6 -Force | Out-Null

    $KeepFile6 = Join-Path $Bin6 "user_program.exe"
    Set-Content -Path $KeepFile6 -Value "BINARY_DATA"
    Set-Content -Path (Join-Path $Prefix6 "install-manifest.json") -Value "{ malformed_json: true, "

    $res6 = Run-Uninstaller $Prefix6
    if ($res6.ExitCode -eq 0) {
        throw "Fail-closed violation: uninstaller succeeded on malformed JSON manifest!"
    }
    if (-not (Test-Path $KeepFile6)) {
        throw "Fail-closed violation: user file was deleted despite malformed manifest!"
    }
    Write-Host "    Scenario 6 Passed: Invalid JSON manifest failed closed; files preserved." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 7: Fail-Closed on Missing Manifest
    # -------------------------------------------------------------
    Write-Host "--> Scenario 7: Testing fail-closed behavior on missing manifest with bin/vanta.exe..." -ForegroundColor Cyan
    $Prefix7 = Join-Path $TestRoot "prefix_7"
    $Bin7 = Join-Path $Prefix7 "bin"
    New-Item -ItemType Directory -Path $Bin7 -Force | Out-Null

    $PreExistingCompiler = Join-Path $Bin7 "vanta.exe"
    Set-Content -Path $PreExistingCompiler -Value "PRE_EXISTING_LEGACY_EXECUTABLE"

    $res7 = Run-Uninstaller $Prefix7
    if ($res7.ExitCode -eq 0) {
        throw "Fail-closed violation: uninstaller reported success on non-empty directory without manifest!"
    }
    if (-not (Test-Path $PreExistingCompiler)) {
        throw "Critical data destruction: pre-existing user executable $PreExistingCompiler was deleted without manifest!"
    }
    Write-Host "    Scenario 7 Passed: Missing manifest failed closed; user bin/vanta.exe strictly preserved." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 8: Fail-Closed and Manifest Preservation on File Deletion Failure
    # -------------------------------------------------------------
    Write-Host "--> Scenario 8: Testing manifest preservation on file deletion failure..." -ForegroundColor Cyan
    $Prefix8 = Join-Path $TestRoot "prefix_8"
    New-Item -ItemType Directory -Path $Prefix8 -Force | Out-Null

    $LockedFile = Join-Path $Prefix8 "locked.dll"
    Set-Content -Path $LockedFile -Value "LOCKED_DLL"

    $Manifest8 = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        prefix = $Prefix8
        files = @("locked.dll")
    }
    $Manifest8 | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $Prefix8 "install-manifest.json") -Encoding UTF8

    $fileStream = [System.IO.File]::Open($LockedFile, [System.IO.FileMode]::Open, [System.IO.FileAccess]::ReadWrite, [System.IO.FileShare]::None)
    try {
        $res8 = Run-Uninstaller $Prefix8
        if ($res8.ExitCode -eq 0) {
            throw "Fail-closed violation: uninstaller reported success when locked file could not be deleted"
        }
        if (-not (Test-Path (Join-Path $Prefix8 "install-manifest.json"))) {
            throw "Fail-closed violation: install-manifest.json was deleted despite file deletion failure!"
        }
    } finally {
        $fileStream.Dispose()
    }
    Write-Host "    Scenario 8 Passed: Locked file failure reported non-zero and preserved manifest." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 9A: Reparse Point / Absolute Junction Escape Rejection
    # -------------------------------------------------------------
    Write-Host "--> Scenario 9A: Testing absolute junction directory escape rejection..." -ForegroundColor Cyan
    $Prefix9 = Join-Path $TestRoot "prefix_9a"
    $OutsideDir9 = Join-Path $TestRoot "outside_target_dir_9a"
    New-Item -ItemType Directory -Path $Prefix9, $OutsideDir9 -Force | Out-Null

    $ProtectedDoc9 = Join-Path $OutsideDir9 "do_not_delete.txt"
    Set-Content -Path $ProtectedDoc9 -Value "PROTECTED_OUTSIDE_DOC"

    $JunctionPath9 = Join-Path $Prefix9 "escape_junction"
    New-Item -ItemType Junction -Path $JunctionPath9 -Target $OutsideDir9 -Force | Out-Null

    if (Test-Path -LiteralPath $JunctionPath9) {
        $Manifest9 = [PSCustomObject]@{
            name = "vanta"
            version = "1.0.0-rc.2"
            prefix = $Prefix9
            files = @("escape_junction/do_not_delete.txt")
        }
        $Manifest9 | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $Prefix9 "install-manifest.json") -Encoding UTF8

        $res9 = Run-Uninstaller $Prefix9
        if ($res9.ExitCode -eq 0) {
            throw "Security failure: absolute junction escape path was not rejected!"
        }
        if (-not (Test-Path $ProtectedDoc9)) {
            throw "Security failure: junction target outside file was deleted!"
        }
        Remove-JunctionSafe $JunctionPath9
        Write-Host "    Scenario 9A Passed: Absolute junction escape rejected; outside file preserved." -ForegroundColor Green
    }

    # -------------------------------------------------------------
    # Scenario 9B: Reparse Point Inside Prefix Resolving Safely Inside Prefix
    # -------------------------------------------------------------
    Write-Host "--> Scenario 9B: Testing safe internal junction resolving within prefix..." -ForegroundColor Cyan
    $Prefix9B = Join-Path $TestRoot "prefix_9b"
    $InternalRealDir = Join-Path $Prefix9B "real_internal_data"
    $InternalJunc = Join-Path $Prefix9B "internal_junc"
    New-Item -ItemType Directory -Path $InternalRealDir -Force | Out-Null

    $InternalFile = Join-Path $InternalRealDir "app.va"
    Set-Content -Path $InternalFile -Value "pub fn test() {}"

    New-Item -ItemType Junction -Path $InternalJunc -Target $InternalRealDir -Force | Out-Null
    if (Test-Path -LiteralPath $InternalJunc) {
        $Manifest9B = [PSCustomObject]@{
            name = "vanta"
            version = "1.0.0-rc.2"
            prefix = $Prefix9B
            files = @("internal_junc/app.va")
        }
        $Manifest9B | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $Prefix9B "install-manifest.json") -Encoding UTF8

        $res9B = Run-Uninstaller $Prefix9B
        if ($res9B.ExitCode -ne 0) {
            throw ("Internal safe junction was unexpectedly rejected: {0}" -f $res9B.StdErr)
        }
        Remove-JunctionSafe $InternalJunc
        Write-Host "    Scenario 9B Passed: Safe internal junction resolved safely and completed." -ForegroundColor Green
    }

    # -------------------------------------------------------------
    # Scenario 9C: Installation Prefix Itself Being a Reparse Point Pointing Outside
    # -------------------------------------------------------------
    Write-Host "--> Scenario 9C: Testing prefix itself being a directory junction (fail closed)..." -ForegroundColor Cyan
    $OutsidePrefixTarget = Join-Path $TestRoot "outside_target_dir_9c"
    New-Item -ItemType Directory -Path $OutsidePrefixTarget -Force | Out-Null
    $Prefix9C = Join-Path $TestRoot "prefix_9c_junc"

    New-Item -ItemType Junction -Path $Prefix9C -Target $OutsidePrefixTarget -Force | Out-Null
    if (Test-Path -LiteralPath $Prefix9C) {
        $res9C = Run-Uninstaller $Prefix9C
        if ($res9C.ExitCode -eq 0) {
            throw "Security failure: installation prefix as a directory junction was not rejected!"
        }
        Remove-JunctionSafe $Prefix9C
        Write-Host "    Scenario 9C Passed: Prefix as junction rejected for safety." -ForegroundColor Green
    }

    # -------------------------------------------------------------
    # Scenario 10: Second Uninstallation Idempotence After Cleanup
    # -------------------------------------------------------------
    Write-Host "--> Scenario 10: Testing idempotence of a second uninstallation run..." -ForegroundColor Cyan
    $Prefix10 = Join-Path $TestRoot "prefix_10"
    New-Item -ItemType Directory -Path $Prefix10 -Force | Out-Null
    Set-Content -Path (Join-Path $Prefix10 "temp.txt") -Value "TEMP"

    $Manifest10 = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        prefix = $Prefix10
        files = @("temp.txt")
    }
    $Manifest10 | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $Prefix10 "install-manifest.json") -Encoding UTF8

    $res10a = Run-Uninstaller $Prefix10
    if ($res10a.ExitCode -ne 0) { throw "First uninstallation failed" }

    # Second uninstallation must return 0
    $res10b = Run-Uninstaller $Prefix10
    if ($res10b.ExitCode -ne 0) {
        throw "Idempotence failed: second uninstallation returned non-zero"
    }
    Write-Host "    Scenario 10 Passed: Second uninstallation completed idempotently with exit code 0." -ForegroundColor Green

} finally {
    Remove-Item -Recurse -Force $TestRoot -ErrorAction SilentlyContinue
    if ($origUserPath) {
        [Environment]::SetEnvironmentVariable("Path", $origUserPath, "User")
    }
}

Write-Host ""
Write-Host "==========================================================" -ForegroundColor Green
Write-Host "  [OK] All Uninstaller Data-Safety Tests Passed!           " -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green
