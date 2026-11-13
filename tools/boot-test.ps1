# Boots headless and checks that the kernel reaches its "ready" line on the
# serial port. Exit code 0 = boot OK. Prints the serial log either way.

param([int]$TimeoutSeconds = 20, [switch]$Release)

. "$PSScriptRoot\common.ps1"

$ReadyMarker = 'VibeCodedOs kernel ready.'
$log = Join-Path $BuildDir 'serial.log'

Invoke-Build -Release:$Release
Remove-Item $log -ErrorAction SilentlyContinue

# Start-Process joins arguments with spaces, so quote those containing one.
$qemuArgs = @(Get-QemuArgs) + @('-display', 'none', '-serial', "file:$log") |
    ForEach-Object { if ($_ -match ' ') { "`"$_`"" } else { $_ } }
$qemu = Start-Process $QemuExe -ArgumentList $qemuArgs -PassThru -NoNewWindow

$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
$booted = $false
while ((Get-Date) -lt $deadline -and -not $qemu.HasExited) {
    if ((Test-Path $log) -and (Select-String -Path $log -SimpleMatch $ReadyMarker -Quiet)) {
        $booted = $true
        break
    }
    Start-Sleep -Milliseconds 250
}
if (-not $qemu.HasExited) { Stop-Process $qemu -Force }

if (Test-Path $log) { Get-Content $log }
if ($booted) {
    Write-Host "`nBOOT OK" -ForegroundColor Green
    exit 0
}
Write-Host "`nBOOT FAILED (no '$ReadyMarker' within $TimeoutSeconds s)" -ForegroundColor Red
exit 1
