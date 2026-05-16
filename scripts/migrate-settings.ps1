param(
    [string]$OldResources = "C:\Users\lcy\Desktop\win-unpacked\resources",
    [string]$ProjectRoot = (Resolve-Path "$PSScriptRoot\..").Path
)

$ErrorActionPreference = "Stop"
$dataDir = Join-Path $ProjectRoot "data"
New-Item -ItemType Directory -Force -Path $dataDir | Out-Null

$poetrySrc = Join-Path $OldResources "poetry.db"
if (Test-Path -LiteralPath $poetrySrc) {
    Copy-Item -LiteralPath $poetrySrc -Destination (Join-Path $dataDir "poetry.db") -Force
    Write-Host "Copied poetry.db"
} else {
    Write-Warning "poetry.db not found: $poetrySrc"
}

$settingsSrc = Join-Path $OldResources "settings.json"
if (Test-Path -LiteralPath $settingsSrc) {
    Copy-Item -LiteralPath $settingsSrc -Destination (Join-Path $dataDir "settings.imported.json") -Force
    Write-Host "Copied settings.imported.json"
} else {
    Write-Warning "settings.json not found: $settingsSrc"
}
