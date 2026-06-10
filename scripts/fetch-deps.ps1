# Загружает нативные зависимости, которых нет в репозитории (они в .gitignore):
#   - libvosk (DLL + .lib) → vendor/vosk-win64-0.3.45/
#   - русская модель Vosk   → models/vosk-model-small-ru-0.22/
#
# Запуск из корня проекта:  powershell -ExecutionPolicy Bypass -File scripts\fetch-deps.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$voskVer  = "0.3.45"
$voskUrl  = "https://github.com/alphacep/vosk-api/releases/download/v$voskVer/vosk-win64-$voskVer.zip"
$modelUrl = "https://alphacephei.com/vosk/models/vosk-model-small-ru-0.22.zip"

New-Item -ItemType Directory -Force -Path vendor, models | Out-Null

if (-not (Test-Path "vendor\vosk-win64-$voskVer\libvosk.dll")) {
    Write-Host "Скачиваю libvosk $voskVer ..."
    Invoke-WebRequest -Uri $voskUrl -OutFile "vendor\vosk.zip"
    Expand-Archive -Path "vendor\vosk.zip" -DestinationPath "vendor" -Force
    Remove-Item "vendor\vosk.zip"
} else { Write-Host "libvosk уже на месте — пропускаю." }

if (-not (Test-Path "models\vosk-model-small-ru-0.22")) {
    Write-Host "Скачиваю русскую модель (~45 МБ) ..."
    Invoke-WebRequest -Uri $modelUrl -OutFile "models\model.zip"
    Expand-Archive -Path "models\model.zip" -DestinationPath "models" -Force
    Remove-Item "models\model.zip"
} else { Write-Host "Модель уже на месте — пропускаю." }

Write-Host "`nГотово. Теперь:"
Write-Host '  $env:PATH = "$PWD\vendor\vosk-win64-0.3.45;$env:PATH"'
Write-Host "  cargo run"

