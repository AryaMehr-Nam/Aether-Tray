# Builds a Windows distribution zip into dist/.
#
# A zip rather than an .msi: an installer buys nothing here. There is no
# service to register, no driver, no elevation, and launch-at-login is a
# per-user HKCU Run key the app writes itself from its own menu.
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$target = if ($env:TARGET) { $env:TARGET } else { 'x86_64-pc-windows-msvc' }
$version = (Select-String -Path "$root\Cargo.toml" -Pattern '^version = "(.*)"').Matches[0].Groups[1].Value

Push-Location $root
try {
    cargo build --release --locked --target $target
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

    $stage = "$root\dist\aether-tray"
    Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $stage | Out-Null

    Copy-Item "target\$target\release\aether-tray.exe" "$stage\aether-tray.exe"

    # Looked up next to the executable first, so shipping it here means the
    # user does not have to fetch the core separately.
    if (Test-Path "$root\binaries\aether.exe") {
        Copy-Item "$root\binaries\aether.exe" "$stage\aether.exe"
    } else {
        Write-Warning "binaries\aether.exe not found - download it into binaries\ first"
    }

    Copy-Item "$root\README.md" "$stage\README.md"
    Copy-Item "$root\LICENSE" "$stage\LICENSE.txt"

    $zip = "$root\dist\Aether-$version-x86_64-windows.zip"
    Remove-Item -Force $zip -ErrorAction SilentlyContinue
    Compress-Archive -Path "$stage\*" -DestinationPath $zip
    Write-Host "Built $zip"
} finally {
    Pop-Location
}
