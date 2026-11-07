$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
& (Join-Path $root 'target\debug\acer-probe.exe') --validate-battery-care-restore | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $root 'evidence\battery-care-restore.json')
exit $LASTEXITCODE
