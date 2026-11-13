# Boots headless and checks the serial log for an expected line.
# Exit code 0 = found. Prints the serial log either way.
#
#   boot-test.ps1             normal boot must reach "VibeCodedOs kernel ready."
#   boot-test.ps1 -CrashTest  kernel deliberately page-faults; the exception
#                             report must show up (checks our fault handling)

param([int]$TimeoutSeconds = 20, [switch]$Release, [switch]$CrashTest)

. "$PSScriptRoot\common.ps1"

$expected = if ($CrashTest) { 'EXCEPTION: Page Fault' } else { 'VibeCodedOs kernel ready.' }
$log = Join-Path $BuildDir 'serial.log'

# The kernel checks this variable at compile time (option_env!).
if ($CrashTest) { $env:VIBE_CRASH_TEST = '1' } else { Remove-Item Env:VIBE_CRASH_TEST -ErrorAction SilentlyContinue }
Invoke-Build -Release:$Release
Remove-Item $log -ErrorAction SilentlyContinue

# Start-Process joins arguments with spaces, so quote those containing one.
$qemuArgs = @(Get-QemuArgs) + @('-display', 'none', '-serial', "file:$log") |
    ForEach-Object { if ($_ -match ' ') { "`"$_`"" } else { $_ } }
$qemu = Start-Process $QemuExe -ArgumentList $qemuArgs -PassThru -NoNewWindow

$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
$found = $false
while ((Get-Date) -lt $deadline -and -not $qemu.HasExited) {
    if ((Test-Path $log) -and (Select-String -Path $log -SimpleMatch $expected -Quiet)) {
        $found = $true
        # Give the rest of a multi-line report a moment to arrive.
        Start-Sleep -Milliseconds 500
        break
    }
    Start-Sleep -Milliseconds 250
}
if (-not $qemu.HasExited) { Stop-Process $qemu -Force }

if (Test-Path $log) { Get-Content $log }
if ($found) {
    Write-Host "`nBOOT TEST OK ('$expected')" -ForegroundColor Green
    exit 0
}
Write-Host "`nBOOT TEST FAILED (no '$expected' within $TimeoutSeconds s)" -ForegroundColor Red
exit 1
