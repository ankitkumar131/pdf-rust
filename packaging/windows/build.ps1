<#
.SYNOPSIS
    Builds the Windows release artifacts for pdf-rust.

.DESCRIPTION
    Produces, in dist\:
      pdf-rust.exe                 release build with the icon and version stamp
      pdfium.dll                   the PDFium build the viewer loads at runtime
      pdf-rust-<version>-win64.zip portable archive
      pdf-rust-<version>-setup.exe NSIS installer (when makensis is available)

    Run from a "x64 Native Tools" prompt for the MSVC resource compiler, or with
    a MinGW toolchain on PATH so build.rs can use windres.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File packaging\windows\build.ps1
#>
[CmdletBinding()]
param(
    [string] $Version = "",
    [string] $PdfiumDll = "",
    [switch] $SkipTests
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
Push-Location $root
try {
    if (-not $Version) {
        $line = Select-String -Path "Cargo.toml" -Pattern '^version = "(.*)"' | Select-Object -First 1
        $Version = $line.Matches[0].Groups[1].Value
    }
    if (-not $PdfiumDll) {
        $PdfiumDll = Join-Path $root "runtime\pdfium\pdfium.dll"
    }
    if (-not (Test-Path $PdfiumDll)) {
        throw "PDFium not found at $PdfiumDll (pass -PdfiumDll <path>)"
    }

    if (-not $SkipTests) {
        Write-Host "==> cargo test"
        cargo test --offline
        if ($LASTEXITCODE -ne 0) { throw "tests failed" }
    }

    Write-Host "==> cargo build --release"
    cargo build --offline --release
    if ($LASTEXITCODE -ne 0) { throw "release build failed" }

    $dist = Join-Path $root "dist"
    $pkg = Join-Path $dist "pdf-rust-$Version-win64"
    New-Item -ItemType Directory -Force -Path $pkg | Out-Null

    $exe = Join-Path $root "target\release\pdf-rust.exe"
    if (-not (Test-Path $exe)) { throw "missing $exe" }
    Copy-Item $exe (Join-Path $pkg "pdf-rust.exe") -Force
    Copy-Item $PdfiumDll (Join-Path $pkg "pdfium.dll") -Force
    foreach ($doc in "README.md", "CHANGELOG.md", "LICENSE") {
        $src = Join-Path $root $doc
        if (Test-Path $src) { Copy-Item $src (Join-Path $pkg $doc) -Force }
    }
    Copy-Item (Join-Path $root "packaging\windows\pdf-rust.exe.manifest") $pkg -Force

    $zip = Join-Path $dist "pdf-rust-$Version-win64.zip"
    if (Test-Path $zip) { Remove-Item $zip -Force }
    Compress-Archive -Path (Join-Path $pkg "*") -DestinationPath $zip
    Write-Host "==> $zip ($([math]::Round((Get-Item $zip).Length / 1MB, 2)) MB)"

    $makensis = Get-Command makensis -ErrorAction SilentlyContinue
    if ($makensis) {
        Write-Host "==> makensis"
        & $makensis.Source `
            "/DAPP_VERSION=$Version" `
            "/DSOURCE_DIR=$root\target\release" `
            "/DPDFIUM_DLL=$PdfiumDll" `
            (Join-Path $root "packaging\windows\installer.nsi")
        if ($LASTEXITCODE -ne 0) { throw "makensis failed" }
    } else {
        Write-Host "makensis not found: skipping the installer (install NSIS to build it)."
    }
} finally {
    Pop-Location
}
