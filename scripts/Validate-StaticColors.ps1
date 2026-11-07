$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
& (Join-Path $root 'target\debug\acer-probe.exe') --validate-lighting-static-colors-restore | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $root 'evidence\lighting-static-colors-restore.json')
exit $LASTEXITCODE
