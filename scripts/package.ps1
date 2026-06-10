# Собирает готовую сборку «скачал-распаковал-кликнул» для не-разработчиков.
# На выходе: dist\mascot\  (exe + DLL + модель + assets + voice.toml) и dist\mascot-win64.zip
#
# Требует, чтобы libvosk и модель уже были на месте (scripts\fetch-deps.ps1).
# Запуск из корня:  powershell -ExecutionPolicy Bypass -File scripts\package.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$voskDir  = "vendor\vosk-win64-0.3.45"
$modelDir = "models\vosk-model-small-ru-0.22"
$out      = "dist\mascot"

if (-not (Test-Path "$voskDir\libvosk.dll")) { throw "Нет libvosk — запусти scripts\fetch-deps.ps1" }
if (-not (Test-Path $modelDir))              { throw "Нет модели — запусти scripts\fetch-deps.ps1" }

Write-Host "1/4  cargo build --release ..."
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "Сборка упала" }

Write-Host "2/4  Собираю папку $out ..."
if (Test-Path "dist") { Remove-Item "dist" -Recurse -Force }
New-Item -ItemType Directory -Force -Path "$out\models" | Out-Null

Copy-Item "target\release\ai_agent.exe" "$out\mascot.exe"
Copy-Item "$voskDir\*.dll" $out                       # libvosk + libgcc/libstdc++/libwinpthread
Copy-Item $modelDir "$out\models\" -Recurse
Copy-Item "assets" "$out\assets" -Recurse
Copy-Item "voice.toml" $out
if (Test-Path "README-user.txt") { Copy-Item "README-user.txt" "$out\ПРОЧТИ-МЕНЯ.txt" }

Write-Host "3/4  Пакую zip ..."
$zip = "dist\mascot-win64.zip"
Compress-Archive -Path "$out\*" -DestinationPath $zip -Force

$size = "{0:N1} МБ" -f ((Get-Item $zip).Length / 1MB)
Write-Host "4/4  Готово: $zip ($size)"
Write-Host "Пользователь: распаковать и запустить mascot.exe (двойной клик)."

