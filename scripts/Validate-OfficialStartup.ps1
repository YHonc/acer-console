$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$backup=Join-Path $root ('evidence\official-task-roundtrip-'+(Get-Date -Format 'yyyyMMdd-HHmmss'))
& (Join-Path $root 'target\debug\examples\official_task_roundtrip.exe') --validate $backup | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $root 'evidence\official-task-roundtrip.json')
exit $LASTEXITCODE
