$ErrorActionPreference='Stop'
$utf8=New-Object System.Text.UTF8Encoding($false)
[Console]::InputEncoding=$utf8
[Console]::OutputEncoding=$utf8
$OutputEncoding=$utf8
Add-Type -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Text;
public sealed class DevboxJobState {
    public string State;
    public uint Count;
    public DevboxJobState(string state, uint count=0) { State=state; Count=count; }
}
public static class DevboxObservation {
    [StructLayout(LayoutKind.Sequential)] struct FileTime { public uint Low, High; }
    [StructLayout(LayoutKind.Sequential)] struct Accounting {
        public long User, Kernel, PeriodUser, PeriodKernel;
        public uint Faults, Total, Active, Terminated;
    }
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct StartupInfo {
        public int Size; public string Reserved, Desktop, Title;
        public int X, Y, Width, Height, Columns, Rows, Fill, Flags;
        public short Show, Reserved2; public IntPtr Reserved3, Input, Output, Error;
    }
    [StructLayout(LayoutKind.Sequential)] struct ProcessInfo { public IntPtr Process, Thread; public uint Pid, Tid; }
    [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
    static extern bool CreateProcess(string application, StringBuilder command, IntPtr processSecurity, IntPtr threadSecurity, bool inherit, uint flags, IntPtr environment, string directory, ref StartupInfo startup, out ProcessInfo info);
    [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
    static extern IntPtr CreateJobObject(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
    static extern IntPtr OpenJobObject(uint access, bool inherit, string name);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool QueryInformationJobObject(IntPtr job, int kind, out Accounting value, uint size, IntPtr returned);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool member);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool GetExitCodeProcess(IntPtr process, out uint code);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool GetProcessTimes(IntPtr process, out FileTime created, out FileTime exited, out FileTime kernel, out FileTime user);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool ProcessIdToSessionId(uint pid, out uint session);
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll")] static extern uint GetCurrentProcessId();
    [DllImport("kernel32.dll")] static extern void SetLastError(uint error);
    [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr handle);
    static string Creation(IntPtr process) {
        FileTime created, exited, kernel, user;
        if(!GetProcessTimes(process,out created,out exited,out kernel,out user)) throw new IOException();
        return (((ulong)created.High << 32) | created.Low).ToString(CultureInfo.InvariantCulture);
    }
    public static string CreationTime() { return Creation(GetCurrentProcess()); }
    public static string Sid() { using(var identity=WindowsIdentity.GetCurrent()) { return identity.User.Value; } }
    public static uint Session() {
        uint session;
        if(!ProcessIdToSessionId(GetCurrentProcessId(),out session)) throw new IOException();
        return session;
    }
    public static IntPtr CreateFresh(string name) {
        SetLastError(0);
        IntPtr job=CreateJobObject(IntPtr.Zero,name);
        int error=Marshal.GetLastWin32Error();
        if(job==IntPtr.Zero) throw new IOException();
        if(error==183) { CloseHandle(job); throw new IOException(); }
        // Fresh job defaults: no breakaway and no KILL_ON_JOB_CLOSE or termination call.
        return job;
    }
    public static void AssociateFresh(IntPtr job) {
        if(!AssignProcessToJobObject(job,GetCurrentProcess())) throw new IOException();
    }
    // Called before AssociateFresh, so the holder runs outside the job. bInheritHandles=false
    // and CREATE_NO_WINDOW: an inherited stdio or WSL interop pipe would hold the caller's EOF
    // open for the holder's lifetime. Values arrive as base64 literals, never as script text.
    public static bool StartHolder(string script,string job,string nonce,string ready,string receipt) {
        string[] values={job,nonce,ready,receipt};
        for(int i=0;i<values.Length;i++)
            values[i]="'"+Convert.ToBase64String(new UTF8Encoding(false).GetBytes(values[i]))+"'";
        string text="$devboxInput="+String.Join(",",values)+"\n"+script;
        string shell=Path.Combine(Environment.SystemDirectory,"WindowsPowerShell\\v1.0\\powershell.exe");
        var command=new StringBuilder("\""+shell+"\" -NoProfile -NonInteractive -EncodedCommand "+Convert.ToBase64String(Encoding.Unicode.GetBytes(text)));
        var startup=new StartupInfo();
        startup.Size=Marshal.SizeOf(typeof(StartupInfo));
        ProcessInfo started;
        if(!CreateProcess(shell,command,IntPtr.Zero,IntPtr.Zero,false,0x08000000,IntPtr.Zero,Environment.SystemDirectory,ref startup,out started)) return false;
        CloseHandle(started.Thread); CloseHandle(started.Process);
        return true;
    }
    public static DevboxJobState Observe(string name,uint pid,string creation,string sid,uint session) {
        IntPtr process=IntPtr.Zero,job=IntPtr.Zero;
        try {
            if(!String.Equals(Sid(),sid,StringComparison.Ordinal) || Session()!=session)
                return new DevboxJobState("identityMismatch");
            process=OpenProcess(0x1000,false,pid);
            bool reused=false;
            if(process==IntPtr.Zero) {
                if(Marshal.GetLastWin32Error()!=87) return new DevboxJobState("unavailable");
            } else if(!String.Equals(Creation(process),creation,StringComparison.Ordinal)) {
                reused=true;
            }
            job=OpenJobObject(4,false,name);
            if(job==IntPtr.Zero) {
                if(Marshal.GetLastWin32Error()!=2) return new DevboxJobState("unavailable");
                // A PID is reused only after the assigned process object is gone. Another holder can
                // keep an exited process openable; an exit code proves exit, STILL_ACTIVE(259) does not.
                if(process==IntPtr.Zero || reused) return new DevboxJobState("notFound");
                uint code;
                if(!GetExitCodeProcess(process,out code)) return new DevboxJobState("unavailable");
                return new DevboxJobState(code==259 ? "identityMismatch" : "notFound");
            }
            if(reused) return new DevboxJobState("identityMismatch");
            if(process!=IntPtr.Zero) {
                bool member;
                if(!IsProcessInJob(process,job,out member)) return new DevboxJobState("unavailable");
                if(!member) return new DevboxJobState("identityMismatch");
            }
            Accounting account;
            if(!QueryInformationJobObject(job,1,out account,(uint)Marshal.SizeOf(typeof(Accounting)),IntPtr.Zero))
                return new DevboxJobState("unavailable");
            return account.Active==0 ? new DevboxJobState("empty") : new DevboxJobState("active",account.Active);
        } catch { return new DevboxJobState("unavailable"); }
        finally { if(job!=IntPtr.Zero) CloseHandle(job); if(process!=IntPtr.Zero) CloseHandle(process); }
    }
    public static bool Exists(string path) {
        try { File.GetAttributes(path); return true; }
        catch(FileNotFoundException) { return false; }
        catch(DirectoryNotFoundException) { return false; }
        // Access denied and other failures are never evidence of absence.
    }
    public static bool SafePath(string path) {
        string full=Path.GetFullPath(path), root=Path.GetPathRoot(full), current=full;
        if(!Path.IsPathRooted(path) || String.IsNullOrEmpty(root)) return false;
        for(int depth=0;depth<512;depth++) {
            try {
                if((File.GetAttributes(current)&FileAttributes.ReparsePoint)!=0) return false;
            } catch(FileNotFoundException) { if(current.Length<=root.Length) throw; }
              catch(DirectoryNotFoundException) { if(current.Length<=root.Length) throw; }
            if(current.Length<=root.Length) return true;
            current=Path.GetDirectoryName(current.TrimEnd('\\'));
            if(String.IsNullOrEmpty(current)) return false;
        }
        return false;
    }
    public static void WriteNew(string path,string json) {
        if(!SafePath(path)) throw new IOException();
        string temporary=path+".tmp-"+Guid.NewGuid().ToString("N");
        try {
            using(var file=new FileStream(temporary,FileMode.CreateNew,FileAccess.Write,FileShare.None)) {
                byte[] bytes=new UTF8Encoding(false).GetBytes(json);
                file.Write(bytes,0,bytes.Length); file.Flush(true);
            }
            File.Move(temporary,path); // Same directory, atomic publish, never replaces an existing receipt.
        } finally { if(File.Exists(temporary)) File.Delete(temporary); }
    }
    public static string ReadBounded(string path) {
        if(!SafePath(path)) throw new IOException();
        using(var file=new FileStream(path,FileMode.Open,FileAccess.Read,FileShare.Read|FileShare.Delete)) {
            long size=file.Length;
            if(size<=0 || size>16384) throw new IOException();
            byte[] bytes=new byte[(int)size]; int offset=0;
            while(offset<bytes.Length) { int n=file.Read(bytes,offset,bytes.Length-offset); if(n==0) throw new IOException(); offset+=n; }
            if(file.Length!=size) throw new IOException();
            return new UTF8Encoding(false,true).GetString(bytes);
        }
    }
}
'@
