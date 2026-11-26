# A8 (1.0.1): the owned app's top-level windows as one type, loaded by native-lifecycle.ps1 -Action window-agent.
# Read-only user32 calls, and the only window messages any helper may post: WM_CLOSE, and WM_SYSCOMMAND with SC_MINIMIZE
# or SC_RESTORE, each a fixed method (validate.py allows these lines in this file only). The agent posts only to the one
# window it proved is the owned app's main window, and only after 30 s without user input (IdleMilliseconds, read-only).
# Pure ASCII (D4).
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class S7OwnedWindow {
 public delegate bool EnumProc(IntPtr window,IntPtr data);
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc proc,IntPtr data);
 [DllImport("user32.dll")] static extern bool IsWindow(IntPtr window);
 [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
 [DllImport("user32.dll")] static extern bool IsIconic(IntPtr window);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr window,StringBuilder name,int size);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr window,StringBuilder text,int size);
 [DllImport("user32.dll",EntryPoint="GetWindowLongW")] static extern int GetWindowLong(IntPtr window,int index);
 [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr window,uint command);
 [DllImport("user32.dll",SetLastError=true)] static extern bool PostMessage(IntPtr window,uint message,IntPtr wParam,IntPtr lParam);
 [StructLayout(LayoutKind.Sequential)] struct LastInput { public uint size; public uint time; }
 [DllImport("user32.dll")] static extern bool GetLastInputInfo(ref LastInput info);
 [DllImport("kernel32.dll")] static extern uint GetTickCount();
 static uint PidOf(IntPtr window) { uint pid=0; GetWindowThreadProcessId(window,out pid); return pid; }
 static string ClassOf(IntPtr window) { var name=new StringBuilder(256); GetClassName(window,name,name.Capacity); return name.ToString(); }
 static string TitleOf(IntPtr window) { var text=new StringBuilder(256); GetWindowText(window,text,text.Capacity); return text.ToString(); }
 static string Flag(bool value) { return value?"1":"0"; }
 // Every top-level window of one process: hwnd|visible|iconic|style|exStyle|owned|class|title (the title last: it may hold '|').
 public static List<string> Of(uint pid) {
  var rows=new List<string>();
  EnumWindows(delegate(IntPtr window,IntPtr data) {
   if(PidOf(window)==pid)rows.Add(window.ToInt64()+"|"+Flag(IsWindowVisible(window))+"|"+Flag(IsIconic(window))+"|"+GetWindowLong(window,-16)+"|"+
    GetWindowLong(window,-20)+"|"+Flag(GetWindow(window,4)!=IntPtr.Zero)+"|"+ClassOf(window)+"|"+TitleOf(window));
   return true;
  },IntPtr.Zero);
  return rows;
 }
 // One window now: pid|visible|iconic|class. The pid is 0 once the handle is no longer a window.
 public static string State(long handle) {
  var window=new IntPtr(handle);
  if(!IsWindow(window))return "0|0|0|";
  return PidOf(window)+"|"+Flag(IsWindowVisible(window))+"|"+Flag(IsIconic(window))+"|"+ClassOf(window);
 }
 // Milliseconds since the last keyboard or mouse input of this session (-1 when it cannot be read). Both clocks are the
 // 32-bit tick count, so the difference is taken modulo 2^32 (unchecked).
 public static long IdleMilliseconds() {
  var info=new LastInput(); info.size=(uint)Marshal.SizeOf(typeof(LastInput));
  if(!GetLastInputInfo(ref info))return -1;
  return unchecked(GetTickCount()-info.time);
 }
 // 0 when the message was posted, else the Win32 error (-1 when none was set).
 static int Posted(bool posted) { if(posted)return 0; int error=Marshal.GetLastWin32Error(); return error==0?-1:error; }
 public static int Close(long handle) { return Posted(PostMessage(new IntPtr(handle),0x0010,IntPtr.Zero,IntPtr.Zero)); }
 public static int Minimize(long handle) { return Posted(PostMessage(new IntPtr(handle),0x0112,new IntPtr(0xF020),IntPtr.Zero)); }
 public static int Restore(long handle) { return Posted(PostMessage(new IntPtr(handle),0x0112,new IntPtr(0xF120),IntPtr.Zero)); }
}
'@
