[CmdletBinding()]
param([string]$OutputDirectory)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
if(!$OutputDirectory){$OutputDirectory=Join-Path $root 'artifacts\AcerConsole'}
$cargo=Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
if(!(Test-Path -LiteralPath $cargo)){$cargo=(Get-Command cargo -ErrorAction Stop).Source}
Push-Location (Join-Path $root 'apps\desktop')
try { & npm.cmd ci; if($LASTEXITCODE -ne 0){throw 'npm ci failed'}; & npm.cmd run build; if($LASTEXITCODE -ne 0){throw 'Frontend build failed'} } finally {Pop-Location}
Push-Location $root
try {
    & $cargo test --workspace --locked
    if($LASTEXITCODE -ne 0){throw 'Workspace tests failed'}
    & $cargo build --release --locked -p acer-console -p acer-broker --features acer-console/custom-protocol
    if($LASTEXITCODE -ne 0){throw 'Release build failed'}
} finally {Pop-Location}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
foreach($name in @('acer-console.exe','acer-helper.exe')){Copy-Item -LiteralPath (Join-Path $root "target\release\$name") -Destination (Join-Path $OutputDirectory $name) -Force}
Copy-Item -LiteralPath (Join-Path $root 'docs\使用与恢复.md') -Destination (Join-Path $OutputDirectory '使用与恢复.md') -Force
Copy-Item -LiteralPath (Join-Path $root 'docs\目标与验收.md') -Destination (Join-Path $OutputDirectory '验收状态.md') -Force
$files=@(Get-ChildItem -LiteralPath $OutputDirectory -Filter '*.exe' | ForEach-Object {[ordered]@{name=$_.Name;bytes=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash}})
[ordered]@{built_at=(Get-Date).ToString('o');target='PHN16-71';bios='V1.18';files=$files;validation_note='Build success does not establish hardware acceptance; see project evidence records.'} | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $OutputDirectory 'build-manifest.json')
Write-Output $OutputDirectory
