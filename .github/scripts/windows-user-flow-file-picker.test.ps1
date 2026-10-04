# Actual modern Windows save chooser, owned temporary executable and output only.
param([string]$Driver=(Join-Path $PSScriptRoot 'windows-installer-ui.ps1'))
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
  using (var dialog = new SaveFileDialog()) {
   dialog.AutoUpgradeEnabled = true;
   dialog.InitialDirectory = args[0];
   dialog.Filter = "JSON files|*.json";
   dialog.Title = "Devbox owned save fixture";
   if (dialog.ShowDialog() == DialogResult.OK) {
    File.WriteAllText(dialog.FileName,"owned-save-fixture");
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
 $child = Start-Process -FilePath $image -ArgumentList @(('"'+$fixture+'"'),('"'+$marker+'"')) -PassThru
 $started = $child.StartTime.ToUniversalTime().ToString('o')
 $deadline = [DateTime]::UtcNow.AddSeconds(15)
 $condition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$child.Id)
 $hostCondition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'FileNameControlHost')
 do {
  $windows = [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,$condition)
  $hosts = @($windows | ForEach-Object { $_.FindAll([System.Windows.Automation.TreeScope]::Descendants,$hostCondition) })
  if($hosts.Count -eq 1){break}
  Start-Sleep -Milliseconds 100
 } while([DateTime]::UtcNow -lt $deadline)
 if($hosts.Count -ne 1){throw 'Modern owned filename host was not observed'}
 & $Driver -TargetProcessId $child.Id -ExpectedExecutable $image -ExpectedStartTimeUtc $started -FixtureRoot $fixture -Action SaveFile -FilePath $output
 if(-not $child.WaitForExit(10000)){throw 'Owned save chooser did not complete'}
 if($child.ExitCode -ne 0 -or [IO.File]::ReadAllText($marker) -ne $output -or [IO.File]::ReadAllText($output) -ne 'owned-save-fixture'){throw 'Owned save selection/output mismatch'}
 Write-Output 'Actual modern owned SaveFile chooser and exact output: PASS'
} finally {
 if($child -and -not $child.HasExited){
  $current = [Diagnostics.Process]::GetProcessById($child.Id)
  if($current.MainModule.FileName -eq $image -and $current.StartTime.ToUniversalTime().ToString('o') -eq $started){$current.Kill();$current.WaitForExit()}
 }
 Remove-Item -LiteralPath $fixture -Recurse -Force
}
