$ErrorActionPreference = 'Stop'
$report = [ordered]@{
    CollectedAt = (Get-Date).ToString('o')
    Computer = Get-CimInstance Win32_ComputerSystem | Select-Object Manufacturer,Model,TotalPhysicalMemory
    CPU = Get-CimInstance Win32_Processor | Select-Object Name
    GPU = @(Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion)
    BIOS = Get-CimInstance Win32_BIOS | Select-Object SMBIOSBIOSVersion
    OS = Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version
    Console = @(Get-AppxPackage '*Predator*' | Select-Object Name,Version,InstallLocation)
    Interfaces = @(Get-CimClass -Namespace root/WMI -ClassName '*Acer*' | Select-Object CimClassName,@{n='Methods';e={@($_.CimClassMethods.Name)}})
}
$report | ConvertTo-Json -Depth 8
