$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
& (Join-Path $root 'target\debug\acer-probe.exe') --validate-fan-custom-restore | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $root 'evidence\fan-custom-restore.json')
exit $LASTEXITCODE
