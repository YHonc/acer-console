[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
param(
    [ValidateSet('Inspect','Backup','Disable','Restore')][string]$Action = 'Inspect',
    [string]$TaskName = 'PredatorSenseLauncher',
    [string]$TaskPath = '\',
    [string]$BackupDirectory,
    [switch]$Apply
)
$ErrorActionPreference = 'Stop'
function Get-DefinitionHash([string]$xml) {
    [xml]$doc = $xml
    # Only the task-level enabled bit may differ during a reversible disable.
    $ns = [Xml.XmlNamespaceManager]::new($doc.NameTable)
    $ns.AddNamespace('t', $doc.DocumentElement.NamespaceURI)
    $node = $doc.SelectSingleNode('/t:Task/t:Settings/t:Enabled', $ns)
    if ($node) { $node.ParentNode.RemoveChild($node) | Out-Null }
    $sha = [Security.Cryptography.SHA256]::Create()
    try { ([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($doc.OuterXml)))).Replace('-','') } finally { $sha.Dispose() }
}
$task = Get-ScheduledTask -TaskName $TaskName -TaskPath $TaskPath -ErrorAction Stop
$xml = Export-ScheduledTask -TaskName $TaskName -TaskPath $TaskPath
$enabled = [bool]$task.Settings.Enabled
$result = [ordered]@{
    TaskName = $TaskName; TaskPath = $TaskPath; State = [string]$task.State; Enabled = $enabled
    Action = $Action; Applied = $false
    Triggers = @($task.Triggers | Select-Object @{n='Type';e={$_.CimClass.CimClassName}},Enabled,Delay,UserId,StartBoundary)
    Actions = @($task.Actions | Select-Object Execute,Arguments,WorkingDirectory)
}
if ($Action -eq 'Inspect') { $result | ConvertTo-Json -Depth 6; return }
if (!$BackupDirectory) { throw 'BackupDirectory is required for Backup/Disable/Restore, including preview.' }
$directory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($BackupDirectory)
$manifestPath = Join-Path $directory 'manifest.json'
$xmlPath = Join-Path $directory 'task.xml'
if ($Action -eq 'Restore') {
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ($manifest.TaskName -cne $TaskName -or $manifest.TaskPath -cne $TaskPath) { throw 'Backup task identity mismatch.' }
    $savedXml = [IO.File]::ReadAllText($xmlPath)
    if ((Get-DefinitionHash $savedXml) -ne $manifest.DefinitionHash -or (Get-DefinitionHash $xml) -ne $manifest.DefinitionHash) { throw 'Task definition changed or backup corrupted; review task.xml manually.' }
    $result['RestoreEnabled'] = [bool]$manifest.Enabled
} elseif (Test-Path -LiteralPath $directory) {
    throw 'Use a new BackupDirectory; existing backups are never overwritten.'
}
if (!$Apply) { $result['Preview'] = 'Pass -Apply to write backup or change the enabled state. No task/service has been changed.'; $result | ConvertTo-Json -Depth 6; return }
if ($PSCmdlet.ShouldProcess("$TaskPath$TaskName", "$Action (backup: $directory)")) {
    if ($Action -ne 'Restore') {
        New-Item -ItemType Directory -Path $directory -ErrorAction Stop | Out-Null
        [IO.File]::WriteAllText($xmlPath, $xml, [Text.UTF8Encoding]::new($false))
        [ordered]@{ SchemaVersion = 1; SavedAt = (Get-Date).ToString('o'); TaskName = $TaskName; TaskPath = $TaskPath; Enabled = $enabled; DefinitionHash = Get-DefinitionHash $xml } | ConvertTo-Json | Set-Content -LiteralPath $manifestPath -Encoding UTF8
    }
    if ($Action -eq 'Disable') { Disable-ScheduledTask -TaskName $TaskName -TaskPath $TaskPath | Out-Null }
    if ($Action -eq 'Restore') {
        if ($manifest.Enabled) { Enable-ScheduledTask -TaskName $TaskName -TaskPath $TaskPath | Out-Null }
        else { Disable-ScheduledTask -TaskName $TaskName -TaskPath $TaskPath | Out-Null }
    }
    $result['Applied'] = $true
    $result['EnabledAfter'] = [bool](Get-ScheduledTask -TaskName $TaskName -TaskPath $TaskPath).Settings.Enabled
}
$result | ConvertTo-Json -Depth 6
