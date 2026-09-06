$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
& (Join-Path $PSScriptRoot 'build.ps1')
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$artifacts = Join-Path $repoRoot 'artifacts'
$fixture = Join-Path $artifacts ('cert-fixture-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $fixture | Out-Null
1..30 | ForEach-Object { Set-Content -LiteralPath (Join-Path $fixture ('보고서 {0:D2}.txt' -f $_)) -Value 'ZeroFind certification fixture' }
1..3 | ForEach-Object { Set-Content -LiteralPath (Join-Path $fixture ('가우스 {0}.txt' -f $_)) -Value 'ZeroFind certification fixture' }
Set-Content -LiteralPath (Join-Path $fixture '배포자료.txt') -Value 'ZeroFind certification fixture'
$oldRoots = $env:ZEROFIND_SCAN_ROOTS
$oldReport = $env:ZEROFIND_CERT_REPORT
try {
    $env:ZEROFIND_SCAN_ROOTS = $fixture
    $env:ZEROFIND_CERT_REPORT = Join-Path $artifacts 'executable-certification.txt'
    $process = Start-Process -FilePath (Join-Path $repoRoot 'dist\ZeroFind.exe') -ArgumentList '--certify' -PassThru -WindowStyle Hidden
    if (-not $process.WaitForExit(60000)) {
        $process.Kill()
        throw 'Executable certification timed out.'
    }
    if ($process.ExitCode -ne 0) { throw "Executable exited with code $($process.ExitCode)." }
    $report = Get-Content -LiteralPath $env:ZEROFIND_CERT_REPORT -Raw
    Write-Output $report
    if ($report -match 'FAIL' -or $report -notmatch 'PASS executable_certification_complete') {
        throw 'Executable certification failed.'
    }
} finally {
    $env:ZEROFIND_SCAN_ROOTS = $oldRoots
    $env:ZEROFIND_CERT_REPORT = $oldReport
}
# Keep the isolated fixture for reproduction; never clean unrelated user files.
