$ErrorActionPreference='Stop'
$OutputEncoding=[Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding=$OutputEncoding
$root=Split-Path -Parent $PSScriptRoot
& (Join-Path $root 'target\debug\examples\inspect_logo.exe') | Set-Content -LiteralPath (Join-Path $root 'evidence\boot-logo-readonly-discovery.json') -Encoding UTF8
exit $LASTEXITCODE
