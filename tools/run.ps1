# Builds everything and boots it in a QEMU window.
# Serial output (kernel log) appears in this terminal.

param([switch]$Release)

. "$PSScriptRoot\common.ps1"

Invoke-Build -Release:$Release
& $QemuExe @(Get-QemuArgs) -serial stdio
