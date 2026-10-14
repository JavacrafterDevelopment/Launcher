# Assembles a release package from a built Launcher.exe. Used by build.ps1 and the
# release workflow, so a local build and a GitHub release are laid out the same.
#
#   tools\package.ps1 -Exe target\release\Launcher.exe -Out dist [-Extra a.dll,b.dll]
#
# Writes:
#   <Out>\Launcher\                     the portable folder, ready to run
#   <Out>\Launcher-windows-x64.zip      that folder zipped: what the updater downloads
#
# Anything in the repo's package\ folder ships too, subfolders included — put any
# file a future version needs beside the exe there and updates will carry it.
#
# Every file is listed in launcher-manifest.json with its size and SHA-256. The
# updater checks each extracted file against it before swapping anything in, and
# uses the installed copy's manifest to remove files a new version no longer ships.

param(
    [Parameter(Mandatory)] [string]$Exe,
    [Parameter(Mandatory)] [string]$Out,
    [string[]]$Extra = @()
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem

$root     = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$version  = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
New-Item -ItemType Directory -Force $Out | Out-Null
$Out      = (Resolve-Path $Out).Path
$folder   = Join-Path $Out 'Launcher'
$zipPath  = Join-Path $Out 'Launcher-windows-x64.zip'
$manifest = 'launcher-manifest.json'

# Fresh program files, but never touch a data\ folder someone has been running from.
if (Test-Path $folder) {
    Get-ChildItem $folder -Force | Where-Object { $_.Name -ne 'data' } | Remove-Item -Recurse -Force
} else {
    New-Item -ItemType Directory -Force $folder | Out-Null
}

Copy-Item $Exe (Join-Path $folder 'Launcher.exe')
foreach ($f in $Extra) { Copy-Item $f $folder }
$extras = Join-Path $root 'package'
if (Test-Path $extras) { Copy-Item (Join-Path $extras '*') $folder -Recurse -Force }

$files = Get-ChildItem $folder -Recurse -File |
    Where-Object { $_.FullName -notlike (Join-Path $folder 'data\*') -and $_.Name -ne $manifest } |
    Sort-Object FullName |
    ForEach-Object {
        [ordered]@{
            path   = $_.FullName.Substring($folder.Length + 1).Replace('\', '/')
            size   = $_.Length
            sha256 = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()
        }
    }

$json = [ordered]@{ version = $version; exe = 'Launcher.exe'; files = @($files) } | ConvertTo-Json -Depth 4
[IO.File]::WriteAllText((Join-Path $folder $manifest), $json, (New-Object Text.UTF8Encoding $false))

# Built entry by entry: Windows PowerShell's Compress-Archive writes backslashes
# into nested entry names, which zip readers treat as part of the file name.
if (Test-Path $zipPath) { Remove-Item $zipPath }
$zip = [IO.Compression.ZipFile]::Open($zipPath, 'Create')
try {
    foreach ($f in @($files.path) + $manifest) {
        $src = Join-Path $folder ($f.Replace('/', '\'))
        [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $src, $f, 'Optimal')
    }
} finally { $zip.Dispose() }

Write-Output "Packaged Launcher $version ($($files.Count) files) -> $zipPath"
