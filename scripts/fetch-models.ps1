# Development only: users download models from Settings.
# Downloads the ASR and VAD models into models/ and verifies their SHA-256.
# -Refiner also fetches llama-server (Vulkan) into models/llama/ and the cleanup model.
# Parakeet TDT 0.6B v3 is CC-BY-4.0 (NVIDIA); Silero VAD is MIT; Qwen2.5-3B-Instruct is Qwen Research License.
param([switch]$Refiner)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$base = 'https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models'
$models = Join-Path $PSScriptRoot '..\models'
New-Item -ItemType Directory -Force $models | Out-Null

$files = @(
    @{ Name = 'silero_vad.onnx'; Sha = '9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6' },
    @{ Name = 'sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2'; Sha = '5793d0fd397c5778d2cf2126994d58e9d56b1be7c04d13c7a15bb1b4eafb16bf' }
)

foreach ($f in $files) {
    $target = Join-Path $models $f.Name
    if (Test-Path $target) {
        Write-Host "$($f.Name): present"
    } else {
        $partial = "$target.partial"
        Write-Host "$($f.Name): downloading"
        Invoke-WebRequest "$base/$($f.Name)" -OutFile $partial
        Move-Item $partial $target
    }
    $sha = (Get-FileHash $target -Algorithm SHA256).Hash.ToLower()
    if ($sha -ne $f.Sha) {
        Remove-Item $target
        throw "$($f.Name): SHA-256 mismatch ($sha), file removed"
    }
}

$parakeet = Join-Path $models 'sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8'
if (-not (Test-Path (Join-Path $parakeet 'tokens.txt'))) {
    tar -xjf (Join-Path $models 'sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2') -C $models
}
Write-Host 'models ready'

if ($Refiner) {
    $zip = Join-Path $models 'llama-vulkan.zip'
    if (-not (Test-Path $zip)) {
        Invoke-WebRequest 'https://github.com/ggml-org/llama.cpp/releases/download/b11095/llama-b11095-bin-win-vulkan-x64.zip' -OutFile $zip
    }
    if ((Get-FileHash $zip -Algorithm SHA256).Hash.ToLower() -ne '45c586f50af57b7e144aa76c6fc38c544a717c4f4b7b659c489b980f3993412c') {
        Remove-Item $zip
        throw 'llama.cpp: SHA-256 mismatch, file removed'
    }
    Expand-Archive $zip (Join-Path $models 'llama') -Force

    $gguf = Join-Path $models 'qwen2.5-3b-instruct-q4_k_m.gguf'
    if (-not (Test-Path $gguf)) {
        Invoke-WebRequest 'https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/7dabda4d13d513e3e842b20f0d435c732f172cbe/qwen2.5-3b-instruct-q4_k_m.gguf' -OutFile "$gguf.partial"
        Move-Item "$gguf.partial" $gguf
    }
    if ((Get-FileHash $gguf -Algorithm SHA256).Hash.ToLower() -ne '626b4a6678b86442240e33df819e00132d3ba7dddfe1cdc4fbb18e0a9615c62d') {
        Remove-Item $gguf
        throw 'cleanup model: SHA-256 mismatch, file removed'
    }
    Write-Host 'refiner ready'
}
