# Test Suite: Vanta Uninstaller Data-Safety, Containment & Fail-Closed Behavior
$ErrorActionPreference = "Stop"

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  [TEST] Running Uninstaller Data-Safety & Containment     " -ForegroundColor Yellow
Write-Host "==========================================================" -ForegroundColor Cyan

$TestRoot = Join-Path $env:TEMP ("vanta_uninst_safety_" + (Get-Random))
New-Item -ItemType Directory -Path $TestRoot -Force | Out-Null

$origUserPath = [Environment]::GetEnvironmentVariable("Path", "User")

function Run-Uninstaller([string]$prefixPath) {
    $outFile = Join-Path $TestRoot "uninst_out.txt"
    $errFile = Join-Path $TestRoot "uninst_err.txt"
    Remove-Item $outFile, $errFile -Force -ErrorAction SilentlyContinue

    $proc = Start-Process -FilePath "powershell.exe" -ArgumentList "-ExecutionPolicy Bypass -File scripts\uninstall.ps1 -Prefix `"$prefixPath`"" -Wait -PassThru -NoNewWindow -RedirectStandardOutput $outFile -RedirectStandardError $errFile
    $outText = if (Test-Path $outFile) { Get-Content -Raw $outFile } else { "" }
    $errText = if (Test-Path $errFile) { Get-Content -Raw $errFile } else { "" }
    Remove-Item $outFile, $errFile -Force -ErrorAction SilentlyContinue

    return [PSCustomObject]@{
        ExitCode = $proc.ExitCode
        StdOut   = $outText
        StdErr   = $errText
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
    if ($pathAfterA -like "*$BinA*") { throw "Assertion failed: $BinA was NOT removed from User PATH" }
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
    Set-Content -Path (Join-Path $BinB "vantac.exe") -Value "MZ_MOCK_COMPILER"

    $ManifestB = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        prefix = $PrefixB
        bin_dir = $BinB
        path_added = $false
        files = @("bin/vantac.exe")
    }
    $ManifestB | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $PrefixB "install-manifest.json") -Encoding UTF8
    [Environment]::SetEnvironmentVariable("Path", "$BinB;C:\ExistingSdk\bin", "User")

    $resB = Run-Uninstaller $PrefixB
    if ($resB.ExitCode -ne 0) { throw ("Scenario B failed with exit code {0}: {1}" -f $resB.ExitCode, $resB.StdErr) }

    $pathAfterB = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($pathAfterB -notlike "*$BinB*") {
        throw "Data-loss bug: Pre-existing PATH entry $BinB was removed even though path_added = false!"
    }
    Write-Host "    Scenario B Passed: Pre-existing PATH entry was preserved." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 1: Path Containment — Sibling Prefix Directory Escape Check
    # -------------------------------------------------------------
    Write-Host "--> Scenario 1: Testing path containment against sibling prefix escape (../prefix_1_sibling)..." -ForegroundColor Cyan
    $Prefix1 = Join-Path $TestRoot "prefix_1"
    $Sibling1 = Join-Path $TestRoot "prefix_1_sibling"
    New-Item -ItemType Directory -Path $Prefix1, $Sibling1 -Force | Out-Null

    $SecretFile1 = Join-Path $Sibling1 "important_user_doc.txt"
    Set-Content -Path $SecretFile1 -Value "CRITICAL_USER_DATA"

    $Manifest1 = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        prefix = $Prefix1
        bin_dir = (Join-Path $Prefix1 "bin")
        path_added = $false
        files = @("../prefix_1_sibling/important_user_doc.txt")
    }
    $Manifest1 | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $Prefix1 "install-manifest.json") -Encoding UTF8

    $res1 = Run-Uninstaller $Prefix1
    if ($res1.ExitCode -eq 0) {
        throw "Security failure: uninstaller should have failed on sibling directory escape path!"
    }
    if (-not (Test-Path $SecretFile1)) {
        throw "Catastrophic security failure: Sibling file was deleted: $SecretFile1"
    }
    Write-Host "    Scenario 1 Passed: Sibling directory escape rejected, outside file preserved." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 2: Absolute Path Outside Prefix
    # -------------------------------------------------------------
    Write-Host "--> Scenario 2: Testing rejection of absolute path outside prefix..." -ForegroundColor Cyan
    $Prefix2 = Join-Path $TestRoot "prefix_2"
    New-Item -ItemType Directory -Path $Prefix2 -Force | Out-Null

    $OutsideFile2 = Join-Path $TestRoot "outside_file_2.txt"
    Set-Content -Path $OutsideFile2 -Value "OUTSIDE_DATA"

    $Manifest2 = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        prefix = $Prefix2
        files = @($OutsideFile2)
    }
    $Manifest2 | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $Prefix2 "install-manifest.json") -Encoding UTF8

    $res2 = Run-Uninstaller $Prefix2
    if ($res2.ExitCode -eq 0) { throw "Security failure: absolute path was not rejected" }
    if (-not (Test-Path $OutsideFile2)) { throw "Security failure: outside file was deleted!" }
    Write-Host "    Scenario 2 Passed: Absolute path outside prefix rejected." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 3: Rooted Drive Path
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
    if ($res3.ExitCode -eq 0) { throw "Security failure: rooted drive path was not rejected" }
    Write-Host "    Scenario 3 Passed: Rooted drive path rejected." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 4: UNC Path
    # -------------------------------------------------------------
    Write-Host "--> Scenario 4: Testing rejection of UNC path..." -ForegroundColor Cyan
    $Prefix4 = Join-Path $TestRoot "prefix_4"
    New-Item -ItemType Directory -Path $Prefix4 -Force | Out-Null

    $Manifest4 = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        prefix = $Prefix4
        files = @("\\server\share\file.txt")
    }
    $Manifest4 | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $Prefix4 "install-manifest.json") -Encoding UTF8

    $res4 = Run-Uninstaller $Prefix4
    if ($res4.ExitCode -eq 0) { throw "Security failure: UNC path was not rejected" }
    Write-Host "    Scenario 4 Passed: UNC path rejected." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 5: Malicious or Incorrect bin_dir in Manifest
    # -------------------------------------------------------------
    Write-Host "--> Scenario 5: Testing validation of bin_dir before modifying User PATH..." -ForegroundColor Cyan
    $Prefix5 = Join-Path $TestRoot "prefix_5"
    New-Item -ItemType Directory -Path $Prefix5 -Force | Out-Null

    $MaliciousBin = "C:\UnrelatedTool\bin"
    [Environment]::SetEnvironmentVariable("Path", "$MaliciousBin;C:\ExistingSdk\bin", "User")

    $Manifest5 = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        prefix = $Prefix5
        bin_dir = $MaliciousBin
        path_added = $true
        files = @()
    }
    $Manifest5 | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $Prefix5 "install-manifest.json") -Encoding UTF8

    $res5 = Run-Uninstaller $Prefix5
    $pathAfter5 = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($pathAfter5 -notlike "*$MaliciousBin*") {
        throw "Security failure: Unrelated PATH entry was removed because of malicious bin_dir in manifest!"
    }
    Write-Host "    Scenario 5 Passed: Malicious bin_dir was rejected; unrelated PATH preserved." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 6: Malformed / Invalid JSON Manifest
    # -------------------------------------------------------------
    Write-Host "--> Scenario 6: Testing fail-closed behavior on invalid JSON manifest..." -ForegroundColor Cyan
    $Prefix6 = Join-Path $TestRoot "prefix_6"
    New-Item -ItemType Directory -Path $Prefix6 -Force | Out-Null

    $UserFile6 = Join-Path $Prefix6 "user_program.exe"
    Set-Content -Path $UserFile6 -Value "USER_BINARY"
    Set-Content -Path (Join-Path $Prefix6 "install-manifest.json") -Value "{ broken_json : true, "

    $res6 = Run-Uninstaller $Prefix6
    if ($res6.ExitCode -eq 0) { throw "Fail-closed violation: uninstaller reported success on invalid JSON manifest" }
    if (-not (Test-Path $UserFile6)) { throw "Data-loss bug: user program was deleted when manifest was invalid JSON" }
    if (-not (Test-Path (Join-Path $Prefix6 "install-manifest.json"))) { throw "Fail-closed violation: manifest was deleted" }
    Write-Host "    Scenario 6 Passed: Invalid JSON manifest failed closed; files preserved." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 7: Missing Manifest with User-Created bin/vanta.exe
    # -------------------------------------------------------------
    Write-Host "--> Scenario 7: Testing fail-closed behavior on missing manifest with bin/vanta.exe..." -ForegroundColor Cyan
    $Prefix7 = Join-Path $TestRoot "prefix_7"
    $Bin7 = Join-Path $Prefix7 "bin"
    New-Item -ItemType Directory -Path $Bin7 -Force | Out-Null

    $UserCustomDriver = Join-Path $Bin7 "vanta.exe"
    Set-Content -Path $UserCustomDriver -Value "USER_CUSTOM_NOT_OWNED_BY_VANTA"

    $res7 = Run-Uninstaller $Prefix7
    if ($res7.ExitCode -eq 0) {
        throw "Fail-closed violation: uninstaller reported success when manifest was missing"
    }
    if (-not (Test-Path $UserCustomDriver)) {
        throw "Catastrophic data-loss: uninstaller deleted user-created bin/vanta.exe when no manifest existed!"
    }
    Write-Host "    Scenario 7 Passed: Missing manifest failed closed; user bin/vanta.exe strictly preserved." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario 8: File Deletion Failure (Preserve Manifest)
    # -------------------------------------------------------------
    Write-Host "--> Scenario 8: Testing manifest preservation on file deletion failure..." -ForegroundColor Cyan
    $Prefix8 = Join-Path $TestRoot "prefix_8"
    New-Item -ItemType Directory -Path $Prefix8 -Force | Out-Null

    $LockedFile = Join-Path $Prefix8 "locked_file.txt"
    Set-Content -Path $LockedFile -Value "LOCK_TEST"

    $Manifest8 = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        prefix = $Prefix8
        files = @("locked_file.txt")
    }
    $Manifest8 | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $Prefix8 "install-manifest.json") -Encoding UTF8

    # Lock file exclusively
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
    # Scenario 9: Reparse Point / Junction Escape Rejection
    # -------------------------------------------------------------
    Write-Host "--> Scenario 9: Testing junction-based directory escape rejection..." -ForegroundColor Cyan
    $Prefix9 = Join-Path $TestRoot "prefix_9"
    $OutsideDir9 = Join-Path $TestRoot "outside_target_dir"
    New-Item -ItemType Directory -Path $Prefix9, $OutsideDir9 -Force | Out-Null

    $ProtectedDoc = Join-Path $OutsideDir9 "do_not_delete.txt"
    Set-Content -Path $ProtectedDoc -Value "PROTECTED_OUTSIDE_DOC"

    $JunctionPath = Join-Path $Prefix9 "escape_junction"
    # Create junction if supported in this environment
    $createdJunction = $false
    try {
        New-Item -ItemType Junction -Path $JunctionPath -Target $OutsideDir9 -Force | Out-Null
        $createdJunction = $true
    } catch {
        Write-Warning "Junction creation not permitted in this environment; skipping active junction check."
    }

    if ($createdJunction) {
        $Manifest9 = [PSCustomObject]@{
            name = "vanta"
            version = "1.0.0-rc.2"
            prefix = $Prefix9
            files = @("escape_junction/do_not_delete.txt")
        }
        $Manifest9 | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $Prefix9 "install-manifest.json") -Encoding UTF8

        $res9 = Run-Uninstaller $Prefix9
        if ($res9.ExitCode -eq 0) {
            throw "Security failure: junction escape path was not rejected!"
        }
        if (-not (Test-Path $ProtectedDoc)) {
            throw "Security failure: junction target outside file was deleted!"
        }
        Write-Host "    Scenario 9 Passed: Junction escape rejected; outside file preserved." -ForegroundColor Green
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
Write-Host "  [OK] All 12 Uninstaller Data-Safety Tests Passed!         " -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green
