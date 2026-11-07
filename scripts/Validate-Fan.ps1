$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
& (Join-Path $root 'target\debug\acer-probe.exe') --validate-fan-max-restore | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $root 'evidence\fan-max-restore.json')
exit $LASTEXITCODE
