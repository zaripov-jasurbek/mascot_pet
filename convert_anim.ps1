# Конвертирует FBX-анимацию (например, из Mixamo) в VRMA и кладёт в assets/anim/.
#
# Использование:
#   .\convert_anim.ps1 "C:\Users\zarip\Downloads\Walking.fbx"
#   .\convert_anim.ps1 "C:\Users\zarip\Downloads\Walking.fbx" walk   # своё имя
#
# Можно скормить папку — сконвертит все .fbx внутри:
#   .\convert_anim.ps1 "C:\Users\zarip\Downloads\mixamo_pack"

param(
    [Parameter(Mandatory = $true)] [string]$Input,
    [string]$Name = ""
)

$NodeDir   = "D:\apps\tools\node-v24.16.0-win-x64"
$Converter = "D:\apps\tools\fbx2vrma-converter\fbx2vrma-converter.js"
$AnimDir   = "D:\apps\ai_agent\assets\anim"

$env:Path = "$env:Path;$NodeDir"
New-Item -ItemType Directory -Force $AnimDir | Out-Null

function Convert-One($fbx, $outName) {
    if (-not $outName) { $outName = [IO.Path]::GetFileNameWithoutExtension($fbx) }
    # имя файла в нижнем регистре, пробелы → подчёркивания
    $outName = $outName.ToLower() -replace '\s+', '_'
    $out = Join-Path $AnimDir "$outName.vrma"
    Write-Host "→ $([IO.Path]::GetFileName($fbx))  =>  assets\anim\$outName.vrma"
    & node $Converter -i $fbx -o $out
}

if (Test-Path $Input -PathType Container) {
    $files = Get-ChildItem $Input -Filter *.fbx
    if (-not $files) { Write-Host "В папке нет .fbx файлов"; exit 1 }
    foreach ($f in $files) { Convert-One $f.FullName "" }
} elseif (Test-Path $Input) {
    Convert-One $Input $Name
} else {
    Write-Host "Файл/папка не найдены: $Input"; exit 1
}

Write-Host ""
Write-Host "Готово. Запусти: cargo run"
