# Build portable PawDesk package (M5 / QA-06).
# Usage: powershell -ExecutionPolicy Bypass -File tools/package.ps1
#
# Ships only runtime files: pawdesk.exe + runtime assets (no _master/_video/tools junk).

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

# Strip non-runtime / authoring-only material from the portable package.
$strip = @(
    "assets\pets\cow-cat\_video",
    "assets\pets\cow-cat\_master",
    "assets\pets\cow-cat\idle_placeholder.png",
    "assets\pets\cow-cat\idle_placeholder.json"
)
foreach ($rel in $strip) {
    $p = Join-Path $dist $rel
    if (Test-Path $p) {
        Remove-Item -Recurse -Force $p -ErrorAction SilentlyContinue
        Write-Host "stripped $rel"
    }
}
Get-ChildItem -Path (Join-Path $dist "assets") -Recurse -Directory -Filter "__pycache__" -ErrorAction SilentlyContinue |
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue

$readme = @(
    "PawDesk portable",
    "",
    "Double-click pawdesk.exe to start.",
    "Exit via system tray menu.",
    "",
    "Tray: show/hide, pet larger/smaller, pause reminder, settings, exit.",
    "Settings: reminder + pet size + shortcuts.",
    "",
    "Config: %APPDATA%\PawDesk\config.json",
    "Logs:   %LOCALAPPDATA%\PawDesk\logs\",
    "",
    "Keep the assets\ folder next to pawdesk.exe."
) -join "`r`n"
Set-Content -Path (Join-Path $dist "README.txt") -Value $readme -Encoding UTF8

Write-Host "OK package at $dist"
Get-ChildItem $dist | Format-Table Name, Length
$bytes = (Get-ChildItem $dist -Recurse -File | Measure-Object -Property Length -Sum).Sum
Write-Host ("package size: {0:N1} MB" -f ($bytes / 1MB))
