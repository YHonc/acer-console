$ErrorActionPreference='Stop'
$OutputEncoding=[Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding=$OutputEncoding
$root=Split-Path -Parent $PSScriptRoot
$output=& (Join-Path $root 'target\debug\examples\inspect_locked_logo.exe')
$probeExit=$LASTEXITCODE
[IO.File]::WriteAllText((Join-Path $root 'evidence\boot-logo-locked-read.json'),($output -join "`n"),[Text.UTF8Encoding]::new($false))
exit $probeExit
