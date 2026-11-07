$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$output = & (Join-Path $root 'target\debug\examples\direct_settings_probe.exe') --read
$probeExit = $LASTEXITCODE
[IO.File]::WriteAllText((Join-Path $root 'evidence\direct-settings-read.json'), ($output -join "`n"), [Text.UTF8Encoding]::new($false))
exit $probeExit
