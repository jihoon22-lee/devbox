# Job holder. The bootstrap starts it outside the job before joining it. A JOB_OBJECT_QUERY
# handle keeps the job name while orphaned descendants outlive the bootstrap, and the
# {"nonce":...} receipt is published only after the job empties. $devboxInput holds base64
# job name, nonce, ready marker and receipt path, all validated by the bootstrap.
$ErrorActionPreference='Stop'
Add-Type -TypeDefinition '
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
public static class DevboxHolder {
    [StructLayout(LayoutKind.Sequential)] struct Accounting {
        public long User, Kernel, PeriodUser, PeriodKernel;
        public uint Faults, Total, Active, Terminated;
    }
    [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
    static extern IntPtr OpenJobObject(uint access, bool inherit, string name);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool QueryInformationJobObject(IntPtr job, int kind, out Accounting value, uint size, IntPtr returned);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    public static void Hold(string name,string nonce,string ready,string receipt) {
        IntPtr job=OpenJobObject(4,false,name); // JOB_OBJECT_QUERY: keeps the name, changes nothing.
        if(job==IntPtr.Zero) return; // No job: leave at once, without a marker or receipt.
        try {
            string body="{\"nonce\":\""+nonce+"\"}";
            WriteNew(ready,body);
            var timer=System.Diagnostics.Stopwatch.StartNew();
            uint size=(uint)Marshal.SizeOf(typeof(Accounting));
            Accounting account;
            // JobObjectBasicAccountingInformation. Total counts every process ever assigned, so
            // a command that ends between two samples still counts. A failed query leaves.
            while(QueryInformationJobObject(job,1,out account,size,IntPtr.Zero)) {
                if(account.Total>=1 && account.Active==0) { WriteNew(receipt,body); return; }
                if(account.Total==0 && timer.ElapsedMilliseconds>=60000) return;
                System.Threading.Thread.Sleep(500);
            }
        } finally { CloseHandle(job); }
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
            current=Path.GetDirectoryName(current.TrimEnd(Path.DirectorySeparatorChar));
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
}
'
$job,$nonce,$ready,$receipt=$devboxInput | ForEach-Object { [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($_)) }
[DevboxHolder]::Hold($job,$nonce,$ready,$receipt)
