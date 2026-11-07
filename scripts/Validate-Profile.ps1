$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
& (Join-Path $root 'target\debug\acer-probe.exe') --validate-profile-balanced-restore | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $root 'evidence\profile-balanced-restore.json')
exit $LASTEXITCODE
