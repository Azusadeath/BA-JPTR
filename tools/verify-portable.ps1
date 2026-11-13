param([Parameter(Mandatory=$true)][string]$Portable,[Parameter(Mandatory=$true)][string]$WorkDir)
$ErrorActionPreference='Stop'
$packageRoot=[IO.Path]::GetFullPath($Portable)
$testRoot=[IO.Path]::GetFullPath($WorkDir)
if(Test-Path -LiteralPath $testRoot){throw 'Use a new test directory.'}
New-Item -ItemType Directory -Path $testRoot -Force|Out-Null
$copy=Join-Path $testRoot 'Portable copy'
Copy-Item -LiteralPath $packageRoot -Destination $copy -Recurse
if(Test-Path -LiteralPath (Join-Path $copy '.gui')){throw 'Distribution contains user settings.'}
$foreign=Join-Path $testRoot 'Unrelated working directory'
$profileRoot=Join-Path $testRoot 'Clean user'
foreach($folder in @($foreign,(Join-Path $profileRoot 'Temp'),(Join-Path $profileRoot 'AppData/Local'),(Join-Path $profileRoot 'AppData/Roaming'))){New-Item -ItemType Directory -Path $folder -Force|Out-Null}
function Set-CleanEnvironment($Start){
    $Start.EnvironmentVariables.Clear()
    $Start.EnvironmentVariables['SystemRoot']=$env:SystemRoot
    $Start.EnvironmentVariables['WINDIR']=$env:SystemRoot
    $Start.EnvironmentVariables['ComSpec']=Join-Path $env:SystemRoot 'System32/cmd.exe'
    $Start.EnvironmentVariables['PATH']=Join-Path $env:SystemRoot 'System32'
    $Start.EnvironmentVariables['TEMP']=Join-Path $profileRoot 'Temp'
    $Start.EnvironmentVariables['TMP']=Join-Path $profileRoot 'Temp'
    $Start.EnvironmentVariables['USERPROFILE']=$profileRoot
    $Start.EnvironmentVariables['LOCALAPPDATA']=Join-Path $profileRoot 'AppData/Local'
    $Start.EnvironmentVariables['APPDATA']=Join-Path $profileRoot 'AppData/Roaming'
}
$startupReport=Join-Path $testRoot 'gui-startup.json'
$start=[Diagnostics.ProcessStartInfo]::new()
$start.FileName=Join-Path $copy 'BA-JPTR.exe'
$start.WorkingDirectory=$foreign
$start.UseShellExecute=$false
$start.WindowStyle=[Diagnostics.ProcessWindowStyle]::Hidden
Set-CleanEnvironment $start
$start.EnvironmentVariables['BAJPTR_STARTUP_REPORT']=$startupReport
$launcher=[Diagnostics.Process]::Start($start)
if(-not $launcher.WaitForExit(10000) -or $launcher.ExitCode -ne 0){throw 'Portable launcher failed.'}
$timer=[Diagnostics.Stopwatch]::StartNew()
$modules=@()
while(-not(Test-Path -LiteralPath $startupReport) -and $timer.Elapsed.TotalSeconds -lt 30){
    if($modules.Count -eq 0){
        $children=@(Get-CimInstance Win32_Process -Filter "Name='bajptr-desktop.exe'"|Where-Object {$_.ExecutablePath -and $_.ExecutablePath.StartsWith($copy,[StringComparison]::OrdinalIgnoreCase)})
        if($children.Count -eq 1){try{$modules=@(([Diagnostics.Process]::GetProcessById($children[0].ProcessId)).Modules|Select-Object -ExpandProperty FileName)}catch{}}
    }
    Start-Sleep -Milliseconds 100
}
if(-not(Test-Path -LiteralPath $startupReport)){throw 'GUI startup probe did not complete.'}
$gui=Get-Content -LiteralPath $startupReport -Raw|ConvertFrom-Json
if(-not $gui.project_loaded -or $gui.failed){throw 'A completely empty portable project failed to load.'}
if($modules.Count -eq 0){throw 'No loaded-module evidence captured.'}
$externalModules=@($modules|Where-Object {-not $_.StartsWith($copy,[StringComparison]::OrdinalIgnoreCase) -and -not $_.StartsWith($env:SystemRoot,[StringComparison]::OrdinalIgnoreCase)})
if($externalModules.Count -gt 0){Write-Warning ('Non-Windows host modules detected; inspect direct imports and retest on a clean OS: '+($externalModules -join ', '))}
$manifest=Get-Content -LiteralPath (Join-Path $copy 'native/engine.json') -Raw|ConvertFrom-Json
$start=[Diagnostics.ProcessStartInfo]::new()
$start.FileName=Join-Path $copy ('native/'+$manifest.cli)
$start.Arguments='--desktop-stdio --data-dir "'+$copy+'"'
$start.WorkingDirectory=$foreign
$start.UseShellExecute=$false;$start.CreateNoWindow=$true
$start.RedirectStandardInput=$true;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true
$start.StandardOutputEncoding=[Text.Encoding]::UTF8
$start.StandardErrorEncoding=[Text.Encoding]::UTF8
Set-CleanEnvironment $start
$process=[Diagnostics.Process]::Start($start)
$writer=[IO.StreamWriter]::new($process.StandardInput.BaseStream,[Text.UTF8Encoding]::new($false))
$writer.AutoFlush=$true
$errors=$process.StandardError.ReadToEndAsync()
$script:sequence=0
function Invoke-Project([string]$Action,$Payload=@{}){
    $script:sequence+=1;$id=$script:sequence
    $writer.WriteLine((@{id=$id;action=$Action;payload=$Payload}|ConvertTo-Json -Compress -Depth 15))
    while($null -ne ($line=$process.StandardOutput.ReadLine())){
        $message=$line|ConvertFrom-Json
        if($message.id -eq $id -and $message.event -ne 'progress'){
            if($message.event -ne 'result'){throw ($message|ConvertTo-Json -Depth 15)}
            return $message.data
        }
    }
    throw 'Project worker exited.'
}
try{
    $loaded=Invoke-Project 'load'
    if($loaded.rules.Count -ne 0 -or @($loaded.errors.PSObject.Properties).Count -gt 0){throw 'Fresh portable project is not empty and healthy.'}
    $state=Invoke-Project 'save_rule' @{revision=0;index=$null;rule=@{old='Portable text';new='Portable result';mode='exact';source='';table=''}}
    if($state.rules.Count -ne 1){throw 'First auto-save failed.'}
    $export=Join-Path $testRoot 'export.tsv'
    $null=Invoke-Project 'export_rules' @{path=$export}
    if(-not(Test-Path -LiteralPath $export)){throw 'Portable export failed.'}
}finally{
    if(-not $process.HasExited){$writer.WriteLine('{"id":999,"action":"quit","payload":{}}');if(-not $process.WaitForExit(20000)){$process.Kill();throw 'Project worker did not stop.'}}
    $errorText=$errors.GetAwaiter().GetResult();$exit=$process.ExitCode;$process.Dispose();if($exit -ne 0){throw $errorText}
}
$report=@{passed=$true;test_kind='isolated process on current Windows, not a new OS image';environment_cleared=$true;fresh_project=$true;foreign_working_directory=$true;separate_user_directories=$true;launcher_passed=$true;gui=$gui;first_save_passed=$true;export_passed=$true;loaded_modules=$modules;module_audit_clean=($externalModules.Count -eq 0);external_non_windows_modules=$externalModules;package=$packageRoot}
[IO.File]::WriteAllText((Join-Path $testRoot 'portable-report.json'),($report|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
$report|ConvertTo-Json -Depth 5
