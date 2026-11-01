# Unregister .va and .vanta file icon association in Windows Explorer (User Scope)

$ErrorActionPreference = "SilentlyContinue"

Write-Host "Unregistering Venta file icon association..." -ForegroundColor Cyan

Remove-Item -Path "HKCU:\Software\Classes\.va" -Recurse -Force
Remove-Item -Path "HKCU:\Software\Classes\.vanta" -Recurse -Force
Remove-Item -Path "HKCU:\Software\Classes\Venta.SourceFile" -Recurse -Force

try {
    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class ShellNotifierUnregister {
    [DllImport("shell32.dll", CharSet = CharSet.Auto, SetLastError = true)]
    public static extern void SHChangeNotify(uint wEventId, uint uFlags, IntPtr dwItem1, IntPtr dwItem2);
}
"@
    [ShellNotifierUnregister]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
} catch {}

Write-Host "Venta file icon association removed." -ForegroundColor Green
