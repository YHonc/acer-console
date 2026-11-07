$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
& (Join-Path $root 'target\debug\acer-probe.exe') --validate-lighting-verified-restore | Set-Content -LiteralPath (Join-Path $root 'evidence\lighting-verified-path-restore.json') -Encoding UTF8
exit $LASTEXITCODE
