# Actual modern Windows save/open choosers, owned temporary executable and files only.
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
public static class OwnedSavePicker {
 [STAThread] public static void Main(string[] args) {
  using (FileDialog dialog = args[2] == "SaveFile" ? (FileDialog)new SaveFileDialog() : new OpenFileDialog()) {
   dialog.AutoUpgradeEnabled = true;
   dialog.InitialDirectory = args[0];
   dialog.Filter = "JSON files|*.json";
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
  & $Driver -TargetProcessId $child.Id -ExpectedExecutable $image -ExpectedStartTimeUtc $started -FixtureRoot $fixture -Action $action -FilePath $output
 } catch {
  Write-Output ($action + ' mutation failure state: ' + (@{childExited=$child.HasExited;markerExists=(Test-Path -LiteralPath $marker);outputExists=(Test-Path -LiteralPath $output)} | ConvertTo-Json -Compress))
  throw
 }
 if(-not $child.WaitForExit(10000)){throw 'Owned save chooser did not complete'}
 if($child.ExitCode -ne 0 -or [IO.File]::ReadAllText($marker) -ne $output -or [IO.File]::ReadAllText($output) -ne 'owned-save-fixture'){throw 'Owned save selection/output mismatch'}
 Write-Output ('Actual modern owned ' + $action + ' chooser and exact output: PASS')
 }
} finally {
 if($child -and -not $child.HasExited){
  $current = [Diagnostics.Process]::GetProcessById($child.Id)
  if($current.MainModule.FileName -eq $image -and $current.StartTime.ToUniversalTime().ToString('o') -eq $started){$current.Kill();$current.WaitForExit()}
 }
 Remove-Item -LiteralPath $fixture -Recurse -Force
}
