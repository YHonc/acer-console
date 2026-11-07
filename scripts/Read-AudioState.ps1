[CmdletBinding()]
param([string]$OutputPath = 'evidence\audio-service-read.json')
$ErrorActionPreference = 'Stop'
# Fixed read-only requests from local PredatorSense 5.0.1341 main.js.
$reports = foreach ($query in @(@{ Id = 0; Function = 'SUPPORT_AUDIO_MODE' }, @{ Id = 20; Function = 'SOUND_MODE' })) {
    $client = [Net.Sockets.TcpClient]::new()
    try {
        if (!$client.ConnectAsync('127.0.0.1', 46933).Wait(1000)) { throw 'Official audio service connect timeout' }
        $stream = $client.GetStream(); $stream.ReadTimeout = 1000; $stream.WriteTimeout = 1000
        $payload = [Text.Encoding]::UTF8.GetBytes((@{Function = $query.Function} | ConvertTo-Json -Compress))
        $frame = [Text.Encoding]::ASCII.GetBytes('ACER') + [BitConverter]::GetBytes([uint32]$query.Id) + $payload
        $stream.Write($frame, 0, $frame.Length)
        $bytes = [Collections.Generic.List[byte]]::new(); $buffer = [byte[]]::new(4096)
        $deadline = [Diagnostics.Stopwatch]::StartNew(); $response = $null
        do {
            if ($deadline.ElapsedMilliseconds -gt 3000) { throw 'Official audio read deadline exceeded' }
            $count = $stream.Read($buffer, 0, $buffer.Length)
            if (!$count) { throw 'Official audio service closed before JSON response' }
            $bytes.AddRange([byte[]]$buffer[0..($count - 1)])
            if ($bytes.Count -gt 65536) { throw 'Official audio read response too large' }
            try { $response = [Text.Encoding]::UTF8.GetString($bytes.ToArray()) | ConvertFrom-Json } catch { $response = $null }
        } while ($null -eq $response)
        if ($response.request -ne $query.Function) { throw 'Audio response request mismatch' }
        [ordered]@{ packet_id = $query.Id; function = $query.Function; response = $response; error = $null }
    } catch {
        [ordered]@{ packet_id = $query.Id; function = $query.Function; response = $null; error = $_.Exception.Message }
    } finally { $client.Dispose() }
}
$json = [ordered]@{ captured_at = (Get-Date).ToString('o'); reads = @($reports); writes_attempted = $false } | ConvertTo-Json -Depth 8
[IO.File]::WriteAllText($ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputPath), $json, [Text.UTF8Encoding]::new($false))
$json
