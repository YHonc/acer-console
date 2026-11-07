<#
.SYNOPSIS
  One command for a user test package: console + Device Center module + docs + manifest + zip.

.DESCRIPTION
  artifacts/AcerConsole-<Name>/ and artifacts/AcerConsole-<Name>.zip
  1. Build-Release.ps1 (frontend build, workspace tests, release exe).
  2. Device Center: GNU toolchain test + release build. Skipped when the cached
     build already matches the source commit (marker file in the target dir).
  3. Package-DeviceCenterModule.ps1 into modules/device-center.
  4. User docs, build-manifest.json, zip, zip self-check, summary.
  Builds and packages only: launches no app and touches no hardware.

.EXAMPLE
  scripts/ps.sh scripts/Build-TestPackage.ps1 -Name DCIPC-20261108
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Name,
    [string]$DeviceCenterSource = 'D:\ChatGpt\DeviceCenter-dc-ipc-ui-integ',
    [string]$DeviceCenterTools = 'D:\ChatGpt\DeviceCenter\.tools',
    [string]$DeviceCenterTarget = (Join-Path $env:TEMP 'device-center-gnu-target'),
    [switch]$RebuildDeviceCenter
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

$root = Split-Path -Parent $PSScriptRoot
$out = Join-Path $root "artifacts\AcerConsole-$Name"
$zip = "$out.zip"
function Step($text) { Write-Output "`n== $text" }
function GitIn([string]$dir) { $r = & git.exe -C $dir @args; if ($LASTEXITCODE) { throw "git $args failed in $dir" }; $r }

# --- preflight -------------------------------------------------------------
if (Test-Path -LiteralPath $out) { throw "Output exists, choose a new -Name: $out" }
if (Test-Path -LiteralPath $zip) { throw "Zip exists, choose a new -Name: $zip" }
foreach ($p in @($DeviceCenterSource, (Join-Path $DeviceCenterTools 'winlibs\mingw64\bin\gcc.exe'))) {
    if (!(Test-Path -LiteralPath $p)) { throw "Missing: $p" }
}
$consoleCommit = (GitIn $root rev-parse --short HEAD)
$consoleDirty = [bool](GitIn $root status --porcelain)
$dcCommit = (GitIn $DeviceCenterSource rev-parse --short HEAD)
if (GitIn $DeviceCenterSource status --porcelain) { throw "Device Center source has uncommitted changes; commit them so the module maps to a commit: $DeviceCenterSource" }
Write-Output "console $consoleCommit$(if ($consoleDirty) {' (uncommitted changes present)'}), device center $dcCommit"

# --- 1. console --------------------------------------------------------------
Step 'console: frontend build, workspace tests, release build'
& (Join-Path $PSScriptRoot 'Build-Release.ps1') -OutputDirectory $out | Out-Null

