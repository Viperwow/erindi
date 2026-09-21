# Packs the release build into dist/ella-<version>-windows-x64.zip.
# Layout: the exe with its DLLs, plus scripts/fetch-models.ps1, which downloads into models/ next to the exe.
$ErrorActionPreference = 'Stop'

$root = Resolve-Path (Join-Path $PSScriptRoot '..')
$release = Join-Path $root 'target\release'
$version = (Get-Content (Join-Path $root 'apps\desktop\src-tauri\tauri.conf.json') | ConvertFrom-Json).version
$name = "ella-$version-windows-x64"
$stage = Join-Path $root "dist\$name"

Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $stage 'scripts') | Out-Null

Copy-Item (Join-Path $release 'ella-desktop.exe') (Join-Path $stage 'ella.exe')
Copy-Item (Join-Path $release '*.dll') $stage
Copy-Item (Join-Path $root 'scripts\fetch-models.ps1') (Join-Path $stage 'scripts')
Copy-Item (Join-Path $root 'README.md') $stage

$zip = Join-Path $root "dist\$name.zip"
Remove-Item -Force $zip -ErrorAction SilentlyContinue
Compress-Archive -Path "$stage\*" -DestinationPath $zip
Write-Host $zip
