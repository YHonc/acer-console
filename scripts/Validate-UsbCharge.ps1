$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
& (Join-Path $root 'target\debug\examples\usb_probe.exe') --validate-switch-restore | Set-Content -LiteralPath (Join-Path $root 'evidence\usb-charge-switch-restore.json') -Encoding UTF8
exit $LASTEXITCODE
