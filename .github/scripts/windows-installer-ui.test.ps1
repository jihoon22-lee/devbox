param([string]$ScriptDirectory=$PSScriptRoot,[string]$BaselineScript)
# Disposable classic Win32 controls; no WinForms/UIA custom button provider.
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$fixture=Join-Path ([IO.Path]::GetTempPath()) ('devbox-installer-uia-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixture | Out-Null
$image=Join-Path $fixture 'owned-classic-button.exe'
$marker=Join-Path $fixture 'invoked.txt'
$source=@'
using System;
using System.IO;
using System.Runtime.InteropServices;
public class OwnedClassicButton {
 delegate IntPtr Proc(IntPtr h,uint m,IntPtr w,IntPtr l);
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct WC { public uint style; public Proc proc; public int a,b; public IntPtr instance,icon,cursor,brush; public string menu,name; }
 [StructLayout(LayoutKind.Sequential)] struct MSG { public IntPtr h; public uint m; public IntPtr w,l; public uint t; public int x,y; }
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern ushort RegisterClass(ref WC c);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern IntPtr CreateWindowEx(uint ex,string cls,string text,uint style,int x,int y,int w,int h,IntPtr parent,IntPtr menu,IntPtr inst,IntPtr p);
 [DllImport("user32.dll")] static extern IntPtr DefWindowProc(IntPtr h,uint m,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] static extern int GetMessage(out MSG m,IntPtr h,uint min,uint max);
 [DllImport("user32.dll")] static extern bool TranslateMessage(ref MSG m);
 [DllImport("user32.dll")] static extern IntPtr DispatchMessage(ref MSG m);
 [DllImport("user32.dll")] static extern void PostQuitMessage(int code);
 static Proc callback=Window;
 static string marker;
 static IntPtr Window(IntPtr h,uint m,IntPtr w,IntPtr l) {
  if(m==0x111 && (w.ToInt64()&65535)==101) { File.WriteAllText(marker,"owned-button-invoked"); return IntPtr.Zero; }
  if(m==0x10) { PostQuitMessage(0); return IntPtr.Zero; }
  return DefWindowProc(h,m,w,l);
 }
 public static void Main(string[] args) {
  marker=args[0]; WC cls=new WC();cls.proc=callback;cls.name="DevboxOwnedClassic";
  if(RegisterClass(ref cls)==0) throw new Exception("RegisterClass failed");
  IntPtr parent=CreateWindowEx(0,cls.name,"Devbox owned classic fixture",0x10CF0000,100,100,320,180,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero);
  if(parent==IntPtr.Zero) throw new Exception("Parent creation failed");
  if(CreateWindowEx(0,"BUTTON","Owned invoke",0x50000000,20,20,160,40,parent,new IntPtr(101),IntPtr.Zero,IntPtr.Zero)==IntPtr.Zero) throw new Exception("Button creation failed");
  MSG msg;while(GetMessage(out msg,IntPtr.Zero,0,0)>0){TranslateMessage(ref msg);DispatchMessage(ref msg);}
 }
}
'@
Add-Type -TypeDefinition $source -OutputAssembly $image -OutputType ConsoleApplication
$child=Start-Process -FilePath $image -ArgumentList $marker -PassThru
try {
  $started=$child.StartTime.ToUniversalTime().ToString('o')
  if($child.MainModule.FileName -cne $image){throw 'Fixture executable identity mismatch'}
  $arguments=@{TargetProcessId=$child.Id;ExpectedExecutable=$image;ExpectedStartTimeUtc=$started;FixtureRoot=$fixture}
  $scriptPath=Join-Path $ScriptDirectory 'windows-installer-ui.ps1'
  # ScriptBlock execution avoids changing execution policy on this host.
  function Invoke-OwnedAction([string]$Action,[string]$ControlId='',[string]$ObserverScript=$scriptPath) {
    $command="& ([scriptblock]::Create([IO.File]::ReadAllText('"+$ObserverScript.Replace("'","''")+"')))"
    foreach($entry in $arguments.GetEnumerator()){$command+=" -"+$entry.Key+" '"+([string]$entry.Value).Replace("'","''")+"'"}
    $command+=" -Action '"+$Action+"'"
    if($ControlId){$command+=" -ControlId '"+$ControlId+"'"}
    $encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
    $info=[Diagnostics.ProcessStartInfo]::new((Join-Path $PSHOME 'powershell.exe'),'-NoProfile -NonInteractive -EncodedCommand '+$encoded)
    $info.UseShellExecute=$false;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
    $observer=[Diagnostics.Process]::Start($info)
    $stdout=$observer.StandardOutput.ReadToEndAsync();$stderr=$observer.StandardError.ReadToEndAsync()
    if(-not $observer.WaitForExit(20000)){$observer.Kill();$observer.WaitForExit();throw 'Owned UIA observer timed out'}
    $output=$stdout.Result;$errors=$stderr.Result
    if($observer.ExitCode -ne 0){throw ('Owned UIA action failed: '+$errors)}
    return $output
  }
  if($BaselineScript) {
    $baseline=Join-Path $fixture 'baseline-inspector.ps1'
    Copy-Item -LiteralPath $BaselineScript -Destination $baseline
    $deadline=[DateTime]::UtcNow.AddSeconds(5)
    do {$child.Refresh();Start-Sleep -Milliseconds 100} while($child.MainWindowHandle -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
    $view=Invoke-OwnedAction Inspect '' $baseline | ConvertFrom-Json
    $raw=@($view.controls | Where-Object {$_.className -eq 'Button'})
    if($raw.Count -ne 1 -or $raw[0].controlTypeId -ne 50033 -or $raw[0].canInvoke -ne $false){throw 'Baseline did not reproduce classic Pane without InvokePattern'}
    Write-Output 'Original classic native Button: Pane/no InvokePattern reproduced'
  }
  $deadline=[DateTime]::UtcNow.AddSeconds(15)
  do {
    Start-Sleep -Milliseconds 100
    $view=Invoke-OwnedAction Inspect | ConvertFrom-Json
    $button=@($view.buttons | Where-Object {$_.id -eq '101' -and $_.name -eq 'Owned invoke'})
  } while($button.Count -ne 1 -and [DateTime]::UtcNow -lt $deadline)
  if($button.Count -ne 1 -or $button[0].controlTypeId -ne 50000 -or $button[0].canInvoke -ne $true -or $button[0].visible -ne $true -or $button[0].enabled -ne $true){throw 'Classic Button was not classified as a visible enabled invokable Button'}
  Invoke-OwnedAction Invoke 101
  $deadline=[DateTime]::UtcNow.AddSeconds(5)
  while(-not(Test-Path -LiteralPath $marker) -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 50}
  if(-not(Test-Path -LiteralPath $marker) -or (Get-Content -LiteralPath $marker -Raw) -cne 'owned-button-invoked'){throw 'Classic Button Invoke did not produce its owned marker'}
  Invoke-OwnedAction Close
  if(-not $child.WaitForExit(5000)){throw 'Owned fixture normal close failed'}
  Write-Output 'Classic native Button classification and Invoke: PASS'
} finally {
  if(-not $child.HasExited){
    if($child.MainModule.FileName -cne $image -or $child.StartTime.ToUniversalTime().ToString('o') -cne $started){throw 'Fixture cleanup ownership changed'}
    $child.Kill();$child.WaitForExit()
  }
  Remove-Item -LiteralPath $fixture -Recurse -Force
}
