# Shared helpers for run.ps1 and boot-test.ps1. Dot-source this file.

$ErrorActionPreference = 'Stop'
$Root = Split-Path $PSScriptRoot -Parent
$BuildDir = Join-Path $Root 'build'
$EspDir = Join-Path $BuildDir 'esp'

$QemuDir = if ($env:QEMU_DIR) { $env:QEMU_DIR } else { 'C:\Program Files\qemu' }
$QemuExe = Join-Path $QemuDir 'qemu-system-x86_64.exe'

# Builds kernel, then loader (which embeds the kernel), and lays out the
# EFI system partition folder. UEFI looks for \EFI\BOOT\BOOTX64.EFI on any
# FAT partition (UEFI spec 2.10, §3.5.1).
function Invoke-Build([switch]$Release) {
    $profileDir = if ($Release) { 'release' } else { 'debug' }
    $releaseFlag = if ($Release) { @('--release') } else { @() }

    # Arrays expand to separate arguments. (Splatting with @ would mangle "--release" in PowerShell 5.1.)
    cargo build -p kernel --target x86_64-unknown-none $releaseFlag
    if ($LASTEXITCODE -ne 0) { throw 'kernel build failed' }

    $env:HEMATITE_KERNEL_ELF = Join-Path $Root "target\x86_64-unknown-none\$profileDir\kernel"
    cargo build -p loader --target x86_64-unknown-uefi $releaseFlag
    if ($LASTEXITCODE -ne 0) { throw 'loader build failed' }

    $bootDir = Join-Path $EspDir 'EFI\BOOT'
    New-Item -ItemType Directory -Force $bootDir | Out-Null
    Copy-Item (Join-Path $Root "target\x86_64-unknown-uefi\$profileDir\loader.efi") (Join-Path $bootDir 'BOOTX64.EFI')
}

# QEMU can expose a plain folder as a FAT disk ("fat:rw:<dir>"), so no disk
# image is needed. The OVMF/edk2 UEFI firmware ships with QEMU for Windows.
function Get-QemuArgs {
    $firmware = Join-Path $QemuDir 'share\edk2-x86_64-code.fd'
    @(
        '-machine', 'q35',
        '-m', '256M',
        '-drive', "if=pflash,format=raw,readonly=on,file=$firmware",
        '-drive', "format=raw,file=fat:rw:$EspDir",
        '-net', 'none'
    )
}
