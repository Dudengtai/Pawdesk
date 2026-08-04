# Build portable PawDesk package (M5 / QA-06).
# Usage: powershell -ExecutionPolicy Bypass -File tools/package.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

Write-Host "== cargo build --release =="
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }

$dist = Join-Path $root "dist\PawDesk"
if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
New-Item -ItemType Directory -Force -Path $dist | Out-Null

Copy-Item (Join-Path $root "target\release\pawdesk.exe") $dist
Copy-Item -Recurse (Join-Path $root "assets") (Join-Path $dist "assets")

$video = Join-Path $dist "assets\pets\cow-cat\_video"
if (Test-Path $video) {
    Remove-Item -Recurse -Force $video -ErrorAction SilentlyContinue
}

$readme = @(
    "PawDesk portable",
    "Double-click pawdesk.exe to start.",
    "Exit via system tray menu.",
    "Config: %APPDATA%\PawDesk\config.json",
    "Logs: %LOCALAPPDATA%\PawDesk\logs\",
    "Keep assets/ next to the exe."
) -join "`r`n"
Set-Content -Path (Join-Path $dist "README.txt") -Value $readme -Encoding UTF8

Write-Host "OK package at $dist"
Get-ChildItem $dist | Format-Table Name, Length
