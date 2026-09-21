# Downloads the ASR and VAD models into models/ and verifies their SHA-256.
# Parakeet TDT 0.6B v3 is CC-BY-4.0 (NVIDIA); Silero VAD is MIT.
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
