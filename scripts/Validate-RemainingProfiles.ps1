$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$probe=Join-Path $root 'target\debug\acer-probe.exe'
foreach($mode in @('quiet','turbo')) {
    $file=Join-Path $root "evidence\profile-$mode-restore.json"
    & $probe --validate-profile $mode --restore | Set-Content -Encoding UTF8 -LiteralPath $file
    if($LASTEXITCODE -ne 0){throw "Profile validation did not confirm restore: $mode"}
    $report=Get-Content -LiteralPath $file -Raw | ConvertFrom-Json
    if($report.outcome -ne 'readbacks_confirmed' -or !$report.restore_readback_confirmed){throw "Unconfirmed profile outcome: $mode"}
}
