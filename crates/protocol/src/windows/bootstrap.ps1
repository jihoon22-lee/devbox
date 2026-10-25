$jobHandle=[IntPtr]::Zero
try {
    $r=[Console]::In.ReadToEnd() | ConvertFrom-Json
    $o=$r.observation
    if($null -eq $o -or $o.schema -ne 1 -or [string]$o.nonce -cnotmatch '^[0-9a-f]{32}$') { throw 'control' }
    if([string]$o.jobName -cne ('Local\Devbox.Observe.'+[string]$o.nonce)) { throw 'job' }
    foreach($id in @($r.runId,$r.attemptId,$o.launchId)) {
        if([string]$id -cnotmatch '^[a-zA-Z0-9_-]{1,128}$') { throw 'identity' }
    }
    foreach($path in @($o.assignedPath,$o.grantPath,$o.closedPath)) {
        if(![DevboxObservation]::SafePath([string]$path)) { throw 'path' }
    }
    if([DevboxObservation]::Exists($o.closedPath) -or [DevboxObservation]::Exists($o.assignedPath)) { throw 'closed' }
    $jobHandle=[DevboxObservation]::AssociateFresh([string]$o.jobName)
    $assigned=[ordered]@{
        schema=1; runId=[string]$r.runId; launchId=[string]$o.launchId; attemptId=[string]$r.attemptId
        nonce=[string]$o.nonce; jobName=[string]$o.jobName; pid=[uint32]$PID
        creationTime=[DevboxObservation]::CreationTime(); sid=[DevboxObservation]::Sid(); sessionId=[DevboxObservation]::Session()
    }
    [DevboxObservation]::WriteNew([string]$o.assignedPath,($assigned | ConvertTo-Json -Compress))
    $timer=[Diagnostics.Stopwatch]::StartNew()
    $granted=$false
    while($timer.ElapsedMilliseconds -lt 15000) {
        if([DevboxObservation]::Exists($o.closedPath)) { throw 'closed' }
        if([DevboxObservation]::Exists($o.grantPath)) {
            try { $grant=[DevboxObservation]::ReadBounded($o.grantPath) | ConvertFrom-Json }
            catch { Start-Sleep -Milliseconds 50; continue }
            if(@($grant.PSObject.Properties).Count -ne 10) { throw 'grant' }
            foreach($key in $assigned.Keys) {
                if(!(@($grant.PSObject.Properties.Name) -ccontains $key)) { throw 'grant' }
                if(![string]::Equals([string]$assigned[$key],[string]$grant.$key,[StringComparison]::Ordinal)) { throw 'grant' }
            }
            $granted=$true; break
        }
        Start-Sleep -Milliseconds 50
    }
    if(!$granted -or [DevboxObservation]::Exists($o.closedPath)) { throw 'closed' }
    # Only a matching durable grant can reach user environment, working directory or command.
    foreach($p in $r.env.PSObject.Properties) { [Environment]::SetEnvironmentVariable($p.Name,[string]$p.Value,'Process') }
    Set-Location -LiteralPath $r.cwd
    $global:LASTEXITCODE=0
    & ([ScriptBlock]::Create($r.command))
    $ok=$?; $code=$global:LASTEXITCODE
    if($code -ne 0) { exit $code }
    if(!$ok) { exit 1 }
    exit 0
} catch { [Console]::Error.WriteLine('Windows execution failed'); exit 1 }
finally { if($jobHandle -ne [IntPtr]::Zero) { [void][DevboxObservation]::CloseHandle($jobHandle) } }
