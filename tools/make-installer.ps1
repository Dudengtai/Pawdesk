# Build a per-user Windows installer (Inno Setup) plus a portable zip.
# Usage:
#   powershell -ExecutionPolicy Bypass -File tools/make-installer.ps1
#
# Outputs (under dist/):
#   PawDesk/                          portable folder
#   PawDesk-<ver>-portable.zip        zip of the portable folder
#   PawDesk-Setup-<ver>.exe           installer (no admin)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Get-CargoVersion {
    $toml = Get-Content (Join-Path $root "Cargo.toml") -Raw
    if ($toml -match '(?m)^version\s*=\s*"([^"]+)"') { return $Matches[1] }
    return "0.1.0"
}

function Find-ISCC {
    $cmd = Get-Command iscc -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $candidates = @(
        "${env:LocalAppData}\Programs\Inno Setup 6\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "${env:ProgramFiles}\Inno Setup 6\ISCC.exe",
        (Join-Path $root "tools\installer\innosetup\ISCC.exe")
    )
    foreach ($p in $candidates) {
        if ($p -and (Test-Path $p)) { return $p }
    }
    return $null
}

function Install-InnoSetup {
    $dest = Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6"
    $installer = Join-Path $env:TEMP "innosetup-pawsdesk.exe"
    $urls = @(
        "https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe",
        "https://github.com/jrsoftware/issrc/releases/download/is-6_6_1/innosetup-6.6.1.exe"
    )

    Write-Host "== installing Inno Setup (per-user, no admin) =="

    $winget = Get-Command winget -ErrorAction SilentlyContinue
    if ($winget) {
        Write-Host "trying winget JRSoftware.InnoSetup"
        $wg = Start-Process -FilePath $winget.Source -ArgumentList @(
            "install", "--id", "JRSoftware.InnoSetup", "-e",
            "--accept-source-agreements", "--accept-package-agreements",
            "--disable-interactivity", "--scope", "user"
        ) -Wait -PassThru -NoNewWindow
        Write-Host "winget exit $($wg.ExitCode)"
        $found = Find-ISCC
        if ($found) { return $found }
        Write-Host "winget did not yield ISCC, falling back to direct download"
    }

    $downloaded = $false
    foreach ($url in $urls) {
        try {
            Write-Host "downloading $url"
            Invoke-WebRequest -Uri $url -OutFile $installer -UseBasicParsing
            if ((Test-Path $installer) -and ((Get-Item $installer).Length -gt 1MB)) {
                $downloaded = $true
                break
            }
        } catch {
            Write-Host "download failed: $_"
        }
    }
    if (-not $downloaded) {
        throw "Failed to download Inno Setup. Install it from https://jrsoftware.org/isinfo.php and re-run."
    }

    $args = @(
        "/VERYSILENT",
        "/SUPPRESSMSGBOXES",
        "/NORESTART",
        "/CURRENTUSER",
        "/SP-",
        "/DIR=$dest"
    )
    $proc = Start-Process -FilePath $installer -ArgumentList $args -Wait -PassThru
    if ($proc.ExitCode -ne 0 -and $proc.ExitCode -ne 3010) {
        throw "Inno Setup silent install failed (exit $($proc.ExitCode))"
    }
    $iscc = Join-Path $dest "ISCC.exe"
    if (-not (Test-Path $iscc)) {
        throw "ISCC.exe not found after install at $iscc"
    }
    return $iscc
}

$version = Get-CargoVersion
Write-Host "PawDesk version $version"

$isl = Join-Path $root "tools\installer\ChineseSimplified.isl"
if (-not (Test-Path $isl)) {
    Write-Host "== download ChineseSimplified.isl =="
    Invoke-WebRequest -Uri "https://raw.githubusercontent.com/kira-96/Inno-Setup-Chinese-Simplified-Translation/main/ChineseSimplified.isl" -OutFile $isl -UseBasicParsing
}

Write-Host "== generate installer icon =="
python (Join-Path $root "tools\installer\make_icon.py")
if ($LASTEXITCODE -ne 0) { throw "make_icon.py failed" }

Write-Host "== portable package =="
$skip = @()
if ($env:PAWDESK_SKIP_BUILD -eq "1") { $skip = @("-SkipBuild") }
powershell -ExecutionPolicy Bypass -File (Join-Path $root "tools\package.ps1") @skip
if ($LASTEXITCODE -ne 0) { throw "package.ps1 failed" }

$distApp = Join-Path $root "dist\PawDesk"
$ico = Join-Path $root "tools\installer\pawdesk.ico"
if (Test-Path $ico) {
    Copy-Item $ico (Join-Path $distApp "pawdesk.ico") -Force
}

$iscc = Find-ISCC
if (-not $iscc) {
    $iscc = Install-InnoSetup
}
Write-Host "ISCC: $iscc"

$iss = Join-Path $root "tools\installer\PawDesk.iss"
$outDir = Join-Path $root "dist"
Write-Host "== compile installer =="
& $iscc `
    "/DMyAppVersion=$version" `
    "/DDistDir=$distApp" `
    "/DOutputDir=$outDir" `
    $iss
if ($LASTEXITCODE -ne 0) { throw "ISCC failed" }

$zip = Join-Path $outDir "PawDesk-$version-portable.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Write-Host "== zip portable package =="
Compress-Archive -Path (Join-Path $distApp "*") -DestinationPath $zip -CompressionLevel Optimal

$setup = Join-Path $outDir "PawDesk-Setup-$version.exe"
Write-Host ""
Write-Host "OK installer artifacts:"
Get-ChildItem $outDir -File | Format-Table Name, @{N="MB";E={"{0:N2}" -f ($_.Length / 1MB)}} -AutoSize
if (-not (Test-Path $setup)) { throw "missing $setup" }
Write-Host "Installer: $setup"
Write-Host "Portable:  $zip"
Write-Host "Give others the Setup exe. They can install without admin."
