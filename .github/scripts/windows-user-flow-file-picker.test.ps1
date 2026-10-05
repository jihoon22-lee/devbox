# Actual rfd-equivalent COM save and WinForms open choosers; owned temporary fixture only.
param([string]$Driver=(Join-Path $PSScriptRoot 'windows-installer-ui.ps1'),[ValidateSet('Both','SaveFile','ChooseFile')][string]$Mode='Both',[switch]$ReadOnly)
$ErrorActionPreference = 'Stop'
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('devbox-save-picker-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixture | Out-Null
$image = Join-Path $fixture 'owned-save-picker.exe'
$marker = Join-Path $fixture 'selected.txt'
$output = Join-Path $fixture 'saved.json'
$source = @'
using System;
using System.IO;
using System.Windows.Forms;
[System.Runtime.InteropServices.ComImport, System.Runtime.InteropServices.Guid("84bccd23-5fde-4cdb-aea4-af64b83d78ab"), System.Runtime.InteropServices.InterfaceType(System.Runtime.InteropServices.ComInterfaceType.InterfaceIsIUnknown)]
interface RfdSaveDialog {
 [System.Runtime.InteropServices.PreserveSig] int Show(IntPtr owner);
 void SetFileTypes(uint count, [System.Runtime.InteropServices.MarshalAs(System.Runtime.InteropServices.UnmanagedType.LPArray, SizeParamIndex=0)] RfdFilter[] filters);
 void SetFileTypeIndex(uint index); void GetFileTypeIndex(out uint index);
 void Advise(IntPtr events, out uint cookie); void Unadvise(uint cookie);
 void SetOptions(uint options); void GetOptions(out uint options);
 void SetDefaultFolder(IntPtr item); void SetFolder(IntPtr item); void GetFolder(out IntPtr item); void GetCurrentSelection(out IntPtr item);
 void SetFileName([System.Runtime.InteropServices.MarshalAs(System.Runtime.InteropServices.UnmanagedType.LPWStr)] string name);
 void GetFileName(out IntPtr name); void SetTitle([System.Runtime.InteropServices.MarshalAs(System.Runtime.InteropServices.UnmanagedType.LPWStr)] string title);
 void SetOkButtonLabel(IntPtr label); void SetFileNameLabel(IntPtr label);
 void GetResult(out RfdShellItem item); void AddPlace(IntPtr item, uint placement);
 void SetDefaultExtension([System.Runtime.InteropServices.MarshalAs(System.Runtime.InteropServices.UnmanagedType.LPWStr)] string extension);
 void Close(int result); void SetClientGuid(ref Guid guid); void ClearClientData(); void SetFilter(IntPtr filter);
}
[System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential, CharSet=System.Runtime.InteropServices.CharSet.Unicode)]
struct RfdFilter {
 [System.Runtime.InteropServices.MarshalAs(System.Runtime.InteropServices.UnmanagedType.LPWStr)] public string name;
 [System.Runtime.InteropServices.MarshalAs(System.Runtime.InteropServices.UnmanagedType.LPWStr)] public string specification;
}
[System.Runtime.InteropServices.ComImport, System.Runtime.InteropServices.Guid("43826d1e-e718-42ee-bc55-a1e261c37bfe"), System.Runtime.InteropServices.InterfaceType(System.Runtime.InteropServices.ComInterfaceType.InterfaceIsIUnknown)]
interface RfdShellItem {
 void BindToHandler(IntPtr context, ref Guid handler, ref Guid iid, out IntPtr value);
 void GetParent(out IntPtr parent); void GetDisplayName(uint kind, out IntPtr name);
 void GetAttributes(uint mask, out uint attributes); void Compare(IntPtr other, uint hint, out int order);
}
public static class OwnedSavePicker {
 [System.Runtime.InteropServices.DllImport("ole32.dll")] static extern int CoInitializeEx(IntPtr reserved, uint flags);
 [System.Runtime.InteropServices.DllImport("ole32.dll")] static extern void CoUninitialize();
 static string SaveLikeRfd() {
  string selected = null;
  Exception failure = null;
  var worker = new System.Threading.Thread(() => {
   object instance = null;
   RfdShellItem result = null;
   IntPtr text = IntPtr.Zero;
   int initialized = CoInitializeEx(IntPtr.Zero, 2 | 4); // rfd: STA | DISABLE_OLE1DDE
   try {
    System.Runtime.InteropServices.Marshal.ThrowExceptionForHR(initialized);
    instance = Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("c0b4e2f3-ba21-4773-8dba-335ec946eb8b")));
    var dialog = (RfdSaveDialog)instance;
    dialog.SetDefaultExtension("json");
    dialog.SetFileTypes(1, new[] {new RfdFilter {name="JSON", specification="*.json"}});
    dialog.SetFileName("grpc-exchange.json");
    // Production supplies neither starting directory nor parent/options override.
    int shown = dialog.Show(IntPtr.Zero);
    if (shown < 0) { System.Runtime.InteropServices.Marshal.ThrowExceptionForHR(shown); }
    dialog.GetResult(out result);
    result.GetDisplayName(0x80058000, out text); // SIGDN_FILESYSPATH
    selected = System.Runtime.InteropServices.Marshal.PtrToStringUni(text);
   } catch(Exception error) { failure = error; }
   finally {
    if(text != IntPtr.Zero) System.Runtime.InteropServices.Marshal.FreeCoTaskMem(text);
    if(result != null) System.Runtime.InteropServices.Marshal.ReleaseComObject(result);
    if(instance != null) System.Runtime.InteropServices.Marshal.ReleaseComObject(instance);
    if(initialized >= 0) CoUninitialize();
   }
  });
  // CLR initializes a managed worker as MTA unless STA is selected before start.
  worker.SetApartmentState(System.Threading.ApartmentState.STA);
  worker.Start(); worker.Join();
  if(failure != null) throw failure;
  return selected;
 }
 [STAThread] public static void Main(string[] args) {
  if(args[2] == "SaveFile") {
   string selected = SaveLikeRfd();
   string requested = Path.Combine(args[0], "saved.json");
   if(!String.Equals(selected, requested, StringComparison.OrdinalIgnoreCase)) {
    // Observe COM selection without writing its possibly unrelated destination.
    File.WriteAllText(args[1], "selection-mismatch;absolute=" + Path.IsPathRooted(selected) +
     ";defaultName=" + String.Equals(Path.GetFileName(selected), "grpc-exchange.json", StringComparison.OrdinalIgnoreCase) +
     ";requestedParent=" + String.Equals(Path.GetDirectoryName(selected), args[0], StringComparison.OrdinalIgnoreCase));
    throw new InvalidOperationException("COM selected path differs from exact owned requested path");
   }
   File.WriteAllText(selected,"owned-save-fixture");
   File.WriteAllText(args[1],selected);
   return;
  }
  using (FileDialog dialog = args[2] == "SaveFile" ? (FileDialog)new SaveFileDialog() : new OpenFileDialog()) {
   dialog.AutoUpgradeEnabled = true;
   dialog.InitialDirectory = args[0];
   dialog.Filter = "JSON files|*.json";
   if (args[2] == "SaveFile") dialog.FileName = "grpc-exchange.json";
   dialog.Title = "Devbox owned save fixture";
   if (dialog.ShowDialog() == DialogResult.OK) {
    if (args[2] == "SaveFile") File.WriteAllText(dialog.FileName,"owned-save-fixture");
    File.WriteAllText(args[1],dialog.FileName);
   }
  }
 }
}
'@
$child = $null
$primaryFailure = $null
try {
 Add-Type -TypeDefinition $source -ReferencedAssemblies System.Windows.Forms -OutputAssembly $image -OutputType WindowsApplication
 Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
 Add-Type -ReferencedAssemblies UIAutomationClient,UIAutomationTypes -TypeDefinition @'
using System.Runtime.CompilerServices;
using System.Windows.Automation;
public static class SaveFixtureAutomation {
 [MethodImpl(MethodImplOptions.NoInlining)] public static void Initialize() {
  ClientSettings.RegisterClientSideProviders(new ClientSideProviderDescription[0]);
 }
}
'@
 [SaveFixtureAutomation]::Initialize()
 $actions=if($Mode -eq 'Both'){@('SaveFile','ChooseFile')}else{@($Mode)}
 foreach($action in $actions) {
 if($action -eq 'ChooseFile' -and -not (Test-Path -LiteralPath $output)){[IO.File]::WriteAllText($output,'owned-save-fixture')}
 $marker = Join-Path $fixture ($action + '-selected.txt')
 $child = Start-Process -FilePath $image -ArgumentList @(('"'+$fixture+'"'),('"'+$marker+'"'),$action) -PassThru
 $started = $child.StartTime.ToUniversalTime().ToString('o')
 $deadline = [DateTime]::UtcNow.AddSeconds(15)
 $before = @(Get-ChildItem -LiteralPath $fixture -File).Count
 do {
  $observed = (& $Driver -TargetProcessId $child.Id -ExpectedExecutable $image -ExpectedStartTimeUtc $started -FixtureRoot $fixture -Action InspectFilePicker) | ConvertFrom-Json
  if($observed.pickerCount -eq 1 -and $observed.fieldCount -eq 1 -and $observed.filenameReady){break}
  Start-Sleep -Milliseconds 100
 } while([DateTime]::UtcNow -lt $deadline)
 Write-Output ($action + ' initial controls: ' + ($observed | ConvertTo-Json -Compress))
 if($ReadOnly) {
  $ownedCondition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$child.Id)
  $ownedWindows=[System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,$ownedCondition)
  $idCondition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1148')
  $structure=@($ownedWindows | ForEach-Object { $_.FindAll([System.Windows.Automation.TreeScope]::Descendants,$idCondition) } | ForEach-Object {
   $parent=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($_)
   @{id=$_.Current.AutomationId;type=$_.Current.ControlType.ProgrammaticName;class=$_.Current.ClassName;hasHandle=($_.Current.NativeWindowHandle -ne 0);parentId=$parent.Current.AutomationId;parentType=$parent.Current.ControlType.ProgrammaticName;parentClass=$parent.Current.ClassName}
  })
  Write-Output ($structure | ConvertTo-Json -Compress)
  break
 }
 if($observed.pickerCount -ne 1 -or $observed.fieldCount -ne 1 -or $observed.confirmCount -ne 1 -or -not $observed.filenameReady){throw 'Owned picker input controls were not ready'}
 if($child.HasExited -or (Test-Path -LiteralPath $marker) -or @(Get-ChildItem -LiteralPath $fixture -File).Count -ne $before){throw 'Readonly picker observation changed fixture state'}
 try {
  $receipt = (& $Driver -TargetProcessId $child.Id -ExpectedExecutable $image -ExpectedStartTimeUtc $started -FixtureRoot $fixture -Action $action -FilePath $output) | ConvertFrom-Json
  if(-not $receipt.filenameMatched -or -not $receipt.chooserClosed){throw 'Owned exact filename/chooser receipt missing'}
 } catch {
  Write-Output ($action + ' mutation failure state: ' + (@{childExited=$child.HasExited;markerExists=(Test-Path -LiteralPath $marker);outputExists=(Test-Path -LiteralPath $output)} | ConvertTo-Json -Compress))
  throw
 }
 if(-not $child.WaitForExit(10000)){throw 'Owned save chooser did not complete'}
 if((Test-Path -LiteralPath $marker) -and [IO.File]::ReadAllText($marker).StartsWith('selection-mismatch;')){Write-Output ([IO.File]::ReadAllText($marker))}
 if($child.ExitCode -ne 0 -or [IO.File]::ReadAllText($marker) -ne $output -or [IO.File]::ReadAllText($output) -ne 'owned-save-fixture'){throw 'Owned save selection/output mismatch'}
 Write-Output ('Actual owned ' + $action + ' chooser and exact selected output: PASS')
 }
} catch {
 $primaryFailure = $_
 throw
} finally {
 $cleanupFailure = $null
 try {
  if($child -and -not $child.HasExited){
   $current = [Diagnostics.Process]::GetProcessById($child.Id)
   if($current.MainModule.FileName -ne $image -or $current.StartTime.ToUniversalTime().ToString('o') -ne $started){throw 'Owned fixture cleanup identity changed'}
   $current.Kill()
   if(-not $current.WaitForExit(5000)){throw 'Owned fixture termination unconfirmed'}
  }
  Remove-Item -LiteralPath $fixture -Recurse -Force
 } catch {
  $cleanupFailure = $_
 }
 if($cleanupFailure){
  # Keep an active/unconfirmed fixture and the original assertion failure.
  Write-Output 'Owned picker fixture cleanup: FAIL (fixture preserved if removal incomplete)'
  if(-not $primaryFailure){throw 'Owned picker fixture cleanup failed'}
 }
}
