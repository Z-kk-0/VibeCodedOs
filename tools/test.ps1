# Runs every check we have, fastest first. Stops at the first failure.
#
#   1. Formatting       cargo fmt --check
#   2. Lints            clippy on host crates (incl. tests), kernel, loader
#   3. Unit tests       cargo test – pure-logic crates, runs on the host
#   4. Boot tests       QEMU: normal boot + deliberate crash
#
#   test.ps1 -Quick     only steps 1–3 (no QEMU)

param([switch]$Quick)

. "$PSScriptRoot\common.ps1"
Set-Location $Root

function Step([string]$name, [scriptblock]$command) {
    Write-Host "`n=== $name ===" -ForegroundColor Cyan
    & $command
    if ($LASTEXITCODE -ne 0) {
        Write-Host "`nFAILED: $name" -ForegroundColor Red
        exit 1
    }
}

Step 'Formatting' { cargo fmt --check }
Step 'Lints: host crates' { cargo clippy --all-targets -- -D warnings }
# The loader embeds the kernel, so the kernel must exist before the loader is checked.
Step 'Lints: kernel' { cargo clippy -p kernel --target x86_64-unknown-none -- -D warnings }
Step 'Build: kernel' { cargo build -p kernel --target x86_64-unknown-none }
Step 'Lints: loader' { cargo clippy -p loader --target x86_64-unknown-uefi -- -D warnings }
Step 'Unit tests' { cargo test }

if (-not $Quick) {
    Step 'Boot test' { powershell -ExecutionPolicy Bypass -File "$PSScriptRoot\boot-test.ps1" }
    Step 'Crash test' { powershell -ExecutionPolicy Bypass -File "$PSScriptRoot\boot-test.ps1" -CrashTest }
}

Write-Host "`nALL CHECKS PASSED" -ForegroundColor Green
