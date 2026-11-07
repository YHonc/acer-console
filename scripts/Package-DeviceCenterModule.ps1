[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$SourceRoot,
    [Parameter(Mandatory)][string]$BuildDirectory,
    [Parameter(Mandatory)][string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

function FullPath([string]$Path) { [IO.Path]::GetFullPath($Path).TrimEnd([IO.Path]::DirectorySeparatorChar) }
function IsWithin([string]$Child, [string]$Root) {
    $Child.Equals($Root, [StringComparison]::OrdinalIgnoreCase) -or $Child.StartsWith($Root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)
}
function AssertPlainPath([string]$Path, [string]$Root) {
    if (!(IsWithin $Path $Root)) { throw "Path escapes input root: $Path" }
    $item = Get-Item -LiteralPath $Path -Force
    while ($null -ne $item -and (IsWithin $item.FullName $Root)) {
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Reparse input is not accepted: $($item.FullName)" }
        $item = if ($item -is [IO.FileInfo]) { $item.Directory } else { $item.Parent }
    }
}
function HashFile([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash }
function HashBytes([byte[]]$Bytes) {
    $sha = [Security.Cryptography.SHA256]::Create()
    try { [BitConverter]::ToString($sha.ComputeHash($Bytes)).Replace('-', '') } finally { $sha.Dispose() }
}
function WriteJson([string]$Path, $Value) {
    [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 10), [Text.UTF8Encoding]::new($false))
}

# Offline PE import inspection. No executable or DLL is loaded into a process.
function GetPeImports([string]$Path) {
    $bytes = [IO.File]::ReadAllBytes($Path)
    if ($bytes.Length -lt 64 -or [BitConverter]::ToUInt16($bytes, 0) -ne 0x5a4d) { throw "Not a PE file: $Path" }
    $pe = [BitConverter]::ToInt32($bytes, 0x3c)
    if ($pe -lt 0 -or $pe + 24 -gt $bytes.Length -or [BitConverter]::ToUInt32($bytes, $pe) -ne 0x4550) { throw "Invalid PE header: $Path" }
    $sections = [BitConverter]::ToUInt16($bytes, $pe + 6)
    $optionalSize = [BitConverter]::ToUInt16($bytes, $pe + 20)
    $optional = $pe + 24
    $magic = [BitConverter]::ToUInt16($bytes, $optional)
    $directory = switch ($magic) { 0x20b { $optional + 112 } 0x10b { $optional + 96 } default { throw "Unsupported PE optional header: $Path" } }
    $importRva = [BitConverter]::ToUInt32($bytes, $directory + 8)
    $delayRva = [BitConverter]::ToUInt32($bytes, $directory + 13 * 8)
    $sectionTable = $optional + $optionalSize
    $mapRva = {
        param([uint32]$Rva)
        for ($i = 0; $i -lt $sections; $i++) {
            $s = $sectionTable + $i * 40
            $start = [BitConverter]::ToUInt32($bytes, $s + 12)
            $rawSize = [BitConverter]::ToUInt32($bytes, $s + 16)
            $size = [Math]::Max([BitConverter]::ToUInt32($bytes, $s + 8), $rawSize)
            if ($Rva -ge $start -and $Rva -lt $start + $size) {
                $offset = [BitConverter]::ToUInt32($bytes, $s + 20) + $Rva - $start
                if ($Rva - $start -ge $rawSize -or $offset -ge $bytes.Length) { throw "PE RVA has no file bytes: $Path" }
                return [int]$offset
            }
        }
        throw "Unmapped PE RVA: $Path"
    }
    foreach ($table in @(@{rva=$importRva;step=20;name_offset=12}, @{rva=$delayRva;step=32;name_offset=4})) {
        if ($table.rva -eq 0) { continue }
        $descriptor = & $mapRva $table.rva
        $terminated = $false
        while ($descriptor + $table.step -le $bytes.Length) {
            $nameRva = [BitConverter]::ToUInt32($bytes, $descriptor + $table.name_offset)
            if ($nameRva -eq 0) { $terminated = $true; break }
            if ($table.step -eq 32 -and ([BitConverter]::ToUInt32($bytes, $descriptor) -band 1) -eq 0) { throw "Unsupported VA-form delay import: $Path" }
            $start = & $mapRva $nameRva
            $end = $start
            while ($end -lt $bytes.Length -and $bytes[$end] -ne 0) { $end++ }
            if ($end -eq $bytes.Length) { throw "Unterminated import name: $Path" }
            $name = [Text.Encoding]::ASCII.GetString($bytes, $start, $end - $start)
            if ($name -notmatch '^[A-Za-z0-9_.-]+\.dll$') { throw "Unexpected import name: $name" }
            $name
            $descriptor += $table.step
        }
        if (!$terminated) { throw "Unterminated import directory: $Path" }
    }
}

$source = FullPath $SourceRoot
$build = FullPath $BuildDirectory
$output = FullPath $OutputDirectory
foreach ($root in @($source, $build)) {
    if (!(Test-Path -LiteralPath $root -PathType Container)) { throw "Missing input directory: $root" }
    AssertPlainPath $root $root
    if ((IsWithin $output $root) -or (IsWithin $root $output)) { throw 'Output must be separate from source and build inputs.' }
}
if (Test-Path -LiteralPath $output) { throw "Output already exists; choose a new module directory: $output" }
$configPath = Join-Path $source 'src-tauri/tauri.conf.json'
$config = [IO.File]::ReadAllText($configPath) | ConvertFrom-Json
$package = [IO.File]::ReadAllText((Join-Path $source 'package.json')) | ConvertFrom-Json
$version = [string]$config.version
if ($version -notmatch '^\d+\.\d+\.\d+(?:[-+][A-Za-z0-9.-]+)?$' -or $package.version -ne $version) { throw 'Package and Tauri versions must agree.' }
$executables = @('DeviceCenter.exe', 'device-center.exe') | ForEach-Object { Join-Path $build $_ } | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf }
if (@($executables).Count -ne 1) { throw 'BuildDirectory must contain exactly one DeviceCenter.exe or device-center.exe.' }
$entrySource = [string]@($executables)[0]
$runtime = [ordered]@{ 'DeviceCenter.exe' = $entrySource }
$loader = Join-Path $build 'WebView2Loader.dll'
if (!(Test-Path -LiteralPath $loader -PathType Leaf)) { throw 'BuildDirectory is missing WebView2Loader.dll; wait for the completed release build.' }
$runtime['WebView2Loader.dll'] = $loader
$queue = [Collections.Generic.Queue[string]]::new()
foreach ($path in $runtime.Values) { $queue.Enqueue($path) }
$systemImports = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
while ($queue.Count) {
    $path = $queue.Dequeue()
    AssertPlainPath $path $build
    foreach ($name in @(GetPeImports $path)) {
        if ($runtime.Contains($name)) { continue }
        $local = Join-Path $build $name
        if (Test-Path -LiteralPath $local -PathType Leaf) {
            $runtime[$name] = $local
            $queue.Enqueue($local)
        } elseif ($name -match '^(api-ms-|ext-ms-)' -or (Test-Path -LiteralPath (Join-Path ([Environment]::SystemDirectory) $name) -PathType Leaf)) {
            [void]$systemImports.Add($name)
        } else { throw "Unresolved runtime DLL '$name' imported by $([IO.Path]::GetFileName($path)); supply it in BuildDirectory." }
    }
}

# This allowlist is derived from archive-source.ps1, not from an old source ZIP.
$topFiles = @('.gitignore','LICENSE','AGENTS.md','README.md','THIRD_PARTY_NOTICES.md','index.html','package.json','package-lock.json','svelte.config.js','tsconfig.json','vite.config.ts')
$nativeFiles = @('Cargo.toml','Cargo.lock','build.rs','tauri.conf.json','LICENSE','LICENSE-ATK','UPSTREAM.md','build-windows.ps1','validate-installer.ps1','validate-lifecycle.ps1','archive-source.ps1')
$sourceFolders = @('src','src-tauri/src','src-tauri/capabilities','src-tauri/icons','tests','scripts','public','assets/icons-v2')
function SourceFiles {
    $paths = @($topFiles | ForEach-Object { Join-Path $source $_ })
    $paths += @($nativeFiles | ForEach-Object { Join-Path $source ('src-tauri/' + $_) })
    foreach ($folder in $sourceFolders) {
        $dir = Join-Path $source $folder
        if (Test-Path -LiteralPath $dir -PathType Container) {
            AssertPlainPath $dir $source
            $paths += @(Get-ChildItem -LiteralPath $dir -Recurse -File -Force | ForEach-Object FullName)
        }
    }
    # Current human-readable documentation, excluding evidence/history captures.
    $docDir = Join-Path $source 'docs'
    if (Test-Path -LiteralPath $docDir) { $paths += @(Get-ChildItem -LiteralPath $docDir -File -Filter '*.md' | ForEach-Object FullName) }
    @($paths | Sort-Object -Unique)
}
$sourceFiles = @(SourceFiles)
$forbidden = '(?i)(^|/)(\.tools|node_modules|\.git|target|references|releases|artifacts|archive)(/|$)|(^|/)\.env($|\.)|\.(dpapi|dat|pem|key|pfx|p12|csv|exe|dll|zip|log|jsonl)$|(^|/)(mi-cloud-session|credentials?|cookies?|session)(\.|/|$)'
foreach ($file in $sourceFiles) {
    if (!(Test-Path -LiteralPath $file -PathType Leaf)) { throw "Missing allowlisted source: $file" }
    AssertPlainPath $file $source
    $relative = $file.Substring($source.Length + 1).Replace('\', '/')
    if ($relative -match $forbidden) { throw "Refusing non-source/private input: $relative" }
}
$parent = Split-Path $output -Parent
if (!(Test-Path -LiteralPath $parent -PathType Container)) { [void](New-Item -ItemType Directory -Path $parent -Force) }
$stage = Join-Path $parent ('.device-center-module-stage-' + [guid]::NewGuid().ToString('N'))
[void](New-Item -ItemType Directory -Path $stage)
try {
    $runtimeManifest = @()
    foreach ($name in $runtime.Keys) {
        $originalHash = HashFile $runtime[$name]
        $destination = Join-Path $stage $name
        Copy-Item -LiteralPath $runtime[$name] -Destination $destination
        if ((HashFile $destination) -ne $originalHash) { throw "Runtime copy hash mismatch: $name" }
        $runtimeManifest += [ordered]@{path=$name;sha256=$originalHash}
    }
    foreach ($license in @('LICENSE','LICENSE-ATK','UPSTREAM.md')) {
        Copy-Item -LiteralPath (Join-Path $source ('src-tauri/' + $license)) -Destination (Join-Path $stage $license)
    }
    Copy-Item -LiteralPath (Join-Path $source 'THIRD_PARTY_NOTICES.md') -Destination (Join-Path $stage 'THIRD_PARTY_NOTICES.md')
    $archiveName = "DeviceCenter-source-$version.zip"
    $archivePath = Join-Path $stage $archiveName
    $zip = [IO.Compression.ZipFile]::Open($archivePath, [IO.Compression.ZipArchiveMode]::Create)
    $sourceManifest = @()
    try {
        foreach ($file in $sourceFiles) {
            $relative = $file.Substring($source.Length + 1).Replace('\', '/')
            $originalBytes = [IO.File]::ReadAllBytes($file)
            $workspaceHash = HashBytes $originalBytes
            $bytes = $originalBytes
            $sanitized = $false
            if ($relative -match '\.md$') {
                $text = [Text.Encoding]::UTF8.GetString($originalBytes).TrimStart([char]0xfeff)
                $redacted = $text.Replace($source, '<WORKSPACE>').Replace($source.Replace('\','/'), '<WORKSPACE>')
                $redacted = [regex]::Replace($redacted, '(?i)[A-Z]:[\\/]Users[\\/][^\\/\s`"<>]+', '<USER_PROFILE>')
                $redacted = [regex]::Replace($redacted, '(?i)\bPID\s*[:=]?\s*\d+|\bsession\s*\d+', '<RUNTIME_ID>')
                $redacted = [regex]::Replace($redacted, '(?<![\d.])(?:10(?:\.\d{1,3}){3}|192\.168(?:\.\d{1,3}){2}|172\.(?:1[6-9]|2\d|3[01])(?:\.\d{1,3}){2})(?![\d.])', '<LOCAL_DEVICE_IP>')
                $redacted = [regex]::Replace($redacted, '(?i)\b(?:[0-9a-f]{2}:){5}[0-9a-f]{2}\b', '<DEVICE_MAC>')
                $redacted = [regex]::Replace($redacted, 'install-smoke-[a-f0-9]{32}|device-center-target-[A-F0-9]{12}', '<ISOLATED_BUILD_OR_TEST>')
                $bytes = [Text.UTF8Encoding]::new($false).GetBytes($redacted)
                $sanitized = (HashBytes $bytes) -ne $workspaceHash
            }
            $entry = $zip.CreateEntry($relative, [IO.Compression.CompressionLevel]::Optimal)
            $stream = $entry.Open()
            try { $stream.Write($bytes, 0, $bytes.Length) } finally { $stream.Dispose() }
            $sourceManifest += [ordered]@{path=$relative;bytes=$bytes.Length;sha256=(HashBytes $bytes);workspace_sha256=$workspaceHash;documentation_sanitized=$sanitized}
        }
    } finally { $zip.Dispose() }
    $zip = [IO.Compression.ZipFile]::OpenRead($archivePath)
    try {
        if ($zip.Entries.Count -ne $sourceManifest.Count) { throw 'Source ZIP allowlist count mismatch.' }
        foreach ($entry in $zip.Entries) {
            $expected = @($sourceManifest | Where-Object { $_.path -ceq $entry.FullName })
            if ($expected.Count -ne 1) { throw "Unexpected source ZIP entry: $($entry.FullName)" }
            $stream = $entry.Open(); $sha = [Security.Cryptography.SHA256]::Create()
            try { $actual = [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-', '') } finally { $stream.Dispose(); $sha.Dispose() }
            if ($actual -ne $expected[0].sha256) { throw "Source ZIP content mismatch: $($entry.FullName)" }
        }
    } finally { $zip.Dispose() }
    foreach ($entry in $sourceManifest) {
        if ((HashFile (Join-Path $source $entry.path)) -ne $entry.workspace_sha256) { throw "Source changed during packaging: $($entry.path)" }
    }
    if (@(Compare-Object $sourceFiles @(SourceFiles)).Count) { throw 'Source allowlist changed during packaging.' }
    foreach ($entry in $runtimeManifest) {
        if ((HashFile $runtime[$entry.path]) -ne $entry.sha256) { throw "Build changed during packaging: $($entry.path)" }
    }
    $relationship = 'Current allowlisted workspace snapshot associated with the supplied binary SHA256. No commit-pinned build provenance or bit-identical rebuild guarantee.'
    WriteJson (Join-Path $stage 'source-manifest.json') ([ordered]@{schema_version=1;version=$version;binary_sha256=$runtimeManifest[0].sha256;relationship=$relationship;files=$sourceManifest})
    WriteJson (Join-Path $stage 'module.json') ([ordered]@{schema_version=1;id='device-center';name='Device Center';version=$version;entry='DeviceCenter.exe';files=$runtimeManifest})
    $inventory = @(Get-ChildItem -LiteralPath $stage -File | Sort-Object Name | ForEach-Object { [ordered]@{path=$_.Name;bytes=$_.Length;sha256=(HashFile $_.FullName)} })
    WriteJson (Join-Path $stage 'packaging-report.json') ([ordered]@{
        created_utc=[DateTime]::UtcNow.ToString('o');version=$version;files=$inventory;source_file_count=$sourceManifest.Count;
        system_imports=@($systemImports | Sort-Object);source_relationship=$relationship;
        validation='Offline PE imports, runtime copies, source allowlist/ZIP hashes and input stability checked. No application, account, hardware or cloud operation was performed. WebView2 Runtime must be installed.'
    })
    if (Test-Path -LiteralPath $output) { throw 'Output appeared during packaging; refusing to overwrite.' }
    Move-Item -LiteralPath $stage -Destination $output
    [ordered]@{module_root=$output;entry=(Join-Path $output 'DeviceCenter.exe');manifest=(Join-Path $output 'module.json');manifest_sha256=(HashFile (Join-Path $output 'module.json'));source_archive=(Join-Path $output $archiveName);source_archive_sha256=(HashFile (Join-Path $output $archiveName));report=(Join-Path $output 'packaging-report.json')} | ConvertTo-Json
} catch {
    Write-Warning "Packaging failed; inspect staging directory (not a completed module): $stage"
    throw
}
