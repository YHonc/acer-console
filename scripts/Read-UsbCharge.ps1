$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
& (Join-Path $root 'target\debug\examples\usb_probe.exe') | Set-Content -LiteralPath (Join-Path $root 'evidence\usb-charge-read.json') -Encoding UTF8
exit $LASTEXITCODE