# --- 2. device center (GNU) ---------------------------------------------------
$release = Join-Path $DeviceCenterTarget 'release'
$marker = Join-Path $DeviceCenterTarget 'acer-package-source.txt'
$cached = (Test-Path -LiteralPath $marker) -and ((Get-Content -LiteralPath $marker -Raw).Trim() -eq $dcCommit) `
    -and (Test-Path -LiteralPath (Join-Path $release 'device-center.exe')) -and (Test-Path -LiteralPath (Join-Path $release 'WebView2Loader.dll'))
if ($cached -and !$RebuildDeviceCenter) {
    Step "device center: reuse cached build of $dcCommit"
} else {
    Step "device center: test + release build of $dcCommit (GNU)"
    $saved = @{}; foreach ($k in 'PATH','CARGO_HOME','RUSTUP_HOME','CC','AR','CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER','CARGO_TARGET_DIR','CARGO_ENCODED_RUSTFLAGS','RUSTUP_TOOLCHAIN') { $saved[$k] = [Environment]::GetEnvironmentVariable($k) }
    try {
        $env:CARGO_HOME = Join-Path $DeviceCenterTools 'cargo'
        $env:RUSTUP_HOME = Join-Path $DeviceCenterTools 'rustup'
        $loader = Get-ChildItem "$env:CARGO_HOME/registry/src/*/webview2-com-sys-*/x64/WebView2Loader.dll" | Select-Object -Last 1
        if (!$loader) { throw 'WebView2Loader.dll not found in the Device Center cargo registry; run cargo fetch there first.' }
        $env:PATH = "$(Join-Path $DeviceCenterTools 'winlibs\mingw64\bin');$env:CARGO_HOME\bin;$($loader.DirectoryName);$env:PATH"
        $env:CC = 'gcc'; $env:AR = 'ar'; $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = 'gcc'
        $env:CARGO_TARGET_DIR = $DeviceCenterTarget
        $env:CARGO_ENCODED_RUSTFLAGS = '--sysroot' + [char]31 + (Join-Path $env:RUSTUP_HOME 'toolchains/stable-x86_64-pc-windows-gnu')
        $env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-gnu'
        Push-Location $DeviceCenterSource
        try {
            & npm.cmd ci; if ($LASTEXITCODE) { throw 'Device Center npm ci failed' }
            & node --test 'tests/*.mjs'; if ($LASTEXITCODE) { throw 'Device Center JS tests failed' }
            & npm.cmd run build; if ($LASTEXITCODE) { throw 'Device Center frontend build failed' }
            & cargo +stable-x86_64-pc-windows-gnu test --manifest-path src-tauri/Cargo.toml --locked; if ($LASTEXITCODE) { throw 'Device Center cargo test failed' }
            & cargo +stable-x86_64-pc-windows-gnu build --manifest-path src-tauri/Cargo.toml --release --locked --no-default-features --features custom-protocol
            if ($LASTEXITCODE) { throw 'Device Center release build failed' }
        } finally { Pop-Location }
        Copy-Item -LiteralPath $loader.FullName -Destination (Join-Path $release 'WebView2Loader.dll') -Force
        Set-Content -LiteralPath $marker -Value $dcCommit -Encoding ASCII
    } finally { foreach ($k in $saved.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) } }
}

# --- 3. module ----------------------------------------------------------------
Step 'device center: package module'
$moduleDir = Join-Path $out 'modules\device-center'
& (Join-Path $PSScriptRoot 'Package-DeviceCenterModule.ps1') -SourceRoot $DeviceCenterSource -BuildDirectory $release -OutputDirectory $moduleDir | Out-Null
$module = Get-Content -LiteralPath (Join-Path $moduleDir 'module.json') -Raw | ConvertFrom-Json

# --- 4. docs, manifest, zip -----------------------------------------------------
Step 'docs, manifest, zip'
$docs = Join-Path $out 'docs'; New-Item -ItemType Directory -Force -Path $docs | Out-Null
foreach ($d in '当前进度.md','用户功能测试单.md','温控候选版使用与测试.md','文件夹模块使用与验收-2026-11-03.md','UI接入目标-2026-11-06.md','DC-IPC协议-v1.md') {
    Copy-Item -LiteralPath (Join-Path $root "docs\$d") -Destination $docs
}
Copy-Item -LiteralPath (Join-Path $root 'design\apple-redesign-20261105\README.md') -Destination (Join-Path $docs 'UI设计与动效说明.md')

$hashed = 'acer-console.exe','acer-helper.exe','modules/device-center/DeviceCenter.exe','modules/device-center/WebView2Loader.dll','modules/device-center/module.json'
$files = foreach ($f in $hashed) { $i = Get-Item -LiteralPath (Join-Path $out $f); [ordered]@{ name = $f; bytes = $i.Length; sha256 = (Get-FileHash -LiteralPath $i.FullName -Algorithm SHA256).Hash } }
[ordered]@{
    built_at = (Get-Date).ToString('o'); commit = $consoleCommit; commit_dirty = $consoleDirty
    device_center = [ordered]@{ version = $module.version; source = $DeviceCenterSource; commit = $dcCommit; toolchain = 'stable-x86_64-pc-windows-gnu' }
    target = 'PHN16-71'; bios = 'V1.18'; files = @($files)
    validation_note = 'Console workspace tests and Device Center JS/Rust tests passed during this build. No app was launched and no hardware was touched; hardware acceptance is the user''s.'
} | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $out 'build-manifest.json')

# Compress-Archive on Windows PowerShell 5.1 stores sub-folder entries with '\', which
# non-Windows extractors treat as part of the file name; write entries with '/' ourselves.
$expected = @(Get-ChildItem -LiteralPath $out -Recurse -File | ForEach-Object { $_.FullName.Substring($out.Length + 1).Replace('\', '/') }) | Sort-Object
$archive = [IO.Compression.ZipFile]::Open($zip, [IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($rel in $expected) {
        [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, (Join-Path $out $rel), $rel, [IO.Compression.CompressionLevel]::Optimal)
    }
} finally { $archive.Dispose() }
$archive = [IO.Compression.ZipFile]::OpenRead($zip)
try { $entries = @($archive.Entries | Where-Object { $_.Name } | ForEach-Object { $_.FullName }) | Sort-Object } finally { $archive.Dispose() }
if (@($entries | Where-Object { $_ -match '\\' }).Count) { throw 'Zip entries use backslashes.' }
if (Compare-Object $expected $entries) { throw 'Zip entries differ from the package folder.' }

$z = Get-Item -LiteralPath $zip
Step 'done'
Write-Output "zip      $($z.FullName)"
Write-Output "bytes    $($z.Length)"
Write-Output "sha256   $((Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash)"
Write-Output "entries  $($entries.Count)"
Write-Output "console  $consoleCommit$(if ($consoleDirty) {' (dirty)'}); device center $($module.version) @ $dcCommit"
