param([string]$ScriptDirectory=$PSScriptRoot,[string]$BaselineScript,[switch]$FrameworkHelpers,[switch]$WorkspaceAuxiliary,[switch]$ZoomSerialization,[switch]$NativePickerRoots)
# Disposable classic Win32 controls; no WinForms/UIA custom button provider.
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
# Pure image admission checks run with every existing helper regression mode.
$imageSource=[IO.File]::ReadAllText((Join-Path $ScriptDirectory 'windows-installer-ui.ps1'))
$imageTokens=$null;$imageErrors=$null
$imageAst=[Management.Automation.Language.Parser]::ParseInput($imageSource,[ref]$imageTokens,[ref]$imageErrors)
if($imageErrors.Count -ne 0){throw 'Native driver syntax invalid'}
$imageDefinition=$imageAst.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Test-OwnedProductImage'},$true)
if($null -eq $imageDefinition){throw 'Owned product image admission missing'}
. ([scriptblock]::Create($imageDefinition.Extent.Text))
$imageRoot='C:\Temp\devbox-suite-delivery-'+('a'*32)
foreach($product in @('workspace','api-studio','knowledge','control-center')) {
  $name='direct-'+$product+'-Ab123x'
  if(-not (Test-OwnedProductImage ($imageRoot+'\'+$name+'\'+$name+'.exe') $imageRoot $product)){throw 'Renamed owned product rejected'}
  if(-not (Test-OwnedProductImage ($imageRoot+'\devbox-'+$product+'.exe') $imageRoot $product)){throw 'Original product rejected'}
  foreach($invalid in @($imageRoot+'\different\'+$name+'.exe','C:\foreign\'+$name+'\'+$name+'.exe',$imageRoot+'\'+$name+'\'+$name+'-extra.exe')) {
    if(Test-OwnedProductImage $invalid $imageRoot $product){throw 'Invalid renamed product admitted'}
  }
}
if($NativePickerRoots) {
  $content=[IO.File]::ReadAllText((Join-Path $ScriptDirectory 'windows-installer-ui.ps1'))
  $tokens=$null;$errors=$null
  $ast=[Management.Automation.Language.Parser]::ParseInput($content,[ref]$tokens,[ref]$errors)
  if($errors.Count -ne 0){throw 'Picker driver syntax invalid'}
  $definition=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-OwnedNativePickers'},$true)
  if($null -eq $definition){throw 'Owned native picker resolver missing'}
  . ([scriptblock]::Create($definition.Extent.Text))
  $native=[pscustomobject]@{nativeHandle=101;nativeProcessId=42;visible=$true;topLevel=$true;className='#32770'}
  $inventory=[pscustomobject]@{count=1;windows=@($native)}
  $resolved=[pscustomobject]@{Current=[pscustomobject]@{NativeWindowHandle=101;ProcessId=42;ClassName='#32770'}}
  $resolver={param($handle,$owner) if($handle -ne 101 -or $owner -ne 42){throw 'Unexpected picker resolution'}; $resolved}
  # No UIA desktop children are supplied: resolve the native-owned HWND directly.
  $roots=@(Get-OwnedNativePickers $inventory 42 $resolver)
  if($roots.Count -ne 1 -or $roots[0].Current.NativeWindowHandle -ne 101){throw 'Native picker omitted by UIA desktop tree was lost'}
  foreach($kind in @('ambiguous','foreign','overflow','changed-handle')) {
    $sample=[pscustomobject]@{count=1;windows=@($native)}
    $resolve=$resolver
    if($kind -eq 'ambiguous'){$sample.count=2;$sample.windows=@($native,$native)}
    if($kind -eq 'foreign'){$sample.windows=@([pscustomobject]@{nativeHandle=101;nativeProcessId=43;visible=$true;topLevel=$true;className='#32770'})}
    if($kind -eq 'overflow'){$sample.count=33}
    if($kind -eq 'changed-handle'){$resolve={param($handle,$owner) [pscustomobject]@{Current=[pscustomobject]@{NativeWindowHandle=102;ProcessId=42;ClassName='#32770'}}}}
    $rejected=$false
    try {Get-OwnedNativePickers $sample 42 $resolve | Out-Null} catch {$rejected=$true}
    if(-not $rejected){throw ('Unsafe native picker was accepted: '+$kind)}
  }
  Write-Output 'Native picker exact HWND selection and ownership rejection: PASS (no native windows/input)'
  return
}
if($ZoomSerialization) {
  $scriptPath=Join-Path $ScriptDirectory 'windows-installer-ui.ps1'
  $content=[IO.File]::ReadAllText($scriptPath)
  $tokens=$null;$parseErrors=$null
  [void][Management.Automation.Language.Parser]::ParseInput($content,[ref]$tokens,[ref]$parseErrors)
  if($parseErrors.Count -ne 0){throw 'Native script PowerShell syntax failed'}
  $source=[regex]::Match($content,'(?s)Add-Type -ReferencedAssemblies UIAutomationClient,UIAutomationTypes -TypeDefinition @"\r?\n(.*?)\r?\n"@').Groups[1].Value
  if(-not $source){throw 'Native helper source unavailable'}
  Add-Type -ReferencedAssemblies UIAutomationClient,UIAutomationTypes -TypeDefinition $source
  if(-not [DevboxInstallerAutomation]::IsUniqueWebViewLayout(2,1,1,$true,$true)){throw 'Exact native WebView layout rejected'}
  foreach($shape in @(@(257,1,1,$true,$true),@(2,0,1,$true,$true),@(3,2,1,$true,$true),@(3,1,2,$true,$true),@(2,1,1,$false,$true),@(2,1,1,$true,$false))) {
    if([DevboxInstallerAutomation]::IsUniqueWebViewLayout($shape[0],$shape[1],$shape[2],$shape[3],$shape[4])){throw 'Ambiguous or foreign WebView layout accepted'}
  }
  $expectedSize=if([IntPtr]::Size -eq 8){40}else{28}
  if([Runtime.InteropServices.Marshal]::SizeOf([type][DevboxInstallerAutomation+Input]) -ne $expectedSize){throw 'Native INPUT layout mismatch'}
  foreach($reset in @($false,$true)) {
    $key=if($reset){48}else{107}
    $chord=[DevboxInstallerAutomation]::ZoomChord($reset,$false)
    if($chord.Count -ne 4){throw 'Zoom chord length'}
    $keys=@(17,$key,$key,17);$flags=@(0,0,2,2)
    for($index=0;$index -lt 4;$index++) {
      if($chord[$index].type -ne 1 -or $chord[$index].value.keyboard.key -ne $keys[$index] -or $chord[$index].value.keyboard.flags -ne $flags[$index] -or $chord[$index].value.keyboard.scan -ne 0){throw 'Zoom allowlist serialization mismatch'}
    }
    $release=[DevboxInstallerAutomation]::ZoomChord($reset,$true)
    if($release.Count -ne 2 -or $release[0].value.keyboard.key -ne $key -or $release[1].value.keyboard.key -ne 17 -or @($release | Where-Object {$_.value.keyboard.flags -ne 2}).Count -ne 0){throw 'Zoom finally key release mismatch'}
  }
  # Native picker text uses one select-all chord and UTF-16 Unicode pairs.
  $filename='C:\owned\한글😀.json'
  $typed=[DevboxInstallerAutomation]::PickerTextInput($filename)
  if($typed.Count -ne (4+2*$filename.Length)){throw 'Picker input count mismatch'}
  $keys=@(17,65,65,17);$flags=@(0,0,2,2)
  for($index=0;$index -lt 4;$index++) {
    if($typed[$index].type -ne 1 -or $typed[$index].value.keyboard.key -ne $keys[$index] -or $typed[$index].value.keyboard.flags -ne $flags[$index]){throw 'Picker select-all chord mismatch'}
  }
  for($index=0;$index -lt $filename.Length;$index++) {
    $down=$typed[4+2*$index];$up=$typed[5+2*$index]
    if($down.type -ne 1 -or $up.type -ne 1 -or $down.value.keyboard.key -ne 0 -or $up.value.keyboard.key -ne 0 -or $down.value.keyboard.scan -ne [int]$filename[$index] -or $up.value.keyboard.scan -ne [int]$filename[$index] -or $down.value.keyboard.flags -ne 4 -or $up.value.keyboard.flags -ne 6){throw 'Picker Unicode key serialization mismatch'}
  }
  if([DevboxInstallerAutomation]::PickerKeyRelease($typed,0).Length -ne 0){throw 'No filename dispatch must send no key releases'}
  $release=[DevboxInstallerAutomation]::PickerKeyRelease($typed,1)
  if($release.Length -ne 2 -or $release[0].value.keyboard.key -ne 65 -or $release[1].value.keyboard.key -ne 17 -or @($release | Where-Object {$_.value.keyboard.flags -ne 2}).Count -ne 0){throw 'Partial chord release mismatch'}
  $release=[DevboxInstallerAutomation]::PickerKeyRelease($typed,5)
  if($release.Length -ne 3 -or $release[2].value.keyboard.scan -ne [int]$filename[0] -or $release[2].value.keyboard.flags -ne 6){throw 'Partial Unicode release mismatch'}
  foreach($invalid in @('',("a"*32768),("a"+[char]0+"b"))) {
    $rejected=$false
    try {[DevboxInstallerAutomation]::PickerTextInput($invalid) | Out-Null} catch {$rejected=$true}
    if(-not $rejected){throw 'Unbounded picker text accepted'}
  }
  # Never set hosted variables or execute SendInput in this local test.
  if($env:GITHUB_ACTIONS -ceq 'true' -or $env:RUNNER_ENVIRONMENT -ceq 'github-hosted'){throw 'Local guard test requires ordinary local environment'}
  foreach($action in @('ZoomIn','ZoomReset')) {
    $blocked=$false
    try {& ([scriptblock]::Create($content)) -TargetProcessId 1 -ExpectedExecutable 'unused.exe' -ExpectedStartTimeUtc 'unused' -FixtureRoot 'unused' -Action $action -ProductWindow workspace} catch {
      if($_.Exception.Message -notmatch 'Native zoom requires GitHub hosted runner'){throw};$blocked=$true
    }
    if(-not $blocked){throw 'Local native zoom was not blocked'}
  }
  Write-Output 'Native zoom C# compile/native hierarchy selection/INPUT layout/allowlist key serialization/finally releases/local hosted guard: PASS (no native input)'
  return
}
$fixture=Join-Path ([IO.Path]::GetTempPath()) ('devbox-installer-uia-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixture | Out-Null
$image=Join-Path $fixture $(if($WorkspaceAuxiliary){'devbox-workspace.exe'}elseif($FrameworkHelpers){'devbox-control-center.exe'}else{'owned-classic-button.exe'})
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
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern IntPtr DefWindowProc(IntPtr h,uint m,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] static extern int GetMessage(out MSG m,IntPtr h,uint min,uint max);
 [DllImport("user32.dll")] static extern bool TranslateMessage(ref MSG m);
 [DllImport("user32.dll")] static extern IntPtr DispatchMessage(ref MSG m);
 [DllImport("user32.dll")] static extern void PostQuitMessage(int code);
 [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr h,int command);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern bool SetWindowText(IntPtr h,string text);
 static IntPtr helper,modal,terminal;
 static Proc callback=Window;
 static string marker;
 static IntPtr Window(IntPtr h,uint m,IntPtr w,IntPtr l) {
  if(m==0x111 && (w.ToInt64()&65535)==106) { SetWindowText(terminal,"Unexpected auxiliary"); return IntPtr.Zero; }
  if(m==0x111 && (w.ToInt64()&65535)==107) { SetWindowText(terminal,"Devbox Workspace · 터미널"); return IntPtr.Zero; }
  if(m==0x111 && (w.ToInt64()&65535)==102) { ShowWindow(helper,5); return IntPtr.Zero; }
  if(m==0x111 && (w.ToInt64()&65535)==104) { ShowWindow(modal,5); return IntPtr.Zero; }
  if(m==0x111 && (w.ToInt64()&65535)==105) { ShowWindow(modal,0); return IntPtr.Zero; }
  if(m==0x111 && (w.ToInt64()&65535)==103) { ShowWindow(helper,0); return IntPtr.Zero; }
  if(m==0x111 && (w.ToInt64()&65535)==101) { File.WriteAllText(marker,"owned-button-invoked"); return IntPtr.Zero; }
  if(m==0x10 && h==terminal) { ShowWindow(terminal,0); return IntPtr.Zero; }
  if(m==0x10) { PostQuitMessage(0); return IntPtr.Zero; }
  return DefWindowProc(h,m,w,l);
 }
 public static void Main(string[] args) {
  marker=args[0]; bool framework=args.Length>1; bool auxiliary=args.Length>1 && args[1]=="auxiliary"; WC cls=new WC();cls.proc=callback;cls.name=framework?"Tauri Window":"DevboxOwnedClassic";
  if(RegisterClass(ref cls)==0) throw new Exception("RegisterClass failed");
  IntPtr parent=CreateWindowEx(0,cls.name,auxiliary?"Devbox Workspace":"Devbox owned classic fixture",0x10CF0000,100,100,320,180,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero);
  if(parent==IntPtr.Zero) throw new Exception("Parent creation failed");
  helper=CreateWindowEx(0,cls.name,auxiliary?"Devbox Workspace · 터미널":"Devbox owned hidden helper",0x00CF0000,450,100,320,180,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero);
  if(helper==IntPtr.Zero) throw new Exception("Helper creation failed");
  if(framework) {
   foreach(string name in new string[]{"Tao Thread Event Target","com.devbox.v08."+(auxiliary?"workspace":"controlcenter")+".i"+new string('a',64)+"-sic"}) {
    WC extra=new WC();extra.proc=callback;extra.name=name;
    if(RegisterClass(ref extra)==0 || CreateWindowEx(0,name,"",0x10CF0000,800,100,100,100,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero)==IntPtr.Zero) throw new Exception("Visible framework helper creation failed");
   }
  }
  if(auxiliary) terminal=CreateWindowEx(0,cls.name,"Devbox Workspace · 터미널",0x10CF0000,450,100,320,180,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero);
  modal=CreateWindowEx(0,"#32770","Owned modal review",0x00CF0000,450,300,320,180,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero);
  if(modal==IntPtr.Zero) throw new Exception("Modal creation failed");
  CreateWindowEx(0,"BUTTON","Show modal",0x50000000,20,105,130,25,parent,new IntPtr(104),IntPtr.Zero,IntPtr.Zero);
  CreateWindowEx(0,"BUTTON","Show helper",0x50000000,20,70,130,30,parent,new IntPtr(102),IntPtr.Zero,IntPtr.Zero);
  CreateWindowEx(0,"BUTTON","Hide helper",0x50000000,165,70,130,30,parent,new IntPtr(103),IntPtr.Zero,IntPtr.Zero);
  if(CreateWindowEx(0,"BUTTON","Owned invoke",0x50000000,20,20,160,40,parent,new IntPtr(101),IntPtr.Zero,IntPtr.Zero)==IntPtr.Zero) throw new Exception("Button creation failed");
  MSG msg;while(GetMessage(out msg,IntPtr.Zero,0,0)>0){TranslateMessage(ref msg);DispatchMessage(ref msg);}
 }
}
'@
Add-Type -TypeDefinition $source -OutputAssembly $image -OutputType $(if($FrameworkHelpers -or $WorkspaceAuxiliary){'WindowsApplication'}else{'ConsoleApplication'})
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class OwnedFixtureWindows {
 delegate bool EnumProc(IntPtr h,IntPtr l);
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback,IntPtr l);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h,System.Text.StringBuilder text,int count);
 [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h,uint message,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
 public static void AssertHiddenHelper(int pid) {
  int main=0,hidden=0;
  EnumWindows(delegate(IntPtr h,IntPtr l) { uint owner; GetWindowThreadProcessId(h,out owner); var text=new System.Text.StringBuilder(256);GetWindowText(h,text,256);
   if(owner==pid && text.ToString()=="Devbox owned classic fixture" && IsWindowVisible(h)) main++;
   if(owner==pid && text.ToString()=="Devbox owned hidden helper" && !IsWindowVisible(h)) hidden++;
   return true; },IntPtr.Zero);
  if(main!=1 || hidden!=1) throw new Exception("Exact owned main and hidden helper were not created");
 }
 public static void HideFixtureHelper(int pid,int controlId) {
  IntPtr main=IntPtr.Zero; int count=0;
  EnumWindows(delegate(IntPtr h,IntPtr l) { uint owner; GetWindowThreadProcessId(h,out owner); var text=new System.Text.StringBuilder(256);GetWindowText(h,text,256);
   if(owner==pid && (text.ToString()=="Devbox owned classic fixture" || text.ToString()=="Devbox Workspace")) { main=h;count++; } return true; },IntPtr.Zero);
  if(count!=1) throw new Exception("Fixture main identity ambiguous");
  PostMessage(main,0x111,new IntPtr(controlId),IntPtr.Zero);
  System.Threading.Thread.Sleep(200);
 }
}
'@
$fixtureArguments=@($marker)
if($WorkspaceAuxiliary){$fixtureArguments+='auxiliary'}elseif($FrameworkHelpers){$fixtureArguments+='framework'}
$child=Start-Process -FilePath $image -ArgumentList $fixtureArguments -PassThru
try {
  $started=$child.StartTime.ToUniversalTime().ToString('o')
  if($child.MainModule.FileName -cne $image){throw 'Fixture executable identity mismatch'}
  $arguments=@{TargetProcessId=$child.Id;ExpectedExecutable=$image;ExpectedStartTimeUtc=$started;FixtureRoot=$fixture}
  if($WorkspaceAuxiliary){$arguments.ProductWindow='workspace';$arguments.AuxiliaryWindow='workspace-terminal'}elseif($FrameworkHelpers){$arguments.ProductWindow='control-center'}
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
  if($WorkspaceAuxiliary) {
    Start-Sleep -Milliseconds 500
    if($BaselineScript){
      $arguments.Remove('AuxiliaryWindow')
      $arguments.WindowName='Devbox Workspace · 터미널'
      Invoke-OwnedAction Close '' $BaselineScript | Out-Null
      throw 'Baseline unexpectedly accepted product lifecycle title override'
    }
    $view=Invoke-OwnedAction Inspect | ConvertFrom-Json
    Write-Output ('Auxiliary root selection: '+($view | Select-Object windowCount,selectedWindowCount,windows | ConvertTo-Json -Depth 5 -Compress))
    if($view.selectedWindowCount -ne 1 -or $view.name -cne 'Devbox Workspace · 터미널'){throw 'Exact terminal not selected'}
    $arguments.Remove('AuxiliaryWindow')
    $main=Invoke-OwnedAction Inspect | ConvertFrom-Json
    if($main.selectedWindowCount -ne 2){throw 'Main lifecycle strictness changed'}
    $arguments.AuxiliaryWindow='workspace-terminal'
    foreach($pair in @(@(102,103),@(104,105),@(106,107))) {
      [OwnedFixtureWindows]::HideFixtureHelper($child.Id,$pair[0])
      $rejected=$false
      try {Invoke-OwnedAction Close} catch {if($_.Exception.Message -notmatch 'Expected one owned top-level window'){throw};$rejected=$true}
      if(-not $rejected -or $child.HasExited){throw 'Duplicate terminal or modal accepted'}
      [OwnedFixtureWindows]::HideFixtureHelper($child.Id,$pair[1])
    }
    Invoke-OwnedAction Close
    Start-Sleep -Milliseconds 200
    if($child.HasExited){throw 'Auxiliary Close exited main process'}
    $view=Invoke-OwnedAction Inspect | ConvertFrom-Json
    if($view.selectedWindowCount -ne 0){throw 'Missing terminal accepted main or wrong window'}
    $arguments.Remove('AuxiliaryWindow')
    Invoke-OwnedAction Close
    if(-not $child.WaitForExit(5000)){throw 'Main close failed'}
    Write-Output 'Workspace terminal exact selection/Close; duplicate terminal/modal/wrong-title/missing terminal rejection; strict main: PASS'
    return
  }
  if($BaselineScript) {
    $baseline=Join-Path $fixture 'baseline-inspector.ps1'
    Copy-Item -LiteralPath $BaselineScript -Destination $baseline
    $deadline=[DateTime]::UtcNow.AddSeconds(5)
    do {$child.Refresh();Start-Sleep -Milliseconds 100} while($child.MainWindowHandle -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
    $view=Invoke-OwnedAction Inspect '' $baseline | ConvertFrom-Json
    $raw=@($view.controls | Where-Object {$_.className -eq 'Button' -and $_.id -eq '101'})
    if($raw.Count -ne 1 -or $raw[0].controlTypeId -ne 50033 -or $raw[0].canInvoke -ne $false){throw 'Baseline did not reproduce classic Pane without InvokePattern'}
    Write-Output 'Original classic native Button: Pane/no InvokePattern reproduced'
  }
  if($FrameworkHelpers) {
    $frameworkView=Invoke-OwnedAction Inspect | ConvertFrom-Json
    Write-Output ('Framework root selection: '+($frameworkView | Select-Object windowCount,selectedWindowCount,windows,nativeWindowCount,nativeWindows | ConvertTo-Json -Depth 5 -Compress))
    if($frameworkView.selectedWindowCount -ne 1){throw 'Visible framework helpers prevented exact main selection'}
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
  [OwnedFixtureWindows]::AssertHiddenHelper($child.Id)
  $native=Invoke-OwnedAction Inspect | ConvertFrom-Json
  $nativeFixture=@($native.nativeWindows | Where-Object {$_.className -ceq $(if($FrameworkHelpers){'Tauri Window'}else{'DevboxOwnedClassic'})})
  if($nativeFixture.Count -ne 2 -or @($nativeFixture | Where-Object visible).Count -ne 1 -or $native.nativeWindowCount -gt 32){throw 'Bounded native inventory did not retain exact main and hidden helper'}
  Write-Output ('Native owned main/hidden inventory: '+($nativeFixture | ConvertTo-Json -Compress))
  Invoke-OwnedAction Minimize
  $view=Invoke-OwnedAction Inspect | ConvertFrom-Json
  if($view.selectedWindowCount -ne 1 -or -not $view.selectedWindow.minimized -or -not $view.selectedWindow.visible -or $view.selectedWindow.nativeProcessId -ne $child.Id){throw ('Minimized owned main was not selected: '+($view | ConvertTo-Json -Depth 5 -Compress))}
  Write-Output ('Minimized selection metadata: '+($view.windows | ConvertTo-Json -Compress))
  Invoke-OwnedAction Activate
  $restored=Invoke-OwnedAction Inspect | ConvertFrom-Json
  if($restored.selectedWindow.minimized -or -not $restored.selectedWindow.visible){throw 'Activate did not restore owned main'}
  Invoke-OwnedAction Invoke 102
  $ambiguous=Invoke-OwnedAction Inspect | ConvertFrom-Json
  if($ambiguous.selectedWindowCount -ne 2){throw 'Two visible owned windows were not reported'}
  $rejected=$false
  try { Invoke-OwnedAction Close } catch {
    if($_.Exception.Message -notmatch 'Expected one owned top-level window' -or $_.Exception.Message -notmatch 'nativeProcessId'){throw}
    $rejected=$true
  }
  if(-not $rejected -or $child.HasExited){throw 'Ambiguous visible windows must reject Close'}
  # The fixture itself hides its second visible window; production selection remains strict.
  [OwnedFixtureWindows]::HideFixtureHelper($child.Id,103)
  Invoke-OwnedAction Invoke 104
  $modalView=Invoke-OwnedAction Inspect | ConvertFrom-Json
  if($modalView.selectedWindowCount -ne 2 -or @($modalView.windows | Where-Object {$_.className -ceq '#32770' -and $_.visible}).Count -ne 1){throw 'Visible native modal was ignored by main selection'}
  $modalRejected=$false
  try { Invoke-OwnedAction Close } catch {
    if($_.Exception.Message -notmatch 'Expected one owned top-level window'){throw}
    $modalRejected=$true
  }
  if(-not $modalRejected -or $child.HasExited){throw 'Modal must block lifecycle Close'}
  [OwnedFixtureWindows]::HideFixtureHelper($child.Id,105)
  Invoke-OwnedAction Close
  if(-not $child.WaitForExit(5000)){throw 'Owned fixture normal close failed'}
  Write-Output 'Classic native Button Invoke; hidden helper/main ownership, minimized Activate, ambiguous visible/modal Close rejection: PASS'
} finally {
  if(-not $child.HasExited){
    if($child.MainModule.FileName -cne $image -or $child.StartTime.ToUniversalTime().ToString('o') -cne $started){throw 'Fixture cleanup ownership changed'}
    $child.Kill();$child.WaitForExit()
  }
  Remove-Item -LiteralPath $fixture -Recurse -Force
}
