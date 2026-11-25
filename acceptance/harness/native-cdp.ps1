param(
  [ValidateSet('targets','invoke')][string]$Action,
  [string]$Ownership, [int]$Port=0,
  [ValidateSet('update_status','update_check','update_download','update_install_summary','update_install_confirm')][string]$Command,
  [long]$Generation=-1, [long]$ConfirmationId=-1, [int]$TimeoutMs=15000
)
# 1.0.1 (D5): the one client of the owned accept app's WebView2 debug port. Pure ASCII (D4).
# - It attaches only to the port this run recorded for a process that still runs the owned executable (exact PID,
#   path and start time), whose listener is on 127.0.0.1 and descends from that process.
# - It calls only the five update commands the Settings screen calls (app/src/features/settings/nativeUpdate.ts),
#   through window.__TAURI_INTERNALS__.invoke in the main window, with integers as the only arguments.
# - Every wait is bounded: 5 s for the target list, the connect and the send, TimeoutMs (1 to 60 s) for the reply.
# - It starts, stops and signals no process and sends no input to a window. The debug-port app itself is ended by
#   native-lifecycle.ps1 (stop-owned), and assert-stopped proves the port closed.
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=New-Object Text.UTF8Encoding
$m=Get-Content -LiteralPath $Ownership -Raw -Encoding UTF8 | ConvertFrom-Json
if ($m.channel -ne 'accept' -or $m.nativePreflight.acceptAbsent -ne $true -or $m.token -notmatch '^owned-[0-9a-f]{12}$') {throw 'Owned accept preflight required'}
$local=[Environment]::GetFolderPath('LocalApplicationData')
$install=$m.nativePreflight.installPath
if ($install -notmatch ('^'+[regex]::Escape($local)+'\\DevboxAccept-s7-[a-z0-9-]+$')) {throw 'Unexpected owned install path'}
if ($Port -lt 1024 -or $Port -gt 65535) {throw 'Owned debug port out of range'}
if ($TimeoutMs -lt 1000 -or $TimeoutMs -gt 60000) {throw 'Reply timeout out of range'}
$native=Get-Content -LiteralPath (Join-Path (Split-Path $Ownership) ('native-'+$m.token+'.json')) -Raw -Encoding UTF8 | ConvertFrom-Json
$exe=Join-Path $install 'Devbox.exe'
$owners=@($native.processes | Where-Object { $_ -and [int]$_.cdpPort -eq $Port -and $_.executable -ieq $exe } | ForEach-Object {
  $p=Get-Process -Id $_.pid -ErrorAction SilentlyContinue
  if ($p -and $p.Path -ieq $exe -and $p.StartTime.ToUniversalTime().ToString('o') -eq $_.startedUtc) { $p.Id }
})
if ($owners.Count -ne 1) {throw 'The debug port does not belong to exactly one running owned app'}
$listeners=@(Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue)
if (-not $listeners.Count) {throw 'The owned debug port is not listening'}
foreach ($listener in $listeners) {
  if ([string]$listener.LocalAddress -ne '127.0.0.1') {throw 'The debug port listens beyond 127.0.0.1'}
  # The listener is the WebView2 browser process: the owned app, or a process it started (at most four levels up).
  $current=[int]$listener.OwningProcess
  $owned=$false
  for ($level=0; $level -lt 4 -and $current -gt 0; $level++) {
    if ($current -eq $owners[0]) { $owned=$true; break }
    $row=Get-CimInstance Win32_Process -Filter ('ProcessId='+$current) -ErrorAction SilentlyContinue
    if (-not $row) { break }
    $current=[int]$row.ParentProcessId
  }
  if (-not $owned) {throw 'The debug port listener does not descend from the owned app'}
}
function Await($task,[int]$milliseconds,[string]$what) {
  try { $done=$task.Wait($milliseconds) } catch { throw ($what+' failed: '+$_.Exception.GetBaseException().Message) }
  if (-not $done) {throw ($what+' timed out')}
}
$request=[Net.HttpWebRequest]::Create('http://127.0.0.1:'+$Port+'/json/list')
$request.Method='GET'
$request.Timeout=5000
$request.ReadWriteTimeout=5000
$request.Proxy=$null
$response=$request.GetResponse()
try {
  $reader=New-Object IO.StreamReader($response.GetResponseStream(),[Text.Encoding]::UTF8)
  $block=New-Object char[] 262145
  $count=$reader.ReadBlock($block,0,$block.Length)
  if ($count -gt 262144) {throw 'Debug target list too large'}
  $listText=[string]::new($block,0,$count)
} finally { $response.Close() }
$pages=@($listText | ConvertFrom-Json | ForEach-Object { $_ } | Where-Object { $_ -and $_.type -eq 'page' })
$rows=@($pages | ForEach-Object { @{id=[string]$_.id;url=([string]$_.url).Split('?')[0];title=[string]$_.title} })
# The quick capture window is /popout/capture; every other pop-out has /popout/ too. The main window is the one page left.
$main=@($pages | Where-Object { ([string]$_.url) -notmatch '/popout/' })
if ($Action -eq 'targets') {
  @{port=$Port;appPid=$owners[0];pages=$rows;mainPages=$main.Count} | ConvertTo-Json -Depth 5 -Compress
  exit
}
if ($main.Count -ne 1) {throw ('Exactly one main window page is required; found '+$main.Count+' of '+$pages.Count)}
$socketUrl=[string]$main[0].webSocketDebuggerUrl
if ($socketUrl -notmatch ('^ws://127\.0\.0\.1:'+$Port+'/devtools/page/[A-Za-z0-9-]+$')) {throw 'The page debugger address is not on the owned port'}
$arguments='{}'
if ($Command -eq 'update_download' -or $Command -eq 'update_install_summary') {
  if ($Generation -lt 0) {throw 'Generation required'}
  $arguments='{generation:'+$Generation+'}'
} elseif ($Command -eq 'update_install_confirm') {
  if ($Generation -lt 0 -or $ConfirmationId -le 0) {throw 'Generation and confirmation id required'}
  $arguments='{generation:'+$Generation+',confirmationId:'+$ConfirmationId+'}'
} elseif ($Generation -ne -1 -or $ConfirmationId -ne -1) {throw 'This command takes no argument'}
# The page answers with one JSON string: {ok, label, value} or {ok:false, label, error}. A rejected invoke is an answer
# (the product's own error code), not a helper failure.
$expression='(async () => { const t = window.__TAURI_INTERNALS__; if (!t || typeof t.invoke !== "function") { return JSON.stringify({ok:false,label:null,error:"no-tauri"}); } '+
  'const m = t.metadata; const label = (m && m.currentWindow && m.currentWindow.label) || null; '+
  'try { const value = await t.invoke("'+$Command+'", '+$arguments+'); return JSON.stringify({ok:true,label:label,value:value}); } '+
  'catch (e) { return JSON.stringify({ok:false,label:label,error:String(e)}); } })()'
