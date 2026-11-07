$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$key=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Run',$false)
$name='AcerConsole.DeviceCenter'
$owned=$null;$other=@()
try {if($key){$owned=$key.GetValue($name,$null);if($boundExe){$other=@($key.GetValueNames() | Where-Object {$_ -ne $name -and ([string]$key.GetValue($_)).IndexOf($boundExe,[StringComparison]::OrdinalIgnoreCase) -ge 0})}}} finally {if($key){$key.Dispose()}}
$expected='"'+$boundExe+'"'
$all=@(Get-CimInstance Win32_Process)
$roots=@($all | Where-Object {$boundExe -and $_.ExecutablePath -and [string]::Equals($_.ExecutablePath,$boundExe,[StringComparison]::OrdinalIgnoreCase)})
$message=$null
if ($dcAction -eq 'launch') {
 if($roots.Count -gt 0){$message='设备中心已经运行；请从它的托盘打开窗口。未重复启动。'} else {
  $process=Start-Process -FilePath $boundExe -WorkingDirectory (Split-Path -Parent $boundExe) -PassThru
  $message='启动请求已提交，尚未确认窗口和设备连接。'
  $process.Dispose()
 }
} elseif($dcAction -eq 'enable_startup' -or $dcAction -eq 'disable_startup') {
 if($owned -and $owned -cne $expected){throw '本控制台启动项内容已改变，请先核对，不覆盖或删除。'}
 if($dcAction -eq 'enable_startup' -and $other.Count -gt 0){throw '原应用已有自启动项，请在原应用管理，避免重复启动。'}
 $write=[Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\Microsoft\Windows\CurrentVersion\Run')
 try {
  if($dcAction -eq 'enable_startup') {$write.SetValue($name,$expected,[Microsoft.Win32.RegistryValueKind]::String)} else {$write.DeleteValue($name,$false)}
  $owned=$write.GetValue($name,$null)
  if(($dcAction -eq 'enable_startup' -and $owned -cne $expected) -or ($dcAction -eq 'disable_startup' -and $null -ne $owned)){throw '自启动读回不一致'}
  $message='本控制台管理的自启动状态已读回确认。'
 } finally {$write.Dispose()}
}
$selected=@{}
foreach($root in $roots){$selected[[long]$root.ProcessId]=$root}
do {$added=$false;foreach($process in $all){
 if(!$selected.ContainsKey([long]$process.ProcessId) -and $selected.ContainsKey([long]$process.ParentProcessId)){
  $parent=$selected[[long]$process.ParentProcessId]
  if($parent.CreationDate -and $process.CreationDate -and $parent.CreationDate -le $process.CreationDate){$selected[[long]$process.ProcessId]=$process;$added=$true}
 }
}}while($added)
$processes=@($selected.Values | Sort-Object ProcessId | ForEach-Object {
 [ordered]@{pid=[long]$_.ProcessId;name=$_.Name;created=if($_.CreationDate){$_.CreationDate.ToUniversalTime().ToString('o')}else{$null};cpu_seconds=if($null -ne $_.KernelModeTime -and $null -ne $_.UserModeTime){([double]$_.KernelModeTime+[double]$_.UserModeTime)/10000000}else{$null};working_set_bytes=[long]$_.WorkingSetSize;private_commit_bytes=[long]$_.PrivatePageCount;handles=[long]$_.HandleCount;threads=[long]$_.ThreadCount}
})
[ordered]@{captured_at_ms=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds();logical_processors=[Environment]::ProcessorCount;running=($roots.Count -gt 0);processes=$processes;startup=[ordered]@{present=($null -ne $owned);enabled=($boundExe -and $owned -ceq $expected);mismatch=($null -ne $owned -and $owned -cne $expected);other_entries=$other};message=$message} | ConvertTo-Json -Depth 6 -Compress
