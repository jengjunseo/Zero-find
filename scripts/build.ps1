$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$localCargo = Join-Path $repoRoot '.tools\cargo\bin\cargo.exe'
$cargo = $localCargo

if (Test-Path -LiteralPath $localCargo) {
    $env:CARGO_HOME = Join-Path $repoRoot '.tools\cargo'
    $env:RUSTUP_HOME = Join-Path $repoRoot '.tools\rustup'
} else {
    $cargo = (Get-Command cargo -ErrorAction Stop).Source
}

& $cargo build --locked --manifest-path (Join-Path $repoRoot 'Cargo.toml') --release --target x86_64-pc-windows-gnu
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$dist = Join-Path $repoRoot 'dist'
New-Item -ItemType Directory -Force -Path $dist | Out-Null
Copy-Item -Force (Join-Path $repoRoot 'target\x86_64-pc-windows-gnu\release\zerofind.exe') (Join-Path $dist 'ZeroFind.exe')
Write-Host "Built: $dist\ZeroFind.exe"


