# Register .va and .vanta file icon association in Windows Explorer (User Scope)
# This does NOT require Administrator privileges.

$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path "$PSScriptRoot\..").Path
$icoPath = Join-Path $repoRoot "editors\vscode\icons\venta-file-icon.ico"

if (-not (Test-Path $icoPath)) {
    Write-Error "Icon file not found at: $icoPath"
    exit 1
}

Write-Host "Registering Venta file icon association..." -ForegroundColor Cyan
Write-Host "Icon path: $icoPath" -ForegroundColor Gray

# 1. Register ProgID for Venta Source File
$progIdKey = "HKCU:\Software\Classes\Venta.SourceFile"
New-Item -Path $progIdKey -Force | Out-Null
Set-ItemProperty -Path $progIdKey -Name "(Default)" -Value "Venta Source File"

# 2. Set DefaultIcon for ProgID
$iconKey = "$progIdKey\DefaultIcon"
New-Item -Path $iconKey -Force | Out-Null
Set-ItemProperty -Path $iconKey -Name "(Default)" -Value "$icoPath,0"

# 3. Associate .va extension
$extVaKey = "HKCU:\Software\Classes\.va"
New-Item -Path $extVaKey -Force | Out-Null
Set-ItemProperty -Path $extVaKey -Name "(Default)" -Value "Venta.SourceFile"
Set-ItemProperty -Path $extVaKey -Name "Content Type" -Value "text/x-venta"
Set-ItemProperty -Path $extVaKey -Name "PerceivedType" -Value "text"

# 4. Associate .vanta extension
$extVantaKey = "HKCU:\Software\Classes\.vanta"
New-Item -Path $extVantaKey -Force | Out-Null
Set-ItemProperty -Path $extVantaKey -Name "(Default)" -Value "Venta.SourceFile"
Set-ItemProperty -Path $extVantaKey -Name "Content Type" -Value "text/x-venta"
Set-ItemProperty -Path $extVantaKey -Name "PerceivedType" -Value "text"

# 5. Notify Windows Shell to refresh icon cache
try {
    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class ShellNotifier {
    [DllImport("shell32.dll", CharSet = CharSet.Auto, SetLastError = true)]
    public static extern void SHChangeNotify(uint wEventId, uint uFlags, IntPtr dwItem1, IntPtr dwItem2);
}
"@
    [ShellNotifier]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero) # SHCNE_ASSOCCHANGED
    Write-Host "Windows Shell notified of file association change." -ForegroundColor Green
} catch {
    Write-Host "Note: Shell notification skipped; icon will appear on explorer restart." -ForegroundColor Yellow
}

Write-Host "`nSuccessfully registered .va and .vanta file icons for Windows Explorer!" -ForegroundColor Green
