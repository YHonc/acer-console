[CmdletBinding()]
param(
    [ValidateRange(1,86400)][int]$DurationSeconds = 600,
    [ValidateRange(1,60)][int]$IntervalSeconds = 2,
    [ValidateSet('custom','official','replacement')][string]$Target = 'custom',
    [string]$ProcessNamePattern = '^(PredatorSense.*|AcerCentralService|AcerLightingService|AcerAgentService|AcerHardwareService)\.exe$',
    [int[]]$RootProcessId = @(),
    [string]$ServiceNamePattern = '^(AASSvc|AcerLightingService)$',
    [string]$Scenario = 'unspecified',
    [string]$OutputPath
)
$ErrorActionPreference = 'Stop'
if ($Target -ne 'custom') {
    if ($PSBoundParameters.ContainsKey('ProcessNamePattern') -or $RootProcessId.Count -gt 0) {
        throw 'Target presets cannot be combined with custom process selection.'
    }
    $retained = 'AcerCentralService|AcerLightingService|AcerAgentService|AcerHardwareService'
    $application = if ($Target -eq 'official') { 'PredatorSense.*' } else { 'acer-console|acer-helper' }
    $ProcessNamePattern = "^($application|$retained)\.exe$"
    $requiredApplication = if ($Target -eq 'official') { '^PredatorSense\.exe$' } else { '^acer-console\.exe$' }
    if (@(Get-CimInstance Win32_Process | Where-Object { $_.Name -match $requiredApplication }).Count -eq 0) {
        throw "The $Target application is not running; refusing to report a service-only sample as application usage."
    }
}
$logicalCpu = [int](Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors
if ($logicalCpu -lt 1) { throw 'Logical CPU count unavailable.' }
$known = @{}
$previous = @{}
$samples = [System.Collections.Generic.List[object]]::new()
$clock = [Diagnostics.Stopwatch]::StartNew()
$started = (Get-Date).ToString('o')
$collectionErrors = [System.Collections.Generic.List[string]]::new()
$previousTime = $null
do {
    $all = @(Get-CimInstance Win32_Process)
    $privateWorkingSets = @{}
    try {
        foreach ($counter in @(Get-CimInstance Win32_PerfRawData_PerfProc_Process)) {
            if ($counter.IDProcess -gt 0 -and $null -ne $counter.WorkingSetPrivate) {
                $privateWorkingSets[[int]$counter.IDProcess] = [long]$counter.WorkingSetPrivate
            }
        }
    } catch {
        $collectionErrors.Add('Private working set unavailable: ' + $_.Exception.Message)
    }
    $services = @(Get-CimInstance Win32_Service | Where-Object { $_.Name -match $ServiceNamePattern })
    $servicePids = @($services | Where-Object { $_.ProcessId -gt 0 } | ForEach-Object { [int]$_.ProcessId })
    $selected = @{}
    foreach ($p in $all) {
        $key = '{0}:{1}' -f $p.ProcessId, $p.CreationDate.ToUniversalTime().Ticks
        if ($p.Name -match $ProcessNamePattern -or ($null -eq $previousTime -and $RootProcessId -contains [int]$p.ProcessId) -or $servicePids -contains [int]$p.ProcessId -or $known.ContainsKey($key)) {
            $selected[[int]$p.ProcessId] = $key
        }
    }
    # Expand only while the observed parent's creation precedes its child.
    do {
        $added = $false
        foreach ($p in $all) {
            if (!$selected.ContainsKey([int]$p.ProcessId) -and $selected.ContainsKey([int]$p.ParentProcessId)) {
                $parent = $all | Where-Object { $_.ProcessId -eq $p.ParentProcessId } | Select-Object -First 1
                if ($parent.CreationDate -le $p.CreationDate) {
                    $selected[[int]$p.ProcessId] = '{0}:{1}' -f $p.ProcessId, $p.CreationDate.ToUniversalTime().Ticks
                    $added = $true
                }
            }
        }
    } while ($added)
    $time = $clock.Elapsed.TotalSeconds
    $rows = @(); $next = @{}; $cpuSum = 0.0; $validCpu = 0
    foreach ($p in $all) {
        $id = [int]$p.ProcessId
        if (!$selected.ContainsKey($id)) { continue }
        $key = $selected[$id]; $known[$key] = $true
        $cumulative = ([double]$p.KernelModeTime + [double]$p.UserModeTime) / 10000000.0
        $cpu = $null
        if ($previous.ContainsKey($key) -and $null -ne $previousTime -and $time -gt $previousTime) {
            $delta = $cumulative - $previous[$key]
            if ($delta -ge 0) { $cpu = 100.0 * $delta / ($time - $previousTime) / $logicalCpu; $cpuSum += $cpu; $validCpu++ }
        }
        $next[$key] = $cumulative
        $rows += [pscustomobject][ordered]@{
            Identity = $key; ProcessId = $id; ParentProcessId = [int]$p.ParentProcessId
            Name = $p.Name; ExecutablePath = $p.ExecutablePath
            Group = $(if ($servicePids -contains $id) { 'retained-service' } else { 'application-or-descendant' })
            CpuSeconds = $cumulative; CpuPercentMachine = $cpu
            WorkingSetBytes = [long]$p.WorkingSetSize; PrivateCommitBytes = [long]$p.PrivatePageCount
            PrivateWorkingSetBytes = $(if ($privateWorkingSets.ContainsKey($id)) { $privateWorkingSets[$id] } else { $null })
        }
    }
    $samples.Add([ordered]@{
        ElapsedSeconds = $time; At = (Get-Date).ToString('o'); Processes = @($rows)
        CpuPercentMachine = $(if ($validCpu -gt 0) { $cpuSum } else { $null })
        CpuMatchedProcessCount = $validCpu; ProcessCount = $rows.Count
        WorkingSetBytes = [long](($rows | Measure-Object WorkingSetBytes -Sum).Sum)
        PrivateCommitBytes = [long](($rows | Measure-Object PrivateCommitBytes -Sum).Sum)
        PrivateWorkingSetBytes = $(if (@($rows | Where-Object { $null -eq $_.PrivateWorkingSetBytes }).Count -eq 0) { [long](($rows | Measure-Object PrivateWorkingSetBytes -Sum).Sum) } else { $null })
        PrivateWorkingSetMatchedProcessCount = @($rows | Where-Object { $null -ne $_.PrivateWorkingSetBytes }).Count
        Services = @($services | Select-Object Name,State,StartMode,ProcessId)
    })
    $previous = $next; $previousTime = $time
    $remaining = $DurationSeconds - $clock.Elapsed.TotalSeconds
    if ($remaining -le 0) { break }
    Start-Sleep -Milliseconds ([int]([Math]::Min($IntervalSeconds, $remaining) * 1000))
} while ($true)
function Get-Stats($values) {
    $v = @($values | Where-Object { $null -ne $_ } | Sort-Object)
    if ($v.Count -eq 0) { return $null }
    [ordered]@{ Count = $v.Count; Mean = ($v | Measure-Object -Average).Average; P95 = $v[[Math]::Max(0,[Math]::Ceiling($v.Count * 0.95)-1)]; Peak = $v[-1] }
}
$weightedCpu = 0.0; $validSeconds = 0.0
for ($i = 1; $i -lt $samples.Count; $i++) {
    if ($null -ne $samples[$i].CpuPercentMachine) {
        $dt = $samples[$i].ElapsedSeconds - $samples[$i-1].ElapsedSeconds
        $weightedCpu += $samples[$i].CpuPercentMachine * $dt; $validSeconds += $dt
    }
}
$report = [ordered]@{
    SchemaVersion = 1; StartedAt = $started; Scenario = $Scenario; Target = $Target; LogicalProcessors = $logicalCpu
    DurationSeconds = $clock.Elapsed.TotalSeconds
    Selection = [ordered]@{ ProcessNamePattern = $ProcessNamePattern; RootProcessId = $RootProcessId; ServiceNamePattern = $ServiceNamePattern }
    Metrics = [ordered]@{
        Cpu = '100 * delta(user+kernel seconds) / observed elapsed seconds / logical processors. Matched process lifetimes only; exited between samples are not captured.'
        WorkingSetBytes = 'Resident working set including shared pages; summed shared pages can be double-counted.'
        PrivateCommitBytes = 'Win32_Process.PrivatePageCount: private committed virtual memory, not private resident working set.'
        PrivateWorkingSetBytes = 'Win32_PerfRawData_PerfProc_Process.WorkingSetPrivate: private resident bytes. Separate CIM query joined by PID; total is null if any selected process lacks a counter.'
        Summary = 'Memory/P95 are sample-based; CPU weighted mean is elapsed-time weighted over valid intervals. First sample CPU is null.'
    }
    Summary = [ordered]@{
        CpuPercentMachine = Get-Stats @($samples | ForEach-Object { $_.CpuPercentMachine })
        CpuTimeWeightedMean = $(if ($validSeconds -gt 0) { $weightedCpu / $validSeconds } else { $null })
        WorkingSetBytes = Get-Stats @($samples | ForEach-Object { $_.WorkingSetBytes })
        PrivateCommitBytes = Get-Stats @($samples | ForEach-Object { $_.PrivateCommitBytes })
        PrivateWorkingSetBytes = Get-Stats @($samples | ForEach-Object { $_.PrivateWorkingSetBytes })
    }
    Samples = $samples.ToArray()
    CollectionErrors = @($collectionErrors | Select-Object -Unique)
}
$json = $report | ConvertTo-Json -Depth 10
if ($OutputPath) { [IO.File]::WriteAllText($ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputPath), $json, [Text.UTF8Encoding]::new($false)) }
$json
