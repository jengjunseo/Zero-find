$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$env:CARGO_HOME = Join-Path $repoRoot '.tools\cargo'
$env:RUSTUP_HOME = Join-Path $repoRoot '.tools\rustup'
$cargo = Join-Path $env:CARGO_HOME 'bin\cargo.exe'

if (-not (Test-Path -LiteralPath $cargo)) {
    throw 'Rust is not installed in .tools. Install a stable x86_64-pc-windows-gnu toolchain first.'
}

& $cargo build --release --target x86_64-pc-windows-gnu
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$dist = Join-Path $repoRoot 'dist'
New-Item -ItemType Directory -Force -Path $dist | Out-Null
Copy-Item -Force (Join-Path $repoRoot 'target\x86_64-pc-windows-gnu\release\zerofind.exe') (Join-Path $dist 'ZeroFind.exe')
Write-Host "Built: $dist\ZeroFind.exe"


