param([Parameter(Mandatory=$true)][ValidateRange(1,7)][int]$Effect)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$output=Join-Path $root "evidence\lighting-effect-$Effect-restore.json"
& (Join-Path $root 'target\debug\acer-probe.exe') --validate-lighting-effect $Effect --restore | Set-Content -Encoding UTF8 -LiteralPath $output
exit $LASTEXITCODE
