# Builds the portable Launcher folder.
#
#   powershell -ExecutionPolicy Bypass -File build.ps1
#
# Produces dist\Launcher\ — copy that folder anywhere (a USB stick, another PC)
# and run Launcher.exe. Nothing is installed; settings live in data\ beside it.
#
# Also writes dist\Launcher-windows-x64.zip, the same package a release ships.
#
#   -Icon     redraws assets\ from tools\make-icon.ps1 first

param(
    [switch]$Icon
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$dist = Join-Path $root 'dist\Launcher'
Set-Location $root

Write-Host ''
Write-Host '  Launcher - portable build' -ForegroundColor DarkYellow
Write-Host '  -------------------------' -ForegroundColor DarkGray

$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    if (Test-Path (Join-Path $cargoBin 'cargo.exe')) { $env:PATH = "$cargoBin;$env:PATH" }
    else { throw 'Rust is not installed. Get it from https://rustup.rs and run this again.' }
}

if ($Icon) {
    Write-Host '  Drawing the icon'
    & (Join-Path $root 'tools\make-icon.ps1') | Out-Null
}

$hostTriple = ((& rustc -vV) | Select-String '^host: (.+)$').Matches[0].Groups[1].Value
$gnu = $hostTriple -like '*-windows-gnu'

if ($gnu) {
    # The `windows` crate imports DLL functions via raw-dylib, which on the GNU
    # toolchain needs a `dlltool`. LLVM's archiver acts as one when it is named
    # dlltool.exe, and rustup ships it in the llvm-tools component.
    if (-not (Get-Command dlltool -ErrorAction SilentlyContinue)) {
        Write-Host '  Setting up dlltool (rustup llvm-tools)'
        & rustup component add llvm-tools | Out-Null
        $sysroot = (& rustc --print sysroot).Trim()
        $llvmAr = Join-Path $sysroot "lib\rustlib\$hostTriple\bin\llvm-ar.exe"
        Copy-Item $llvmAr (Join-Path $cargoBin 'dlltool.exe') -Force
    }
}

# A running copy holds its exe open; say so plainly rather than fail on the copy.
$exe = Join-Path $dist 'Launcher.exe'
if (Test-Path $exe) {
    try { [IO.File]::Open($exe, 'Open', 'ReadWrite', 'None').Close() }
    catch { throw "Launcher is still running from $dist. Close it and build again." }
}

Write-Host "  Compiling (release, $hostTriple)"
& cargo build --release
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

$extra = @()
if ($gnu) {
    # MSVC builds link the WebView2 loader statically; GNU builds load it as a DLL,
    # so it has to travel with the exe. It ships inside the webview2-com-sys crate.
    $registry = Join-Path $env:USERPROFILE '.cargo\registry\src'
    $ver = ((& cargo tree -p webview2-com-sys --depth 0 -e normal) | Select-Object -First 1) -replace '^webview2-com-sys v', ''
    $dll = Get-ChildItem $registry -Directory |
        ForEach-Object { Join-Path $_.FullName "webview2-com-sys-$ver\x64\WebView2Loader.dll" } |
        Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $dll) { throw "WebView2Loader.dll for webview2-com-sys $ver not found under $registry" }
    $extra += $dll
}

# The same packaging the release workflow uses: folder, manifest and update zip.
& (Join-Path $root 'tools\package.ps1') -Exe (Join-Path $root 'target\release\Launcher.exe') `
    -Out (Join-Path $root 'dist') -Extra $extra | Out-Null

$version = ((Get-Content (Join-Path $root 'Cargo.toml')) | Select-String '^version = "(.+)"').Matches[0].Groups[1].Value
$size = [Math]::Round((Get-ChildItem $dist -File | Measure-Object Length -Sum).Sum / 1MB, 1)
Write-Host "  Built Launcher $version -> $dist ($size MB)" -ForegroundColor Green
Write-Host "  Package -> $(Join-Path $root 'dist\Launcher-windows-x64.zip')" -ForegroundColor Green
Write-Host ''
