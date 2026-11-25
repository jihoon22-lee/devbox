# Long-lived owned apps inherit only explicit log/NUL handles, never WSL relay pipes.
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
public static class S7OwnedProcessLauncher {
 [StructLayout(LayoutKind.Sequential)] struct SECURITY_ATTRIBUTES { public int length; public IntPtr descriptor; public int inherit; }
 [StructLayout(LayoutKind.Sequential)] struct STARTUPINFO {
  public uint cb; public IntPtr reserved,desktop,title;
  public uint x,y,xSize,ySize,xChars,yChars,fill,flags;
  public ushort show,reservedCount; public IntPtr reservedBytes,input,output,error;
 }
 [StructLayout(LayoutKind.Sequential)] struct STARTUPINFOEX { public STARTUPINFO startup; public IntPtr attributes; }
 [StructLayout(LayoutKind.Sequential)] struct PROCESS_INFORMATION { public IntPtr process,thread; public uint pid,tid; }
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateFile(string name,uint access,uint share,ref SECURITY_ATTRIBUTES security,uint creation,uint flags,IntPtr template);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool CloseHandle(IntPtr handle);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool InitializeProcThreadAttributeList(IntPtr list,int count,int flags,ref IntPtr size);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool UpdateProcThreadAttribute(IntPtr list,uint flags,IntPtr attribute,IntPtr value,IntPtr size,IntPtr previous,IntPtr returned);
 [DllImport("kernel32.dll")] static extern void DeleteProcThreadAttributeList(IntPtr list);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateProcess(string executable,StringBuilder command,IntPtr processSecurity,IntPtr threadSecurity,bool inherit,uint flags,IntPtr environment,string directory,ref STARTUPINFOEX startup,out PROCESS_INFORMATION info);
 static IntPtr File(string path,uint access,uint creation) {
  var security=new SECURITY_ATTRIBUTES {length=Marshal.SizeOf(typeof(SECURITY_ATTRIBUTES)),inherit=1};
  var handle=CreateFile(path,access,3,ref security,creation,0x80,IntPtr.Zero);
  if(handle==new IntPtr(-1))throw new Win32Exception(Marshal.GetLastWin32Error());
  return handle;
 }
 public static int Start(string executable,string arguments,string stdout,string stderr) {
  IntPtr input=IntPtr.Zero,output=IntPtr.Zero,error=IntPtr.Zero,attributes=IntPtr.Zero,handles=IntPtr.Zero;
  bool initialized=false;var info=new PROCESS_INFORMATION();
  try {
   input=File("NUL",0x80000000,3);
   output=File(stdout,0x40000000,1); // CREATE_NEW: never clobber an existing log.
   error=File(stderr,0x40000000,1);
   IntPtr bytes=IntPtr.Zero;InitializeProcThreadAttributeList(IntPtr.Zero,1,0,ref bytes);
   if(bytes==IntPtr.Zero)throw new Win32Exception(Marshal.GetLastWin32Error());
   attributes=Marshal.AllocHGlobal(bytes);
   if(!InitializeProcThreadAttributeList(attributes,1,0,ref bytes))throw new Win32Exception(Marshal.GetLastWin32Error());
   initialized=true;handles=Marshal.AllocHGlobal(IntPtr.Size*3);
   Marshal.WriteIntPtr(handles,0,input);Marshal.WriteIntPtr(handles,IntPtr.Size,output);Marshal.WriteIntPtr(handles,IntPtr.Size*2,error);
   // PROC_THREAD_ATTRIBUTE_HANDLE_LIST excludes every unrelated inherited handle.
   if(!UpdateProcThreadAttribute(attributes,0,new IntPtr(0x20002),handles,new IntPtr(IntPtr.Size*3),IntPtr.Zero,IntPtr.Zero))throw new Win32Exception(Marshal.GetLastWin32Error());
   var startup=new STARTUPINFOEX();startup.startup.cb=(uint)Marshal.SizeOf(typeof(STARTUPINFOEX));
   startup.startup.flags=0x100;startup.startup.input=input;startup.startup.output=output;startup.startup.error=error;startup.attributes=attributes;
   string command="\""+executable+"\""+(String.IsNullOrEmpty(arguments)?"":" "+arguments);
   // Extended startup + no console window + new process group. No shell or broker.
   if(!CreateProcess(executable,new StringBuilder(command),IntPtr.Zero,IntPtr.Zero,true,0x00080000|0x08000000|0x00000200,IntPtr.Zero,null,ref startup,out info))throw new Win32Exception(Marshal.GetLastWin32Error());
   return checked((int)info.pid);
  } finally {
   if(info.thread!=IntPtr.Zero)CloseHandle(info.thread);
   if(info.process!=IntPtr.Zero)CloseHandle(info.process);
   if(initialized)DeleteProcThreadAttributeList(attributes);
   if(attributes!=IntPtr.Zero)Marshal.FreeHGlobal(attributes);
   if(handles!=IntPtr.Zero)Marshal.FreeHGlobal(handles);
   foreach(var handle in new[]{input,output,error})if(handle!=IntPtr.Zero)CloseHandle(handle);
  }
 }
}
'@
function Start-OwnedLoggedProcess([string]$Executable,[string]$StandardOutput,[string]$StandardError,[string]$Arguments='') {
  $ownedPid=[S7OwnedProcessLauncher]::Start($Executable,$Arguments,$StandardOutput,$StandardError)
  return Get-Process -Id $ownedPid
}
