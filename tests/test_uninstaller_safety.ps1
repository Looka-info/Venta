# Test Suite: Vanta Uninstaller Data-Safety & Preservation
$ErrorActionPreference = "Stop"

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  🧪 Running Uninstaller Data-Safety & Preservation Tests" -ForegroundColor Yellow
Write-Host "==========================================================" -ForegroundColor Cyan

$TestRoot = Join-Path $env:TEMP ("vanta_uninstaller_test_" + (Get-Random))
New-Item -ItemType Directory -Path $TestRoot -Force | Out-Null

try {
    # -------------------------------------------------------------
    # Scenario A: Installation that added PATH, with untracked files
    # -------------------------------------------------------------
    Write-Host "--> Scenario A: Verifying preservation of untracked files and PATH rollback..." -ForegroundColor Cyan

    $PrefixA = Join-Path $TestRoot "prefix_a"
    $BinA = Join-Path $PrefixA "bin"
    $LibStdA = Join-Path $PrefixA "lib\vanta\std"
    $ExamplesA = Join-Path $PrefixA "examples"

    New-Item -ItemType Directory -Path $BinA -Force | Out-Null
    New-Item -ItemType Directory -Path $LibStdA -Force | Out-Null
    New-Item -ItemType Directory -Path $ExamplesA -Force | Out-Null

    # Create Vanta-owned files
    Set-Content -Path (Join-Path $BinA "vantac.exe") -Value "MZ_MOCK_COMPILER"
    Set-Content -Path (Join-Path $BinA "vanta.exe") -Value "MZ_MOCK_DRIVER"
    Set-Content -Path (Join-Path $LibStdA "mod.va") -Value "pub mod std;"
    Set-Content -Path (Join-Path $PrefixA "LICENSE") -Value "Dual License"

    # Copy actual uninstall.ps1 into prefix
    Copy-Item -Path "scripts\uninstall.ps1" -Destination (Join-Path $PrefixA "uninstall.ps1") -Force

    # Create User-created Untracked Files
    $UserFileBin = Join-Path $BinA "my_custom_tool.exe"
    Set-Content -Path $UserFileBin -Value "USER_CUSTOM_TOOL"

    $UserFileExamples = Join-Path $ExamplesA "my_game_prototype.va"
    Set-Content -Path $UserFileExamples -Value "USER_GAME_SOURCE"

    $UserFileRoot = Join-Path $PrefixA "custom_project_notes.txt"
    Set-Content -Path $UserFileRoot -Value "USER_NOTES"

    # Write manifest recording only Vanta-owned files
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
            "LICENSE",
            "uninstall.ps1"
        )
    }
    $ManifestA | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $PrefixA "install-manifest.json") -Encoding UTF8

    # Set up mock User PATH with existing entries + $BinA
    $origUserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $mockPreExisting = "C:\UnrelatedTool\bin;C:\ExistingSdk\bin"
    [Environment]::SetEnvironmentVariable("Path", "$BinA;$mockPreExisting", "User")

    # Run uninstaller
    & (Join-Path $PrefixA "uninstall.ps1") -Prefix $PrefixA

    # Assertions for Scenario A
    # 1. Manifest-owned Vanta files must be removed
    if (Test-Path (Join-Path $BinA "vantac.exe")) { throw "Assertion failed: vantac.exe was NOT removed" }
    if (Test-Path (Join-Path $BinA "vanta.exe")) { throw "Assertion failed: vanta.exe was NOT removed" }
    if (Test-Path (Join-Path $LibStdA "mod.va")) { throw "Assertion failed: mod.va was NOT removed" }
    if (Test-Path (Join-Path $PrefixA "LICENSE")) { throw "Assertion failed: LICENSE was NOT removed" }
    if (Test-Path (Join-Path $PrefixA "install-manifest.json")) { throw "Assertion failed: install-manifest.json was NOT removed" }

    # 2. Untracked user-created files MUST survive
    if (-not (Test-Path $UserFileBin)) { throw "Data-loss bug: $UserFileBin was deleted!" }
    if (-not (Test-Path $UserFileExamples)) { throw "Data-loss bug: $UserFileExamples was deleted!" }
    if (-not (Test-Path $UserFileRoot)) { throw "Data-loss bug: $UserFileRoot was deleted!" }

    # 3. Empty Vanta-created directories (lib/vanta/std) should be cleaned up
    if (Test-Path $LibStdA) { throw "Assertion failed: empty directory $LibStdA was NOT cleaned up" }

    # 4. User PATH verification: only $BinA removed; unrelated entries preserved
    $newUserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($newUserPath -like "*$BinA*") { throw "Assertion failed: $BinA was NOT removed from User PATH" }
    if ($newUserPath -notlike "*C:\UnrelatedTool\bin*" -or $newUserPath -notlike "*C:\ExistingSdk\bin*") {
        throw "Data-loss bug: Unrelated User PATH entries were corrupted: $newUserPath"
    }

    # 5. Idempotence: running uninstaller a second time must not fail
    & "scripts\uninstall.ps1" -Prefix $PrefixA

    Write-Host "    Scenario A Passed: Untracked files survived, empty dirs removed, PATH safely cleaned." -ForegroundColor Green

    # -------------------------------------------------------------
    # Scenario B: PATH entry was pre-existing (path_added = false)
    # -------------------------------------------------------------
    Write-Host "--> Scenario B: Verifying pre-existing PATH entries are NOT deleted..." -ForegroundColor Cyan

    $PrefixB = Join-Path $TestRoot "prefix_b"
    $BinB = Join-Path $PrefixB "bin"
    New-Item -ItemType Directory -Path $BinB -Force | Out-Null

    Set-Content -Path (Join-Path $BinB "vantac.exe") -Value "MZ_MOCK_COMPILER"
    Set-Content -Path (Join-Path $BinB "vanta.exe") -Value "MZ_MOCK_DRIVER"

    $ManifestB = [PSCustomObject]@{
        name = "vanta"
        version = "1.0.0-rc.2"
        target = "x86_64-pc-windows-msvc"
        installed_at = (Get-Date -Format "o")
        prefix = $PrefixB
        bin_dir = $BinB
        path_added = $false # Crucial: was pre-existing!
        binaries = @("bin/vantac.exe", "bin/vanta.exe")
        files = @("bin/vantac.exe", "bin/vanta.exe")
    }
    $ManifestB | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $PrefixB "install-manifest.json") -Encoding UTF8

    [Environment]::SetEnvironmentVariable("Path", "$BinB;C:\ExistingSdk\bin", "User")

    # Run uninstaller
    & "scripts\uninstall.ps1" -Prefix $PrefixB

    $pathAfterB = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($pathAfterB -notlike "*$BinB*") {
        throw "Data-loss bug: Pre-existing PATH entry $BinB was removed even though path_added = false!"
    }
    if ($pathAfterB -notlike "*C:\ExistingSdk\bin*") {
        throw "Data-loss bug: Unrelated PATH entry was corrupted in Scenario B"
    }

    # Since prefix_b has no untracked files, prefix_b itself should be deleted
    if (Test-Path $PrefixB) {
        throw "Assertion failed: Completely empty prefix_b was not removed"
    }

    Write-Host "    Scenario B Passed: Pre-existing PATH entry was preserved, empty prefix cleaned." -ForegroundColor Green

    # Restore original User PATH
    [Environment]::SetEnvironmentVariable("Path", $origUserPath, "User")
} finally {
    Remove-Item -Recurse -Force $TestRoot -ErrorAction SilentlyContinue
    if ($origUserPath) {
        [Environment]::SetEnvironmentVariable("Path", $origUserPath, "User")
    }
}

Write-Host ""
Write-Host "==========================================================" -ForegroundColor Green
Write-Host "  ✅ All Uninstaller Data-Safety Tests Passed!              " -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green
