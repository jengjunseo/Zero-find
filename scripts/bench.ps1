$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$env:CARGO_HOME = Join-Path $repoRoot '.tools\cargo'
$env:RUSTUP_HOME = Join-Path $repoRoot '.tools\rustup'
$cargo = Join-Path $env:CARGO_HOME 'bin\cargo.exe'

& $cargo run --release --target x86_64-pc-windows-gnu --bin spike_a -- C:
& $cargo run --release --target x86_64-pc-windows-gnu --bin spike_b
& $cargo run --release --target x86_64-pc-windows-gnu --bin spike_c


