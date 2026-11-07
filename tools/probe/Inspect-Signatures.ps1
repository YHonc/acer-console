param([string]$OutputPath = (Join-Path $PSScriptRoot 'signatures.local.json'))
$ErrorActionPreference = 'Stop'
# Metadata only. No instance methods, firmware writes, service or startup changes.
$result = foreach ($className in 'AcerGamingFunction','BatteryControl') {
    try {
        $class = Get-CimClass -Namespace root/wmi -ClassName $className
        foreach ($method in $class.CimClassMethods) {
            [pscustomobject]@{
                Class = $className
                Method = $method.Name
                ReturnType = [string]$method.ReturnType
                Parameters = @($method.Parameters | ForEach-Object {
                    [pscustomobject]@{ Name=$_.Name; Type=[string]$_.CimType; Qualifiers=@($_.Qualifiers | ForEach-Object { @{Name=$_.Name;Value=$_.Value} }) }
                })
            }
        }
    } catch { [pscustomobject]@{Class=$className;Error=$_.Exception.Message} }
}
$result | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $OutputPath -Encoding utf8
Write-Output $OutputPath