$message=@{id=1;method='Runtime.evaluate';params=@{expression=$expression;awaitPromise=$true;returnByValue=$true}} | ConvertTo-Json -Depth 5 -Compress
$clock=[Diagnostics.Stopwatch]::StartNew()
$socket=New-Object Net.WebSockets.ClientWebSocket
$cancel=New-Object Threading.CancellationTokenSource
$cancel.CancelAfter($TimeoutMs+15000)
$reply=$null
try {
  Await ($socket.ConnectAsync([Uri]$socketUrl,$cancel.Token)) 5000 'Debug socket connect'
  $bytes=[Text.Encoding]::UTF8.GetBytes($message)
  Await ($socket.SendAsync([ArraySegment[byte]]::new($bytes),[Net.WebSockets.WebSocketMessageType]::Text,$true,$cancel.Token)) 5000 'Debug socket send'
  $deadline=[DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
  $buffer=New-Object byte[] 65536
  while (-not $reply) {
    $frame=New-Object IO.MemoryStream
    do {
      $left=[int]($deadline-[DateTime]::UtcNow).TotalMilliseconds
      if ($left -le 0) {throw 'Debug socket reply timed out'}
      $receive=$socket.ReceiveAsync([ArraySegment[byte]]::new($buffer),$cancel.Token)
      Await $receive $left 'Debug socket reply'
      $part=$receive.Result
      if ($part.MessageType -eq [Net.WebSockets.WebSocketMessageType]::Close) {throw 'Debug socket closed before the reply'}
      $frame.Write($buffer,0,$part.Count)
      if ($frame.Length -gt 4194304) {throw 'Debug socket reply too large'}
    } while (-not $part.EndOfMessage)
    $parsed=[Text.Encoding]::UTF8.GetString($frame.ToArray()) | ConvertFrom-Json
    if ($parsed.id -eq 1) { $reply=$parsed }
  }
} finally {
  $cancel.Cancel()
  $socket.Abort()
  $socket.Dispose()
  $cancel.Dispose()
}
if ($reply.error) {throw ('Debug protocol error: '+[string]$reply.error.message)}
if ($reply.result.exceptionDetails) {throw ('Page evaluation failed: '+[string]$reply.result.exceptionDetails.text)}
$value=[string]$reply.result.result.value
if (-not $value -or $value.Length -gt 65536) {throw 'Page reply missing or too large'}
@{port=$Port;appPid=$owners[0];target=@{id=[string]$main[0].id;url=([string]$main[0].url).Split('?')[0]};command=$Command;
  reply=($value | ConvertFrom-Json);ms=[int]$clock.ElapsedMilliseconds} | ConvertTo-Json -Depth 8 -Compress
