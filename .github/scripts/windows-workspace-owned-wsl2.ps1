param([string]$Archive,[string]$Digest,[string]$AppArtifact,[string]$NodeScript,[string]$SourceSha,[string]$RunId,[string]$InstalledTargets)
$ErrorActionPreference='Stop'
$WslVersion=2
if ($Digest -notmatch '^[a-f0-9]{64}$' -or (Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Digest) {throw 'Owned fixture archive digest mismatch'}
$name='DevboxWorkspaceFixture-'+[guid]::NewGuid().ToString('N')
$directory=Join-Path ([IO.Path]::GetTempPath()) $name
New-Item -ItemType Directory -Path $directory | Out-Null
$marker=Join-Path $directory 'owner.txt'
[IO.File]::WriteAllText($marker,$name)
$wsl=Join-Path $env:SystemRoot 'System32/wsl.exe'
function Quoted([string]$Value) {
  if ($Value -match '["\r\n]') {throw 'Unexpected fixture argument'}
  if ($Value -match '\s') {return '"'+$Value+'"'}
  return $Value
}
function Run-Process([string]$Executable,[string[]]$Arguments,[int]$Milliseconds=30000,[string]$Owner='') {
  $info=New-Object Diagnostics.ProcessStartInfo
  $info.FileName=$Executable
  $info.WorkingDirectory=[Environment]::SystemDirectory
  $info.Arguments=($Arguments|ForEach-Object {Quoted $_}) -join ' '
  $info.UseShellExecute=$false
  $info.CreateNoWindow=$true
  $info.RedirectStandardOutput=$true
  $info.RedirectStandardError=$true
  if ($Owner) {$info.EnvironmentVariables['DEVBOX_WORKSPACE_LOCAL_OWNER']=$Owner}
  $process=New-Object Diagnostics.Process
  $process.StartInfo=$info
  if (-not $process.Start()) {throw 'Could not start owned fixture process'}
  $stdout=$process.StandardOutput.ReadToEndAsync()
  $stderr=$process.StandardError.ReadToEndAsync()
  $timedOut=-not $process.WaitForExit($Milliseconds)
  if ($timedOut) {$process.Kill();$process.WaitForExit()}
  $result=@{exit=$process.ExitCode;timedOut=$timedOut;output=$stdout.Result.Replace([string][char]0,'');error=$stderr.Result.Replace([string][char]0,'')}
  $process.Dispose()
  return $result
}
function Run-OwnedWsl([string[]]$Arguments,[int]$Milliseconds=30000) {
  if ($Arguments.Count -lt 2 -or $Arguments[0] -notin @('--import','--distribution','--terminate','--unregister') -or $Arguments[1] -ne $name) {throw 'Unexpected fixture command scope'}
  return Run-Process $wsl $Arguments $Milliseconds
}
function Registration {
  return @(Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Lxss' -ErrorAction SilentlyContinue | Where-Object {(Get-ItemProperty -LiteralPath $_.PSPath).DistributionName -eq $name})
}
$install=Join-Path $directory 'distribution'
try {
  if ((Registration).Count -ne 0) {throw 'Owned fixture name already registered'}
  $tar=Join-Path $directory $(if ($Archive.EndsWith('.gz')) {'rootfs.tar.gz'} else {'rootfs.tar'})
  Copy-Item -LiteralPath $Archive -Destination $tar
  $metadata=Get-Content -LiteralPath (Join-Path $AppArtifact 'resources/wsl/manifest.json') -Raw | ConvertFrom-Json
  $import=Run-OwnedWsl @('--import',$name,$install,$tar,'--version',[string]$WslVersion) 120000
  @{stage='import';wslVersion=$WslVersion;result=$import}|ConvertTo-Json -Compress
  if ($import.exit -ne 0) {throw 'Owned fixture import failed'}
  $registered=Registration
  if ($registered.Count -ne 1) {throw 'Owned fixture registration missing/ambiguous'}
  $properties=Get-ItemProperty -LiteralPath $registered[0].PSPath
  $actual=([string]$properties.BasePath).Replace('\\?\','').TrimEnd('\')
  if ($actual -ne $install) {throw 'Owned fixture storage differs from created directory'}
  $mode=if (($properties.Flags -band 8) -ne 0) {2} else {1}
  if ($mode -ne $WslVersion) {throw 'Owned fixture mode differs from requested mode'}
  $owner=Join-Path $directory 'owner.json'
  @{schema=1;name=$name;distroId=$registered[0].PSChildName.Trim('{}');version=$mode}|ConvertTo-Json | Set-Content -Encoding UTF8 -LiteralPath $owner
  $start=Run-OwnedWsl @('--distribution',$name,'--user','root','--cd','/','--exec','/usr/bin/mkdir','-p','/home/devbox-fixture')
  if ($start.exit -ne 0) {throw 'Owned fixture did not start'}
  # The digest-pinned Ubuntu fixture includes Git; service errors are not missing packages.
  $git=Run-OwnedWsl @('--distribution',$name,'--user','root','--cd','/','--exec','/usr/bin/git','--version')
  @{stage='owned-fixture-git';result=$git}|ConvertTo-Json -Compress
  if ($git.exit -ne 0) {throw 'Owned fixture Git provisioning failed'}
  # Explicit test tooling only in this newly registered, digest-verified distro.
  $updated=Run-OwnedWsl @('--distribution',$name,'--user','root','--cd','/','--exec','/usr/bin/apt-get','update') 180000
  if ($updated.exit -ne 0) {throw 'Owned fixture package index failed'}
  $tools=Run-OwnedWsl @('--distribution',$name,'--user','root','--cd','/','--exec','/usr/bin/env','DEBIAN_FRONTEND=noninteractive','/usr/bin/apt-get','install','-y','--no-install-recommends','git','python3','tmux','docker.io','busybox-static','ca-certificates') 300000
  if ($tools.exit -ne 0) {throw 'Owned fixture tooling installation failed'}
  $zellijArchive=Join-Path $directory 'zellij.tar.gz'
  Invoke-WebRequest -UseBasicParsing -TimeoutSec 120 -Uri 'https://github.com/zellij-org/zellij/releases/download/v0.43.1/zellij-x86_64-unknown-linux-musl.tar.gz' -OutFile $zellijArchive
  if ((Get-FileHash -LiteralPath $zellijArchive -Algorithm SHA256).Hash.ToLowerInvariant() -ne '541d98efef5558293ef85ad9acd29e4d920b6e881513b9e77255d8207020d75a') {throw 'Zellij fixture digest mismatch'}
  $linuxArchive=Run-OwnedWsl @('--distribution',$name,'--user','root','--cd','/','--exec','/usr/bin/wslpath','-u',$zellijArchive)
  if ($linuxArchive.exit -ne 0) {throw 'Owned fixture archive mapping failed'}
  $installScript=Join-Path $directory 'install-tools.py'
  @'
import hashlib,pathlib,subprocess,sys,tarfile,time
archive=pathlib.Path(sys.argv[1])
assert hashlib.sha256(archive.read_bytes()).hexdigest()=='541d98efef5558293ef85ad9acd29e4d920b6e881513b9e77255d8207020d75a'
with tarfile.open(archive) as source:
    member=source.getmember('zellij')
    assert member.isfile() and member.size<150_000_000
    target=pathlib.Path('/usr/local/bin/zellij')
    with target.open('xb') as destination: destination.write(source.extractfile(member).read())
    target.chmod(0o755)
if subprocess.run(['/usr/bin/docker','info'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode:
    log=open('/tmp/devbox-fixture-dockerd.log','xb')
    subprocess.Popen(['/usr/sbin/dockerd','--host=unix:///var/run/docker.sock','--storage-driver=vfs'],stdout=log,stderr=log,start_new_session=True)
    for _ in range(60):
        if subprocess.run(['/usr/bin/docker','info'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0: break
        time.sleep(.5)
    else: raise RuntimeError('Owned daemon unavailable')
for command in [['/usr/bin/tmux','-V'],['/usr/local/bin/zellij','--version'],['/usr/bin/docker','--version']]:
    subprocess.run(command,check=True)
'@ | Set-Content -Encoding utf8 -LiteralPath $installScript
  $linuxScript=Run-OwnedWsl @('--distribution',$name,'--user','root','--cd','/','--exec','/usr/bin/wslpath','-u',$installScript)
  if ($linuxScript.exit -ne 0) {throw 'Owned fixture setup mapping failed'}
  $provision=Run-OwnedWsl @('--distribution',$name,'--user','root','--cd','/','--exec','/usr/bin/python3',$linuxScript.output.Trim(),$linuxArchive.output.Trim()) 90000
  @{stage='owned-fixture-tooling';result=$provision}|ConvertTo-Json -Compress
  if ($provision.exit -ne 0) {throw 'Owned fixture tool setup failed'}
  $watch=[Diagnostics.Stopwatch]::StartNew()
  $node=(Get-Command node.exe -ErrorAction Stop).Source
  $test=Run-Process $node @($NodeScript,$owner,$AppArtifact,$SourceSha,$RunId,$InstalledTargets) 600000
  $watch.Stop()
  @{stage='actual-windows-wsl2-workspace';elapsedSeconds=$watch.Elapsed.TotalSeconds;result=$test}|ConvertTo-Json -Depth 5 -Compress
  if ($test.exit -ne 0 -or $test.timedOut) {throw 'Owned WSL Runtime fixture failed'}
  $result=$test.output|ConvertFrom-Json
  if ($result.result -ne 'pass' -or -not $result.appExited -or -not $result.ownedDataRemoved -or -not $result.acceptanceComplete) {throw 'Owned WSL Runtime result or cleanup missing'}
} finally {
  $appExecutable=Join-Path $directory 'app/devbox-workspace.exe'
  $appOwner=Join-Path $directory 'app-owner.json'
  if (Test-Path -LiteralPath $appOwner) {
    if ([IO.File]::ReadAllText($marker) -ne $name) {throw 'Fixture marker changed before app cleanup'}
    $record=Get-Content -LiteralPath $appOwner -Raw | ConvertFrom-Json
    $hash=[Security.Cryptography.SHA256]::Create()
    try {$expectedId=([BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes('\\?\'+[IO.Path]::GetFullPath($appExecutable))))).Replace('-','').ToLowerInvariant()} finally {$hash.Dispose()}
    $expectedData=Join-Path $env:LOCALAPPDATA ('com.devbox.v08.workspace.i'+$expectedId)
    if ($record.executable -ne $appExecutable -or $record.installationId -ne $expectedId -or $record.dataRoot -ne $expectedData -or $record.preexisting -ne $false) {throw 'Owned app receipt does not match this fixture installation'}
    $apps=@(Get-CimInstance Win32_Process | Where-Object {$_.ExecutablePath -eq $appExecutable})
    foreach ($app in $apps) {
      $process=[Diagnostics.Process]::GetProcessById($app.ProcessId)
      try {
        $null=$process.Handle
        if ($process.MainModule.FileName -ne $appExecutable) {throw 'Owned app process changed'}
        & (Join-Path $env:SystemRoot 'System32/taskkill.exe') /PID $app.ProcessId /T /F | Out-Null
        if (-not $process.WaitForExit(10000)) {throw 'Owned app process did not retire'}
      } finally {$process.Dispose()}
    }
    if (Test-Path -LiteralPath $expectedData) {Remove-Item -LiteralPath $expectedData -Recurse -Force}
  }
  $owned=Registration
  if ($owned.Count -gt 1) {throw "Ambiguous owned fixture registration; retained $directory"}
  if ([IO.File]::ReadAllText($marker) -ne $name) {throw 'Owned fixture marker changed'}
  if ($owned.Count -eq 1) {
    $base=([string](Get-ItemProperty -LiteralPath $owned[0].PSPath).BasePath).Replace('\\?\','').TrimEnd('\')
    if ($base -ne $install) {throw 'Owned fixture storage changed; no cleanup attempted'}
    $terminated=Run-OwnedWsl @('--terminate',$name)
    $removed=Run-OwnedWsl @('--unregister',$name)
    if ($removed.exit -ne 0 -or (Registration).Count -ne 0) {throw "Owned fixture cleanup failed; retained $directory"}
  }
  Remove-Item -LiteralPath $directory -Recurse -Force
  @{stage='cleanup';ownedDistroAbsent=$true;ownedDirectoryRemoved=$true}|ConvertTo-Json -Compress
}
