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
    } elseif($r.type -ceq 'listeners') {
        # Compiled only for this probe, so a failure here cannot affect the run bootstrap.
        Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class DevboxListeners {
    [DllImport("iphlpapi.dll")]
    static extern uint GetExtendedTcpTable(IntPtr table, ref int size, bool order, int family, int tableClass, uint reserved);
    [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
    static extern IntPtr OpenJobObject(uint access, bool inherit, string name);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool member);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    // AF_INET=2/AF_INET6=23, TCP_TABLE_OWNER_PID_LISTENER=3, ERROR_INSUFFICIENT_BUFFER=122.
    // MIB_TCPROW_OWNER_PID is 6 DWORDs; MIB_TCP6ROW_OWNER_PID is 56 bytes. Rows follow dwNumEntries.
    // Network-order address bytes: only 127.0.0.1, 0.0.0.0, ::1 and :: are accepted.
    static bool Local(IntPtr buffer,int at,int length) {
        bool wildcard=true, loopback=true;
        for(int b=0;b<length;b++) {
            byte value=Marshal.ReadByte(buffer,at+b);
            byte expected=b==length-1 ? (byte)1 : (length==4 && b==0 ? (byte)127 : (byte)0);
            wildcard&=value==0;
            loopback&=value==expected;
        }
        return wildcard || loopback;
    }
    static List<KeyValuePair<ushort,uint>> Rows(int family) {
        bool v4=family==2;
        int row=v4 ? 24 : 56, address=v4 ? 4 : 0, port=v4 ? 8 : 20, state=v4 ? 0 : 48, owner=v4 ? 20 : 52;
        int size=0, allocated=0;
        uint result=122;
        IntPtr buffer=IntPtr.Zero;
        var rows=new List<KeyValuePair<ushort,uint>>();
        try {
            for(int attempt=0;attempt<4;attempt++) {
                result=GetExtendedTcpTable(buffer,ref size,false,family,3,0);
                if(result==0 && buffer!=IntPtr.Zero) break;
                if(result!=122 || size<=0 || size>16*1024*1024) throw new InvalidOperationException();
                if(buffer!=IntPtr.Zero) Marshal.FreeHGlobal(buffer);
                allocated=size+4096; size=allocated;
                buffer=Marshal.AllocHGlobal(allocated);
            }
            if(result!=0 || buffer==IntPtr.Zero) throw new InvalidOperationException();
            int count=Marshal.ReadInt32(buffer,0);
            if(count<0 || 4L+(long)count*row>allocated) throw new InvalidOperationException();
            for(int i=0;i<count;i++) {
                int at=4+i*row;
                if(Marshal.ReadInt32(buffer,at+state)!=2) continue; // MIB_TCP_STATE_LISTEN
                if(!Local(buffer,at+address,v4 ? 4 : 16)) continue;
                ushort local=(ushort)((Marshal.ReadByte(buffer,at+port)<<8)|Marshal.ReadByte(buffer,at+port+1));
                rows.Add(new KeyValuePair<ushort,uint>(local,(uint)Marshal.ReadInt32(buffer,at+owner)));
            }
            return rows;
        } finally { if(buffer!=IntPtr.Zero) Marshal.FreeHGlobal(buffer); }
    }
    static List<KeyValuePair<ushort,uint>> Listening() {
        var rows=Rows(2);
        rows.AddRange(Rows(23));
        return rows;
    }
    // Job membership is the descendant rule: a fresh named job without breakaway holds every
    // descendant. Exact process handles stay open so a re-read cannot match a reused PID.
    public static string Check(string name,ushort[] ports) {
        IntPtr job=IntPtr.Zero;
        var handles=new Dictionary<uint,IntPtr>();
        try {
            if(ports==null || ports.Length==0) return "unavailable";
            job=OpenJobObject(4,false,name); // JOB_OBJECT_QUERY
            if(job==IntPtr.Zero) return Marshal.GetLastWin32Error()==2 ? "notFound" : "unavailable";
            var rows=Listening();
            var owned=new List<KeyValuePair<ushort,uint>>();
            bool unknown=false;
            foreach(ushort port in ports) {
                bool found=false, failed=false;
                foreach(var candidate in rows) {
                    if(candidate.Key!=port) continue;
                    IntPtr process;
                    if(!handles.TryGetValue(candidate.Value,out process)) {
                        process=OpenProcess(0x1000,false,candidate.Value); // PROCESS_QUERY_LIMITED_INFORMATION
                        if(process==IntPtr.Zero) { failed=true; continue; }
                        handles[candidate.Value]=process;
                    }
                    bool member;
                    if(!IsProcessInJob(process,job,out member)) { failed=true; continue; }
                    if(member) { owned.Add(candidate); found=true; break; }
                }
                if(!found) { if(failed) unknown=true; else return "notOwned"; }
            }
            if(unknown) return "unavailable";
            var again=Listening();
            foreach(var pair in owned) if(!again.Contains(pair)) return "notOwned";
            return "owned";
        } catch { return "unavailable"; }
        finally {
            foreach(IntPtr handle in handles.Values) CloseHandle(handle);
            if(job!=IntPtr.Zero) CloseHandle(job);
        }
    }
}
'@
        $a=$r.assigned
        # The same receipt validation as observation: SID, session, PID creation time, job.
        $s=[DevboxObservation]::Observe([string]$a.jobName,[uint32]$a.pid,[string]$a.creationTime,[string]$a.sid,[uint32]$a.sessionId)
        $state=$s.State
        if($state -ceq 'active') { $state=[DevboxListeners]::Check([string]$a.jobName,[uint16[]]@($r.ports)) }
        @{type='listeners';state=$state} | ConvertTo-Json -Compress
    } else { exit 1 }
} catch { exit 1 }
