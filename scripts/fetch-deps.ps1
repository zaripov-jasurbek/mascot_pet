# Downloads native dependencies that are intentionally not stored in git:
#   - libvosk              -> vendor/vosk-win64-0.3.45/
#   - Russian Vosk model   -> models/vosk-model-small-ru-0.22/
#   - English Vosk model   -> models/vosk-model-small-en-us-0.15/
#
# Run from repo root:
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-deps.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$voskVer = "0.3.45"
$voskUrl = "https://github.com/alphacep/vosk-api/releases/download/v$voskVer/vosk-win64-$voskVer.zip"

$models = @(
    @{
        Lang = "RU"
        Name = "vosk-model-small-ru-0.22"
        Url  = "https://alphacephei.com/vosk/models/vosk-model-small-ru-0.22.zip"
    },
    @{
        Lang = "EN"
        Name = "vosk-model-small-en-us-0.15"
        Url  = "https://alphacephei.com/vosk/models/vosk-model-small-en-us-0.15.zip"
    }
)

New-Item -ItemType Directory -Force -Path vendor, models | Out-Null

if (-not (Test-Path "vendor\vosk-win64-$voskVer\libvosk.dll")) {
    Write-Host "Downloading libvosk $voskVer ..."
    Invoke-WebRequest -Uri $voskUrl -OutFile "vendor\vosk.zip"
    Expand-Archive -Path "vendor\vosk.zip" -DestinationPath "vendor" -Force
    Remove-Item "vendor\vosk.zip"
} else {
    Write-Host "libvosk is already present; skipping."
}

foreach ($model in $models) {
    $modelPath = "models\$($model.Name)"
    if (-not (Test-Path $modelPath)) {
        Write-Host "Downloading $($model.Lang) model: $($model.Name) ..."
        $zipPath = "models\$($model.Name).zip"
        Invoke-WebRequest -Uri $model.Url -OutFile $zipPath
        Expand-Archive -Path $zipPath -DestinationPath "models" -Force
        Remove-Item $zipPath
    } else {
        Write-Host "$($model.Lang) model is already present; skipping."
    }
}

Write-Host ""
Write-Host "Done. Next:"
Write-Host "  cargo run"
