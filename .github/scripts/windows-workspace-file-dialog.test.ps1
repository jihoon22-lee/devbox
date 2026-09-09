# Exercise the exact chooser driver against an owned, disposable native dialog.
# This verifies the test tool; packaged Workspace acceptance remains separate.
param([string]$Driver=(Join-Path $PSScriptRoot 'windows-workspace-file-dialog.ps1'))
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$fixtureRoot=Join-Path ([IO.Path]::GetTempPath()) ('devbox-file-dialog-'+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
$exe=Join-Path $fixtureRoot 'NativeDialogProbe.exe'
$chosen=Join-Path $fixtureRoot '한글 selected.txt'
[IO.File]::WriteAllText($chosen,'synthetic chooser fixture')
$secondChosen=Join-Path $fixtureRoot '한글 second.txt'
[IO.File]::WriteAllText($secondChosen,'synthetic multi-file fixture')
$code=@'
using System;
using System.IO;
using System.Windows.Forms;
public static class NativeDialogProbe {
    [STAThread]
    public static int Main(string[] args) {
        if(args.Length!=2) return 3;
        Application.EnableVisualStyles();
        using(var dialog=new OpenFileDialog()) {
            dialog.InitialDirectory=args[0];
            dialog.Multiselect=true;
            if(dialog.ShowDialog()!=DialogResult.OK) return 2;
            File.WriteAllLines(args[1],dialog.FileNames);
            return 0;
        }
    }
}
'@
$process=$null
try {
    Add-Type -TypeDefinition $code -ReferencedAssemblies System.Windows.Forms,System.Drawing -OutputAssembly $exe -OutputType WindowsApplication
    foreach($action in @('Cancel','Open','Multi')) {
        $result=Join-Path $fixtureRoot ('result-'+$action+'.txt')
        $process=Start-Process -FilePath $exe -ArgumentList ('"'+$fixtureRoot+'" "'+$result+'"') -PassThru
        $null=$process.Handle
        $process.Refresh()
        $driverAction=if($action -eq 'Multi'){'Open'}else{$action}
        $selectionJson=if($action -eq 'Multi'){@($chosen,$secondChosen)|ConvertTo-Json -Compress}else{@($chosen)|ConvertTo-Json -Compress}
        & $Driver -TargetProcessId $process.Id -ExpectedExecutable $exe -FixtureRoot $fixtureRoot -Action $driverAction -SelectedFilesJson $selectionJson
        if(-not $process.WaitForExit(5000)){throw 'Fixture chooser did not close'}
        $expectedExit=if($action -eq 'Cancel'){2}else{0}
        if($process.ExitCode -ne $expectedExit){throw 'Fixture chooser returned an unexpected exit code'}
        if($action -ne 'Cancel') {
            if(-not [IO.File]::Exists($result)){throw 'Fixture chooser did not publish its selection'}
            $actual=@([IO.File]::ReadAllLines($result)|ForEach-Object{[WorkspaceFixturePath]::Canonical($_)})
            $expected=if($action -eq 'Multi'){@($chosen,$secondChosen)}else{@($chosen)}
            if($actual.Count -ne @($expected).Count){throw 'Fixture chooser selected a different number of files'}
            foreach($file in $expected){if($actual -notcontains [WorkspaceFixturePath]::Canonical($file)){throw 'Fixture chooser selected a different file'}}
        } elseif([IO.File]::Exists($result)) {throw 'Cancelled fixture chooser published a file'}
        Write-Output ('Native owned chooser '+$action+': PASS')
        $process.Dispose()
        $process=$null
    }
} finally {
    if($null -ne $process) {
        $process.Refresh()
        if(-not $process.HasExited) {
            if([IO.Path]::GetFullPath($process.Path) -ne [IO.Path]::GetFullPath($exe)){throw 'Fixture process identity changed'}
            $process.Kill()
            if(-not $process.WaitForExit(5000)){throw 'Fixture process termination unconfirmed'}
        }
        $process.Dispose()
    }
    if(([IO.File]::GetAttributes($fixtureRoot) -band [IO.FileAttributes]::ReparsePoint) -ne 0){throw 'Fixture root changed'}
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force
}
