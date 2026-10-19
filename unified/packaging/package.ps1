<#
.SYNOPSIS
    Builds GitDaddy for Windows and packages it as an installer and a portable zip.

.DESCRIPTION
    Produces, in windows\dist:
      GitDaddy-<version>-windows-x64-setup.exe     Inno Setup installer (per-user, no admin needed)
      GitDaddy-<version>-windows-x64-portable.zip  GitDaddy.exe + LICENSE, runs from any folder
      GitDaddy-<version>-windows-x64-SHA256SUMS.txt

    Requires the Rust toolchain and Inno Setup 6 (https://jrsoftware.org/isdl.php).

.EXAMPLE
    .\packaging\package.ps1
.EXAMPLE
    .\packaging\package.ps1 -SkipBuild    # package an already built target\release\GitDaddy.exe
#>
param(
    [switch]$SkipBuild,
    [string]$Target = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $versionMatch = Select-String -Path "Cargo.toml" -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
    $version = $versionMatch.Matches[0].Groups[1].Value
    $base = "GitDaddy-$version-windows-x64"

    $targetArgs = @()
    $releaseDir = "target\release"
    if ($Target) {
        $targetArgs = @("--target", $Target)
        $releaseDir = "target\$Target\release"
    }
    if (-not $SkipBuild) {
        cargo build --release --locked @targetArgs
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    }
    $exe = Join-Path $releaseDir "GitDaddy.exe"
    if (-not (Test-Path $exe)) { throw "$exe not found; build first or drop -SkipBuild" }
    $exe = (Resolve-Path $exe).Path

    $dist = Join-Path $root "dist"
    New-Item -ItemType Directory -Force $dist | Out-Null
    Get-ChildItem $dist -File | Where-Object Name -like "$base*" | Remove-Item

    # Portable zip.
    $staging = Join-Path $dist "$base-portable"
    if (Test-Path $staging) { Remove-Item -Recurse -Force $staging }
    New-Item -ItemType Directory $staging | Out-Null
    Copy-Item $exe $staging
    Copy-Item (Join-Path $root "..\LICENSE") (Join-Path $staging "LICENSE.txt")
    Compress-Archive -Path (Join-Path $staging "*") -DestinationPath (Join-Path $dist "$base-portable.zip")
    Remove-Item -Recurse -Force $staging

    # Installer.
    $isccCandidates = @(
        (Get-Command "iscc.exe" -ErrorAction SilentlyContinue).Source,
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
    )
    $iscc = $isccCandidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
    if (-not $iscc) { throw "Inno Setup 6 (ISCC.exe) was not found. Install it from https://jrsoftware.org/isdl.php" }
    & $iscc /Qp "/DAppVersion=$version" "/DSourceExe=$exe" "packaging\GitDaddy.iss"
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed" }

    # Checksums.
    $artifacts = Get-ChildItem $dist -File | Where-Object { $_.Name -like "$base*" -and $_.Extension -in @(".zip", ".exe") }
    $artifacts |
        ForEach-Object { "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name } |
        Set-Content -Encoding ascii (Join-Path $dist "$base-SHA256SUMS.txt")

    Get-ChildItem $dist -File | Where-Object Name -like "$base*" | Format-Table Name, Length -AutoSize
}
finally {
    Pop-Location
}
