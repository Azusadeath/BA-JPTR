param([Parameter(Mandatory=$true)][string]$Portable,[Parameter(Mandatory=$true)][string]$WorkDir)
$ErrorActionPreference = 'Stop'
$portableRoot = [IO.Path]::GetFullPath($Portable)
$testRoot = [IO.Path]::GetFullPath($WorkDir)
if ((Test-Path -LiteralPath $testRoot) -and (Get-ChildItem -LiteralPath $testRoot -Force | Select-Object -First 1)) { throw 'Use a new or empty test directory.' }
$projectRoot = Split-Path $PSScriptRoot -Parent
$fixture = Join-Path $projectRoot 'crates/bajptr-project/tests/fixtures/v1'
$game = Join-Path $testRoot 'game'
$data = Join-Path $testRoot 'project'
$bundles = Join-Path $game 'BlueArchive_Data/StreamingAssets/TableBundles'
$catalog = Join-Path $bundles 'Catalog/TableCatalog.bytes'
New-Item -ItemType Directory -Path (Split-Path $catalog -Parent),(Join-Path $data '.gui') -Force | Out-Null
$before = [IO.File]::ReadAllBytes((Join-Path $fixture 'TableCatalog.bytes'))
$crc = [BitConverter]::ToInt64($before,50)
[IO.File]::WriteAllBytes($catalog,$before)
Copy-Item -LiteralPath (Join-Path $fixture 'TableCatalog.hash') -Destination (Join-Path (Split-Path $catalog -Parent) 'TableCatalog.hash')
Copy-Item -LiteralPath (Join-Path $fixture 'encrypted.db') -Destination (Join-Path $bundles "123456_$crc")
Copy-Item -LiteralPath (Join-Path $fixture 'rules.tsv') -Destination (Join-Path $data '.gui/rules.tsv')
$utf8 = [Text.UTF8Encoding]::new($false)
[IO.File]::WriteAllText((Join-Path $data '.gui/settings.json'),(@{format=2;game=$game;skip_kana=$true}|ConvertTo-Json),$utf8)
$manifest = Get-Content -LiteralPath (Join-Path $portableRoot 'native/engine.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$cli = Join-Path $portableRoot "native/$($manifest.cli)"
$start = [Diagnostics.ProcessStartInfo]::new()
$start.FileName = $cli
$start.Arguments = '--desktop-stdio --data-dir "' + $data + '"'
$start.WorkingDirectory = $portableRoot
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.RedirectStandardInput = $true
$start.RedirectStandardOutput = $true
$start.RedirectStandardError = $true
$start.StandardOutputEncoding = $utf8
$start.StandardErrorEncoding = $utf8
$cleanProfile=Join-Path $testRoot 'clean-user'
foreach($folder in @('Temp','AppData/Local','AppData/Roaming')){New-Item -ItemType Directory -Path (Join-Path $cleanProfile $folder) -Force|Out-Null}
$start.EnvironmentVariables.Clear()
$start.EnvironmentVariables['SystemRoot']=$env:SystemRoot
$start.EnvironmentVariables['WINDIR']=$env:SystemRoot
$start.EnvironmentVariables['ComSpec']=Join-Path $env:SystemRoot 'System32/cmd.exe'
$start.EnvironmentVariables['PATH'] = Join-Path $env:SystemRoot 'System32'
$start.EnvironmentVariables['TEMP']=Join-Path $cleanProfile 'Temp'
$start.EnvironmentVariables['TMP']=Join-Path $cleanProfile 'Temp'
$start.EnvironmentVariables['USERPROFILE']=$cleanProfile
$start.EnvironmentVariables['LOCALAPPDATA']=Join-Path $cleanProfile 'AppData/Local'
$start.EnvironmentVariables['APPDATA']=Join-Path $cleanProfile 'AppData/Roaming'
$start.EnvironmentVariables['BAJPTR_TRASH_DIR'] = Join-Path $testRoot 'retired'
$process = [Diagnostics.Process]::Start($start)
$writer = [IO.StreamWriter]::new($process.StandardInput.BaseStream,$utf8)
$writer.AutoFlush = $true
$errorReader = $process.StandardError.ReadToEndAsync()
$script:sequence = 0
function Invoke-Project([string]$Action,$Payload=@{}) {
    $script:sequence += 1
    $id = $script:sequence
    $writer.WriteLine((@{id=$id;action=$Action;payload=$Payload}|ConvertTo-Json -Depth 30 -Compress))
    while ($null -ne ($line=$process.StandardOutput.ReadLine())) {
        $message = $line | ConvertFrom-Json
        if ($message.id -eq $id -and $message.event -ne 'progress') {
            if ($message.event -ne 'result') { throw ($message|ConvertTo-Json -Depth 30) }
            $writer.WriteLine((@{id=0;action='accept';payload=@{target=$id}}|ConvertTo-Json -Compress))
            return $message.data
        }
    }
    throw 'Project service stopped without a result.'
}
try {
    $state = Invoke-Project 'load'
    if ($state.rules.Count -ne 2 -or $state.errors.PSObject.Properties.Count -gt 0) { throw 'Word list failed to load.' }
    $check = Invoke-Project 'check'
    if ($check.baseline_status.state -ne 'missing') { throw 'Fresh game should ask for a baseline.' }
    if (Test-Path -LiteralPath (Join-Path $data 'backup/_baselines')) { throw 'Checking silently created a baseline.' }
    $null = Invoke-Project 'baseline_commit' @{token=$check.baseline_status.token}
    $check = Invoke-Project 'check'
    if (-not $check.connected -or $check.baseline_status.state -ne 'ready') { throw 'Confirmed baseline is not ready.' }
    $tables = @(Invoke-Project 'tables')
    if ($tables.Count -ne 1) { throw 'Source table load failed.' }
    $search = Invoke-Project 'search' @{keyword=([char]0x4efb).ToString()+[char]0x52a1}
    # The other matching fixture text contains kana and must now be skipped.
    if ($search.total -ne 1 -or $search.rows.Count -ne 1) { throw 'Search must exclude the kana-bearing fixture text.' }
    $opened = Invoke-Project 'text_open' @{index=0;search_nonce=$search.nonce}
    $edited = Invoke-Project 'text_save' @{nonce=$opened.nonce;text='Rust edited text'}
    if ($edited.edits.Count -ne 1) { throw 'Targeted edit failed.' }
    $preview = Invoke-Project 'preview'
    if ($preview.total_changes -ne 1) { throw 'Preview failed.' }
    $generated = Invoke-Project 'generate'
    if (-not (Test-Path -LiteralPath $generated.output)) { throw 'Output missing.' }
    if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($catalog)) -ne [Convert]::ToBase64String($before)) { throw 'Generate changed the game.' }
    $null = Invoke-Project 'apply' @{nonce=$preview.nonce}
    $null = Invoke-Project 'export_text' @{path=(Join-Path $testRoot 'export')}
    $null = Invoke-Project 'restore'
    if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($catalog)) -ne [Convert]::ToBase64String($before)) { throw 'Restore differs from original.' }
    $again = Invoke-Project 'load'
    if ($again.edits.Count -ne 1) { throw 'Restore discarded documents.' }
    $report = @{passed=$true;baseline_confirmation_required=$true;environment_cleared=$true;separate_user_directories=$true;python_on_child_path=$false;current_format=$true;search=$true;search_skip_kana=$true;targeted_edit=$true;preview=$true;generate_verified=$generated.verified;apply=$true;export=$true;restore_equal=$true;documents_preserved=$true;game_dir=$game;data_dir=$data}
    [IO.File]::WriteAllText((Join-Path $testRoot 'report.json'),($report|ConvertTo-Json),$utf8)
    $report | ConvertTo-Json
} finally {
    if (-not $process.HasExited) {
        $writer.WriteLine('{"id":999,"action":"quit","payload":{}}')
        if (-not $process.WaitForExit(20000)) { $process.Kill(); throw 'Project worker did not exit.' }
    }
    $errors = $errorReader.GetAwaiter().GetResult()
    $exitCode = $process.ExitCode
    $process.Dispose()
    if ($exitCode -ne 0) { throw "Project worker failed: $errors" }
}
