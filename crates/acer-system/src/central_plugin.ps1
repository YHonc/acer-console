$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$errors=[Collections.Generic.List[string]]::new()
function Get-BinaryHash([string]$path) {
 $stream=[IO.File]::OpenRead($path)
 $hasher=[Security.Cryptography.SHA256]::Create()
 try {return ([BitConverter]::ToString($hasher.ComputeHash($stream))).Replace('-','')} finally {$hasher.Dispose();$stream.Dispose()}
}
$guardExe=Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'AcerHandleGuard\acer-handle-guard.exe'
$guardHash='FE4A659E0DEFA20911AD103042C339B4471F549D5A0116853701913F98233296'
$patchHash='3CAFE4566B4921BAA8185EB5FA110B4482C4B355419B7EC14BDC42083E35E8EE'
$originalHash='5BB16FA594878F7014BC61E2A7B2A6C6B640F0CE0E6A1BF1E18E440C3DD4C9F3'
$stateDir=Join-Path ([Environment]::GetFolderPath('CommonApplicationData')) 'AcerHandleGuard'
$expectedArgs='--apply --threshold 12000 --cooldown 600 --output "'+(Join-Path $stateDir 'last-check.txt')+'"'
$guard=[ordered]@{installed=$null;enabled=$null;state=$null;verified=$false;last_run=$null;next_run=$null;last_result=$null;threshold=12000;cooldown_seconds=600;error=$null}
$task=$null
try {
 $scheduler=New-Object -ComObject Schedule.Service
 $scheduler.Connect()
 $folder=$scheduler.GetFolder('\')
 try {$task=$folder.GetTask('AcerHandleGuard')} catch {
  if ($_.Exception.HResult -eq -2147024894) {$guard.installed=$false} else {throw}
 }
 if ($task) {
  $guard.installed=$true
  $definition=$task.Definition
  $action=$definition.Actions.Item(1)
  $valid=$definition.Actions.Count -eq 1 -and $action.Type -eq 0 -and [string]::Equals($action.Path,$guardExe,[StringComparison]::OrdinalIgnoreCase) -and $action.Arguments -ceq $expectedArgs
  if ($valid) {$valid=(Get-BinaryHash $guardExe) -eq $guardHash}
  $guard.verified=[bool]$valid
  if ($guardAction -ne 'query') {
   if (!$valid) {throw 'Guard task or binary differs from verified installation; no change made.'}
   $task.Enabled=($guardAction -eq 'enable')
   $task=$folder.GetTask('AcerHandleGuard')
   if ($task.Enabled -ne ($guardAction -eq 'enable')) {throw 'Guard state readback mismatch.'}
  }
  $guard.enabled=[bool]$task.Enabled
  $guard.state=[int]$task.State
  $guard.last_run=$task.LastRunTime.ToString('o')
  $guard.next_run=$task.NextRunTime.ToString('o')
  $guard.last_result=[long]$task.LastTaskResult
 }
 if (!$task -and $guardAction -ne 'query') {throw 'AcerHandleGuard is not installed.'}
} catch {
 if ($guardAction -ne 'query') {throw}
 $guard.error=$_.Exception.Message
}
$service=$null
$metrics=$null
try {
 $svc=Get-CimInstance Win32_Service -Filter "Name='AASSvc'"
 if ($svc) {
  $binary=$svc.PathName.Trim('"')
  $patch='unknown';$hash=$null
  try {
   $hash=Get-BinaryHash $binary
   if ($hash -eq $patchHash) {$patch='patched'} elseif ($hash -eq $originalHash) {$patch='original'}
  } catch {$errors.Add('Service binary check: '+$_.Exception.Message)}
  $service=[ordered]@{state=$svc.State;start_mode=$svc.StartMode;pid=[long]$svc.ProcessId;path=$svc.PathName;patch=$patch;sha256=$hash}
  if ($svc.ProcessId -gt 0) {
   $p=Get-CimInstance Win32_Process -Filter ('ProcessId='+[long]$svc.ProcessId)
   if (!$p -or !$p.CreationDate -or $null -eq $p.KernelModeTime -or $null -eq $p.UserModeTime) {throw 'Process counters unavailable; authorize and refresh.'}
   $metrics=[ordered]@{pid=[long]$p.ProcessId;started_at=$p.CreationDate.ToUniversalTime().ToString('o');cpu_seconds=([double]$p.KernelModeTime+[double]$p.UserModeTime)/10000000;working_set_bytes=[long]$p.WorkingSetSize;private_commit_bytes=[long]$p.PrivatePageCount;handles=[long]$p.HandleCount;threads=[long]$p.ThreadCount}
  }
 }
} catch {$errors.Add('Service metrics: '+$_.Exception.Message)}
[ordered]@{captured_at_ms=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds();logical_processors=[Environment]::ProcessorCount;service=$service;metrics=$metrics;guard=$guard;errors=@($errors)} | ConvertTo-Json -Depth 6 -Compress
