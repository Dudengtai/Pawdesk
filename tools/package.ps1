# Build portable PawDesk package (M5 / QA-06).
# Usage: powershell -ExecutionPolicy Bypass -File tools/package.ps1
#
# Ships only runtime files: pawdesk.exe + runtime assets (no _master/_video/tools junk).
# Formal installer (Setup.exe) is produced by tools/make-installer.ps1.

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Get-CargoVersion {
    $toml = Get-Content (Join-Path $root "Cargo.toml") -Raw
    if ($toml -match '(?m)^version\s*=\s*"([^"]+)"') { return $Matches[1] }
    return "0.1.0"
}

$version = Get-CargoVersion
$skipBuild = ($env:PAWDESK_SKIP_BUILD -eq "1") -or ($args -contains "-SkipBuild")

if ($skipBuild) {
    $exe = Join-Path $root "target\release\pawdesk.exe"
    if (-not (Test-Path $exe)) { throw "PAWDESK_SKIP_BUILD set but $exe missing" }
    Write-Host "== skip cargo build (using existing release) =="
} else {
    Write-Host "== cargo build --release =="
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }
}

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
    "assets\pets\cow-cat\idle_placeholder.json",
    "assets\tray\_gen"
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

$utf8NoBom = New-Object System.Text.UTF8Encoding $false
$readme = [System.IO.File]::ReadAllText((Join-Path $root "tools\installer\README.txt")).Replace("{VERSION}", $version)
[System.IO.File]::WriteAllText((Join-Path $dist "README.txt"), $readme.Trim() + "`r`n", $utf8NoBom)
[System.IO.File]::WriteAllText((Join-Path $dist "VERSION.txt"), ("PawDesk {0}`r`n" -f $version), $utf8NoBom)
Copy-Item (Join-Path $root "tools\installer\LICENSE.txt") (Join-Path $dist "LICENSE.txt") -Force

$icoSrc = Join-Path $root "tools\installer\pawdesk.ico"
if (Test-Path $icoSrc) {
    Copy-Item $icoSrc (Join-Path $dist "pawdesk.ico") -Force
}

Write-Host "OK package at $dist"
Get-ChildItem $dist | Format-Table Name, Length
$bytes = (Get-ChildItem $dist -Recurse -File | Measure-Object -Property Length -Sum).Sum
Write-Host ("package size: {0:N1} MB" -f ($bytes / 1MB))
