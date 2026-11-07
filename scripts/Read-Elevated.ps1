$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$probe = Join-Path $root 'target\debug\acer-probe.exe'
$output = Join-Path $root 'evidence\rust-probe-elevated.json'
& $probe | Set-Content -Encoding UTF8 -LiteralPath $output
if ($LASTEXITCODE -ne 0) { throw "Read-only probe failed: $LASTEXITCODE" }
& $probe --capabilities | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $root 'evidence\rust-capabilities-elevated.json')
if ($LASTEXITCODE -ne 0) { throw "Capability probe failed: $LASTEXITCODE" }
