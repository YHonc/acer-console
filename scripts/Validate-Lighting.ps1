$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
& (Join-Path $root 'target\debug\acer-probe.exe') --validate-lighting-brightness-restore | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $root 'evidence\lighting-brightness-restore.json')
exit $LASTEXITCODE
