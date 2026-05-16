param(
    [string]$ProjectRoot = (Resolve-Path "$PSScriptRoot\..").Path
)

$ErrorActionPreference = "Stop"
Push-Location $ProjectRoot
try {
    cargo fmt --all
    cargo test --workspace
    Push-Location app
    try {
        npm install
        npm run build
    } finally {
        Pop-Location
    }
} finally {
    Pop-Location
}
