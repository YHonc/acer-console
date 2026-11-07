param([Parameter(Mandatory=$true)][ValidateSet('lcd_overdrive','boot_sound','backlight_timeout')][string]$Setting)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$output = & (Join-Path $root 'target\debug\examples\direct_settings_probe.exe') --validate-restore $Setting
$probeExit=$LASTEXITCODE
[IO.File]::WriteAllText((Join-Path $root "evidence\direct-$Setting-restore.json"), ($output -join "`n"), [Text.UTF8Encoding]::new($false))
exit $probeExit
