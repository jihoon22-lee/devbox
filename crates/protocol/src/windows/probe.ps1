try {
    $r=[Console]::In.ReadToEnd() | ConvertFrom-Json
    if($r.type -ceq 'observation') {
        $a=$r.assigned
        $s=[DevboxObservation]::Observe([string]$a.jobName,[uint32]$a.pid,[string]$a.creationTime,[string]$a.sid,[uint32]$a.sessionId)
        $state=@{state=$s.State}
        if($s.State -ceq 'active') { $state.count=$s.Count }
        @{type='observation';state=$state} | ConvertTo-Json -Compress -Depth 4
    } elseif($r.type -ceq 'paths') {
        $state='safe'
        foreach($path in $r.paths) { if(![DevboxObservation]::SafePath([string]$path)) { $state='unsafe'; break } }
        @{type='paths';state=$state} | ConvertTo-Json -Compress
    } else { exit 1 }
} catch { exit 1 }
