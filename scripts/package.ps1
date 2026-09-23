# Packs the release build into dist/erindi-<version>-windows-x64.zip.
# Layout: the exe with its DLLs and llama/ (llama-server for the prompt refiner). Models download from Settings.
$ErrorActionPreference = 'Stop'

$root = Resolve-Path (Join-Path $PSScriptRoot '..')
$release = Join-Path $root 'target\release'
$version = (Get-Content (Join-Path $root 'apps\desktop\src-tauri\tauri.conf.json') | ConvertFrom-Json).version
$name = "erindi-$version-windows-x64"
$stage = Join-Path $root "dist\$name"

Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null

Copy-Item (Join-Path $release 'erindi-desktop.exe') (Join-Path $stage 'erindi.exe')
Copy-Item (Join-Path $release '*.dll') $stage
$llamaZip = Join-Path $root 'dist\llama-vulkan.zip'
if (-not (Test-Path $llamaZip)) {
    Invoke-WebRequest 'https://github.com/ggml-org/llama.cpp/releases/download/b11095/llama-b11095-bin-win-vulkan-x64.zip' -OutFile $llamaZip
}
if ((Get-FileHash $llamaZip -Algorithm SHA256).Hash.ToLower() -ne '45c586f50af57b7e144aa76c6fc38c544a717c4f4b7b659c489b980f3993412c') {
    Remove-Item $llamaZip
    throw 'llama.cpp: SHA-256 mismatch'
}
Expand-Archive $llamaZip (Join-Path $stage 'llama') -Force
Copy-Item (Join-Path $root 'README.md') $stage

$zip = Join-Path $root "dist\$name.zip"
Remove-Item -Force $zip -ErrorAction SilentlyContinue
Compress-Archive -Path "$stage\*" -DestinationPath $zip
Write-Host $zip
