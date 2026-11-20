<#
.SYNOPSIS
    Builds the Windows release artifacts for pdf-rust.

.DESCRIPTION
    Produces, in dist\:
      pdf-rust.exe                 release build with the icon and version stamp
      pdfium.dll                   the PDFium build the viewer loads at runtime
      pdf-rust-<version>-win64.zip portable archive
      pdf-rust-<version>-setup.exe NSIS installer (when makensis is available)

    build.rs locates the resource compiler itself (a MinGW windres on PATH, or
    rc.exe/cvtres.exe from the installed Windows SDK and MSVC toolchain), so a
    normal shell is enough; the .exe gets its icon and version stamp either way.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File packaging\windows\build.ps1
#>
[CmdletBinding()]
param(
    [string] $Version = "",
    [string] $PdfiumDll = "",
    [switch] $SkipTests,
    [switch] $SkipInstaller,
    # Fail (instead of warning) when the icon/version resource is missing. CI uses
    # this so a binary without an icon can never reach a release again.
    [switch] $RequireResources
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

    # The viewer must be a GUI-subsystem binary: a console subsystem makes Windows
    # open a terminal window every time a PDF is opened from Explorer.
    $bytes = [System.IO.File]::ReadAllBytes($exe)
    $peOffset = [System.BitConverter]::ToInt32($bytes, 0x3C)
    $subsystem = [System.BitConverter]::ToUInt16($bytes, $peOffset + 0x5C)
    if ($subsystem -ne 2) {
        throw "pdf-rust.exe uses subsystem $subsystem, expected 2 (WINDOWS): it would open a console window"
    }
    Write-Host "==> subsystem: WINDOWS (no console window)"

    # The icon and version stamp come from build.rs; a missing version resource
    # means rc.exe/cvtres.exe were not found and the exe would show a generic icon.
    $versionInfo = (Get-Item $exe).VersionInfo
    if (-not $versionInfo.FileDescription) {
        $message = "pdf-rust.exe has no version resource: the icon and version stamp were not embedded (install the Windows SDK, or set PDFRUST_RESOURCE_COMPILER)"
        if ($RequireResources) { throw $message } else { Write-Warning $message }
    } else {
        Write-Host "==> resources: $($versionInfo.FileDescription) $($versionInfo.FileVersion)"
    }

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
    if ($SkipInstaller) {
        Write-Host "installer skipped (-SkipInstaller)"
    } elseif ($makensis) {
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
