# ============================================================================
# ⚡ Vanta Runtime Build Script (Windows)
# Compiles runtime/vanta_rt.c into target-specific Windows COFF archive
# ============================================================================

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = (Resolve-Path "$ScriptDir\..").Path
$RuntimeDir = Join-Path $RepoRoot "runtime"
$TargetDir = Join-Path $RuntimeDir "x86_64-pc-windows-msvc"

if (-not (Test-Path $TargetDir)) {
    New-Item -ItemType Directory -Path $TargetDir -Force | Out-Null
}

# Locate C compiler
$ClangCmd = Get-Command clang.exe -ErrorAction SilentlyContinue
$Clang = if ($ClangCmd) { $ClangCmd.Source } else { $null }
if (-not $Clang) {
    # Check winget llvm-mingw location
    $ClangCandidate = "$env:LOCALAPPDATA\Microsoft\WinGet\Packages\MartinStorsjo.LLVM-MinGW.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\llvm-mingw-20261020-ucrt-x86_64\bin\clang.exe"
    if (Test-Path $ClangCandidate) {
        $Clang = $ClangCandidate
    }
}

if (-not $Clang) {
    Write-Error "Could not locate clang.exe. Please install LLVM or Clang."
    exit 1
}

$ArCmd = Get-Command llvm-ar.exe -ErrorAction SilentlyContinue
$Ar = if ($ArCmd) { $ArCmd.Source } else { $null }
if (-not $Ar) {
    $ArCandidate = (Split-Path -Parent $Clang) + "\llvm-ar.exe"
    if (Test-Path $ArCandidate) {
        $Ar = $ArCandidate
    }
}

if (-not $Ar) {
    $Ar = "ar.exe"
}

Write-Host "Compiling Windows runtime with $Clang..."
$SrcFile = Join-Path $RuntimeDir "vanta_rt.c"
$ObjFile = Join-Path $RuntimeDir "vanta_rt.obj"
$LibFile = Join-Path $TargetDir "vanta_rt.lib"
$AFile = Join-Path $TargetDir "libvanta_rt.a"

& $Clang -c -O2 -mno-stack-arg-probe -mno-avx -mno-avx2 $SrcFile -o $ObjFile
if ($LASTEXITCODE -ne 0) {
    Write-Error "Compilation of vanta_rt.c failed with code $LASTEXITCODE"
    exit $LASTEXITCODE
}

Write-Host "Creating archive $LibFile with $Ar..."
& $Ar rcs $LibFile $ObjFile
if ($LASTEXITCODE -ne 0) {
    Write-Error "Archiving vanta_rt.lib failed with code $LASTEXITCODE"
    exit $LASTEXITCODE
}

Copy-Item $LibFile $AFile -Force
Remove-Item $ObjFile -Force

Write-Host "Windows runtime built successfully at $TargetDir"
