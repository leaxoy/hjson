# Windows package: a zip with the exe. Assumes
#   cargo build --release --target $env:TARGET
# has already run.
#
#   pwsh scripts/package-windows.ps1 -Target x86_64-pc-windows-msvc
param(
    [Parameter(Mandatory = $true)][string]$Target,
    [string]$Version = "",
    [string]$Dist = "dist"
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot

if (-not $Version) {
    $line = Get-Content "$Root/Cargo.toml" | Select-String -Pattern '^version = "([^"]+)"' | Select-Object -First 1
    $Version = $line.Matches[0].Groups[1].Value
}

$Arch = switch ($Target.Split('-')[0]) {
    "x86_64" { "x64" }
    "aarch64" { "arm64" }
    default { $Target.Split('-')[0] }
}

$Bin = Join-Path $Root "target\$Target\release\hjson.exe"
if (-not (Test-Path $Bin)) { throw "no binary at $Bin — build it first" }

New-Item -ItemType Directory -Force -Path $Dist | Out-Null
$Zip = Join-Path $Dist "hjson-$Version-windows-$Arch.zip"
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path $Bin -DestinationPath $Zip

Write-Host "packaged $Zip"
