param([ValidateSet('switch','10','20','30')][string]$Mode='switch')
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$probe=Join-Path $root 'target\debug\examples\usb_probe.exe'
if($Mode -eq 'switch'){$result=& $probe --validate-switch-restore}else{$result=& $probe --validate-threshold-restore $Mode}
$code=$LASTEXITCODE
$result | Set-Content -LiteralPath (Join-Path $root "evidence\usb-charge-verified-$Mode-restore.json") -Encoding UTF8
exit $code
