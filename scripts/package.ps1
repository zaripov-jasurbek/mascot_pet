# Builds ready-to-run Windows packages for non-developers.
# Outputs:
#   dist\mascot_ru\  and dist\mascot_ru.zip
#   dist\mascot_en\  and dist\mascot_en.zip
#
# Requires libvosk and both models to be present:
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-deps.ps1
#
# Run from repo root:
#   powershell -ExecutionPolicy Bypass -File scripts\package.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$voskDir = "vendor\vosk-win64-0.3.45"

$packages = @(
    @{
        Lang      = "ru"
        Out       = "dist\mascot_ru"
        Zip       = "dist\mascot_ru.zip"
        ModelDir  = "models\vosk-model-small-ru-0.22"
        Config    = "voice.toml"
        Readme    = "README-user.txt"
        ReadmeOut = "README.txt"
    },
    @{
        Lang      = "en"
        Out       = "dist\mascot_en"
        Zip       = "dist\mascot_en.zip"
        ModelDir  = "models\vosk-model-small-en-us-0.15"
        Config    = "voice.en.toml"
        Readme    = "README-user.en.txt"
        ReadmeOut = "README.txt"
    }
)

if (-not (Test-Path "$voskDir\libvosk.dll")) {
    throw "Missing libvosk; run scripts\fetch-deps.ps1"
}

foreach ($pkg in $packages) {
    if (-not (Test-Path $pkg.ModelDir)) {
        throw "Missing $($pkg.Lang) model at $($pkg.ModelDir); run scripts\fetch-deps.ps1"
    }
    if (-not (Test-Path $pkg.Config)) {
        throw "Missing config $($pkg.Config)"
    }
}

Write-Host "1/4  cargo build --release ..."
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "Build failed" }

Write-Host "2/4  Preparing dist folders ..."
if (Test-Path "dist") { Remove-Item "dist" -Recurse -Force }

foreach ($pkg in $packages) {
    $out = $pkg.Out
    New-Item -ItemType Directory -Force -Path "$out\models" | Out-Null

    Copy-Item "target\release\ai_agent.exe" "$out\mascot.exe"
    Copy-Item "$voskDir\*.dll" $out
    Copy-Item $pkg.ModelDir "$out\models\" -Recurse
    Copy-Item "assets" "$out\assets" -Recurse
    Copy-Item $pkg.Config "$out\voice.toml"

    if (Test-Path $pkg.Readme) {
        Copy-Item $pkg.Readme "$out\$($pkg.ReadmeOut)"
    } elseif (Test-Path "README-user.txt") {
        Copy-Item "README-user.txt" "$out\README.txt"
    }
}

Write-Host "3/4  Creating zip files ..."
foreach ($pkg in $packages) {
    Compress-Archive -Path "$($pkg.Out)\*" -DestinationPath $pkg.Zip -Force
}

Write-Host "4/4  Done:"
foreach ($pkg in $packages) {
    $size = "{0:N1} MB" -f ((Get-Item $pkg.Zip).Length / 1MB)
    Write-Host "  $($pkg.Zip) ($size)"
}
Write-Host "Users can choose mascot_ru.zip or mascot_en.zip, unzip, and run mascot.exe."
