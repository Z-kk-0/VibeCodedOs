# Builds the loader and boots it in QEMU.
#
# QEMU can expose a plain host folder as a FAT disk ("fat:rw:<dir>"), so we
# don't need to create a disk image. UEFI firmware looks for the default boot
# file \EFI\BOOT\BOOTX64.EFI on any FAT partition (UEFI spec 2.10, §3.5.1).

param([switch]$Release)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$profileDir = if ($Release) { 'release' } else { 'debug' }

$cargoArgs = @('build', '-p', 'loader', '--target', 'x86_64-unknown-uefi')
if ($Release) { $cargoArgs += '--release' }
cargo @cargoArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$esp = Join-Path $root 'build\esp'
$bootDir = Join-Path $esp 'EFI\BOOT'
New-Item -ItemType Directory -Force $bootDir | Out-Null
Copy-Item (Join-Path $root "target\x86_64-unknown-uefi\$profileDir\loader.efi") (Join-Path $bootDir 'BOOTX64.EFI')

$qemuDir = if ($env:QEMU_DIR) { $env:QEMU_DIR } else { 'C:\Program Files\qemu' }
$qemu = Join-Path $qemuDir 'qemu-system-x86_64.exe'
# OVMF/edk2 UEFI firmware ships with the Windows build of QEMU.
$firmware = Join-Path $qemuDir 'share\edk2-x86_64-code.fd'

& $qemu `
    -machine q35 `
    -m 256M `
    -drive "if=pflash,format=raw,readonly=on,file=$firmware" `
    -drive "format=raw,file=fat:rw:$esp" `
    -net none `
    -serial stdio  # OVMF mirrors the text console to the serial port -> visible in this terminal too
