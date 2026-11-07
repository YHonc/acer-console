[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$InputPath,[string]$OutputPath)
$ErrorActionPreference='Stop'
$report=Get-Content -LiteralPath $InputPath -Raw | ConvertFrom-Json
if($report.SchemaVersion -ne 1){throw 'Unsupported resource sample schema'}
$knownGroups=@{}
$rows=foreach($sample in $report.Samples){
    $groups=@{}
    foreach($process in $sample.Processes){
        $group=if($process.Name -in @('acer-console.exe','acer-helper.exe')){'replacement'}elseif($process.Name -eq 'PredatorSense.exe'){'official-ui'}elseif($process.Group -eq 'retained-service' -or $process.Name -in @('AcerAgentService.exe','AcerHardwareService.exe')){'retained-services'}elseif($knownGroups.ContainsKey($process.Identity)){$knownGroups[$process.Identity]}else{$null}
        if($group){$groups[[int]$process.ProcessId]=$group}
    }
    do{
        $changed=$false
        foreach($process in $sample.Processes){if(!$groups.ContainsKey([int]$process.ProcessId) -and $groups.ContainsKey([int]$process.ParentProcessId)){$groups[[int]$process.ProcessId]=$groups[[int]$process.ParentProcessId];$changed=$true}}
    }while($changed)
    foreach($process in $sample.Processes){if($groups.ContainsKey([int]$process.ProcessId)){$knownGroups[$process.Identity]=$groups[[int]$process.ProcessId]}}
    foreach($group in @('replacement','official-ui','retained-services','unclassified')){
        $members=@($sample.Processes | Where-Object {if($group -eq 'unclassified'){!$groups.ContainsKey([int]$_.ProcessId)}else{$groups[[int]$_.ProcessId] -eq $group}})
        $missingMemory=@($members | Where-Object {$null -eq $_.PrivateWorkingSetBytes}).Count
        $missingCpu=@($members | Where-Object {$null -eq $_.CpuPercentMachine}).Count
        [pscustomobject]@{Group=$group;Elapsed=[double]$sample.ElapsedSeconds;Count=$members.Count;Names=@($members.Name | Sort-Object -Unique);PrivateMiB=$(if($missingMemory){$null}else{[double](($members | Measure-Object PrivateWorkingSetBytes -Sum).Sum)/1MB});Cpu=$(if($missingCpu){$null}else{[double](($members | Measure-Object CpuPercentMachine -Sum).Sum)});MissingMemory=$missingMemory;MissingCpu=$missingCpu}
    }
}
$summary=foreach($group in @('replacement','official-ui','retained-services','unclassified')){
    $values=@($rows | Where-Object Group -eq $group);$memory=@($values | Where-Object {$null -ne $_.PrivateMiB})
    $weighted=0.0;$seconds=0.0
    for($i=1;$i -lt $values.Count;$i++){if($null -ne $values[$i].Cpu){$dt=$values[$i].Elapsed-$values[$i-1].Elapsed;$weighted+=$values[$i].Cpu*$dt;$seconds+=$dt}}
    [ordered]@{group=$group;private_mib_mean=$(if($memory.Count){($memory | Measure-Object PrivateMiB -Average).Average}else{$null});private_mib_peak=$(if($memory.Count){($memory | Measure-Object PrivateMiB -Maximum).Maximum}else{$null});cpu_percent_time_weighted_mean=$(if($seconds -gt 0){$weighted/$seconds}else{$null});cpu_valid_seconds=$seconds;process_count_peak=($values | Measure-Object Count -Maximum).Maximum;names=@($values.Names | Sort-Object -Unique);memory_incomplete_samples=@($values | Where-Object MissingMemory -gt 0).Count;cpu_incomplete_samples=@($values | Where-Object MissingCpu -gt 0).Count}
}
$result=[ordered]@{source=(Resolve-Path -LiteralPath $InputPath).Path;scenario=$report.Scenario;duration_seconds=$report.DurationSeconds;groups=@($summary);collection_errors=$report.CollectionErrors;note='Observed scenarios only. UI visibility and functional coverage must be matched before claiming savings. Retained services are separate, not omitted; private working set is resident private memory.'}
$json=$result | ConvertTo-Json -Depth 6
if($OutputPath){[IO.File]::WriteAllText($ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputPath),$json,[Text.UTF8Encoding]::new($false))}
$json
