[CmdletBinding()]
param()
$ErrorActionPreference='Stop'
# Fixed read-only probe. Authentication values remain in memory and are never logged.
$projectRoot=Split-Path -Parent $PSScriptRoot
$rendererPath=Join-Path $projectRoot 'references\research\reverse-continuation\official-app\main.be349a7a7c9ce11c.js'
$renderer=[IO.File]::ReadAllText($rendererPath)
$classOffset=$renderer.IndexOf('this.ServiceName="QuickAccess"')
if($classOffset -lt 0){throw 'Official QuickAccess class not found'}
$classEnd=$renderer.IndexOf('static get instance',$classOffset)
if($classEnd -le $classOffset){throw 'Official QuickAccess constructor boundary missing'}
$constructor=$renderer.Substring($classOffset,$classEnd-$classOffset)
function Read-Literal([string]$Name,[bool]$Parsed){
    $pattern=if($Parsed){'this\.'+[regex]::Escape($Name)+'=C2\.enc\.Utf8\.parse\("((?:\\.|[^"\\])*)"\)'}else{'this\.'+[regex]::Escape($Name)+'="((?:\\.|[^"\\])*)"'}
    $match=[regex]::Match($constructor,$pattern)
    if(!$match.Success){throw 'Official authentication constructor format is unsupported'}
    return ('"'+$match.Groups[1].Value+'"' | ConvertFrom-Json)
}
function Encode-Aes([string]$Text,[string]$Key){
    $aes=[Security.Cryptography.Aes]::Create()
    try{
        $aes.Mode=[Security.Cryptography.CipherMode]::ECB
        $aes.Padding=[Security.Cryptography.PaddingMode]::PKCS7
        $aes.Key=[Text.Encoding]::UTF8.GetBytes($Key)
        $plain=[Text.Encoding]::UTF8.GetBytes($Text)
        $encryptor=$aes.CreateEncryptor()
        try{return [Convert]::ToBase64String($encryptor.TransformFinalBlock($plain,0,$plain.Length))}finally{$encryptor.Dispose()}
    }finally{$aes.Dispose()}
}
$keyA=Read-Literal 'ENCRYPTION_KEY_A' $true
$keyB=Read-Literal 'ENCRYPTION_KEY_B' $false
$keyBArray=Read-Literal 'ENCRYPTION_KEY_B_ARRAY' $true
$question=Read-Literal 'QUESTION_STRING' $false
$socket=[Net.WebSockets.ClientWebSocket]::new()
$socket.Options.Proxy=$null
# Local endpoint pin, measured on this machine. This is not public-CA trust.
# It applies only to this socket after SCM/port/signed-image checks below.
if(-not ('AcerQaCertificatePin' -as [type])){
    Add-Type -TypeDefinition @'
using System;
using System.Net.Security;
using System.Security.Cryptography;
using System.Security.Cryptography.X509Certificates;
public static class AcerQaCertificatePin {
 public static readonly RemoteCertificateValidationCallback Callback = (sender,certificate,chain,errors) => {
  if(certificate==null || (errors & (SslPolicyErrors.RemoteCertificateNameMismatch|SslPolicyErrors.RemoteCertificateNotAvailable))!=0) return false;
  using(var cert=new X509Certificate2(certificate)) {
   var now=DateTime.UtcNow;
   return now>=cert.NotBefore.ToUniversalTime() && now<=cert.NotAfter.ToUniversalTime()
    && cert.GetCertHashString(HashAlgorithmName.SHA256)=="A71513CDC5AE7DD7169DB81E1544A27DE32B22E8CF8F0E85A0B8DF49CC291F9C";
  }
 };
}
'@
}
function Confirm-ServiceBinding {
    $service=Get-CimInstance Win32_Service -Filter "Name='AcerQAAgentSvis'"
    $expected=Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::System)) 'AcerQAAgent.exe'
    if($service.State -ne 'Running' -or $service.PathName.Trim('"') -ine $expected){throw 'QuickAccess service identity differs'}
    $owners=@(Get-NetTCPConnection -State Listen -LocalPort 5141 -ErrorAction Stop|Select-Object -ExpandProperty OwningProcess -Unique)
    if($owners.Count -ne 1 -or $owners[0] -ne $service.ProcessId){throw 'QuickAccess listener is not owned by the expected service'}
    $hash=(Get-FileHash -LiteralPath $expected -Algorithm SHA256).Hash
    if($hash -ne 'E7BC1695ABF4700712B40D23F7642FBEF4AC9C8EA3BF13AEF65D9062AC0CAC4B'){throw 'QuickAccess image changed; review endpoint trust before reconnecting'}
    if((Get-AuthenticodeSignature -LiteralPath $expected).Status -ne 'Valid'){throw 'QuickAccess image signature did not validate'}
    return [ordered]@{service='AcerQAAgentSvis';pid=$service.ProcessId;image_sha256=$hash;signature='Valid';certificate_sha256='A71513CDC5AE7DD7169DB81E1544A27DE32B22E8CF8F0E85A0B8DF49CC291F9C';trust='local_service_bound_exact_certificate_pin';runtime_image_query='not_available_under_standard_user'}
}
function Send-Packet($Packet){
    $bytes=[Text.Encoding]::UTF8.GetBytes(($Packet|ConvertTo-Json -Compress -Depth 8))
    $cts=[Threading.CancellationTokenSource]::new(5000)
    try{$socket.SendAsync([ArraySegment[byte]]::new($bytes),[Net.WebSockets.WebSocketMessageType]::Text,$true,$cts.Token).GetAwaiter().GetResult()}finally{$cts.Dispose()}
}
function Receive-Packet([Threading.CancellationToken]$Token){
    $stream=[IO.MemoryStream]::new()
    try{
        do{
            $buffer=[byte[]]::new(8192)
            $result=$socket.ReceiveAsync([ArraySegment[byte]]::new($buffer),$Token).GetAwaiter().GetResult()
            if($result.MessageType -ne [Net.WebSockets.WebSocketMessageType]::Text){throw 'QuickAccess returned a non-text/closed frame'}
            $stream.Write($buffer,0,$result.Count)
            if($stream.Length -gt 131072){throw 'QuickAccess response exceeds 128 KiB'}
        }while(!$result.EndOfMessage)
        return ([Text.Encoding]::UTF8.GetString($stream.ToArray())|ConvertFrom-Json)
    }finally{$stream.Dispose()}
}
$responses=[Collections.Generic.List[object]]::new()
$summary=[ordered]@{captured_at=(Get-Date).ToString('o');endpoint='wss://localhost:5141';authenticated=$false;writes_sent=0;responses=$responses;error=$null}
try{
    $stage='service_binding'
    $summary.service_binding=Confirm-ServiceBinding
    $socket.Options.RemoteCertificateValidationCallback=[AcerQaCertificatePin]::Callback
    $stage='connect'
    $cts=[Threading.CancellationTokenSource]::new(5000)
    try{$socket.ConnectAsync([Uri]'wss://localhost:5141',$cts.Token).GetAwaiter().GetResult()}finally{$cts.Dispose()}
    $bindingAfter=Confirm-ServiceBinding
    if($bindingAfter.pid -ne $summary.service_binding.pid){throw 'QuickAccess service changed during connection'}
    $authId=[Guid]::NewGuid().ToString()
    $stage='authenticate'
    $authPlain=[ordered]@{Question=$question;Key=$keyB}|ConvertTo-Json -Compress
    Send-Packet ([ordered]@{PacketType=1;Session=$authId;Version=1;Data=(Encode-Aes $authPlain $keyA)})
    $cts=[Threading.CancellationTokenSource]::new(5000)
    try{$auth=Receive-Packet $cts.Token}finally{$cts.Dispose()}
    if($auth.PacketType -ne 1 -or $auth.Data -cne (Encode-Aes $question $keyBArray)){throw 'QuickAccess authentication response did not verify'}
    $summary.authenticated=$true
    $stage='query'
    foreach($command in @('FunctionQuery','BluelightShield','DarkMode','TransparentEffect','AutoHideTaskbar','ScreenSaver','ScreenSaverTimeout','HDR')){
        $requestId=[Guid]::NewGuid().ToString()
        $packet=[ordered]@{PacketType=2;Version=1;Session=$requestId;Command=$command}
        if($command -ne 'FunctionQuery'){$packet.Action='Get'}
        Send-Packet $packet
        $cts=[Threading.CancellationTokenSource]::new(5000)
        try{
            $matched=$false
            for($i=0;$i -lt 64;$i++){
                $reply=Receive-Packet $cts.Token
                if($reply.Session -ne $requestId){continue}
                if($reply.PacketType -notin @(3,9) -or $reply.Command -ne $command){throw 'QuickAccess response identity does not match the request'}
                # Never retain authentication packets or arbitrary unrequested fields.
                $responses.Add([ordered]@{command=$command;packet_type=$reply.PacketType;action=$reply.Action;result=$reply.Result;data=$reply.Data;error_code=$reply.ErrorCode})
                $matched=$true;break
            }
            if(!$matched){throw 'QuickAccess response matching limit reached'}
        }finally{$cts.Dispose()}
    }
}catch{
    # Exceptions are summarized without packet contents or authentication material.
    $failure=$_.Exception
    while($failure.InnerException){$failure=$failure.InnerException}
    $message=$failure.Message
    foreach($secret in @($keyA,$keyB,$keyBArray,$question,$authPlain)){if($secret){$message=$message.Replace([string]$secret,'[redacted]')}}
    $summary.error='Read-only QuickAccess probe failed at '+$stage+': '+$failure.GetType().Name+': '+$message.Substring(0,[Math]::Min(500,$message.Length))
}finally{
    $socket.Abort();$socket.Dispose()
    $keyA=$null;$keyB=$null;$keyBArray=$null;$question=$null;$authPlain=$null;$renderer=$null;$constructor=$null
}
$output=Join-Path $projectRoot 'evidence\quickaccess-read.json'
$summary|ConvertTo-Json -Depth 16|Set-Content -LiteralPath $output -Encoding UTF8
$summary|ConvertTo-Json -Depth 16
if($summary.error){exit 1}
