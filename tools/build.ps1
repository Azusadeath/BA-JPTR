param(
    [switch]$Check,
    [switch]$Test,
    [switch]$DebugBuild,
    [switch]$UpdateLock,
    [switch]$NoPublish,
    [string]$Package,
    [string]$TestTarget,
    [string]$TestFilter,
    [switch]$Format,
    [switch]$FormatCheck,
    [switch]$Lint,
    [switch]$FixLint,
    [string]$MetadataPath
)
$ErrorActionPreference = 'Stop'
function Retire-OldNativeVersions($NativeDir, $Manifests) {
    $versions = (Resolve-Path -LiteralPath (Join-Path $NativeDir 'versions')).Path
    $keep = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($manifest in $Manifests) {
        foreach ($kind in @('cli','desktop')) {
            if ($manifest.$kind -notmatch '^versions/([0-9a-f]{20})/bajptr-(rs|desktop)\.exe$') {
                Write-Warning 'Skipped native cleanup: an active manifest is invalid.'
                return
            }
            $null = $keep.Add($Matches[1])
        }
    }
    try {
        $processes = @(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object { $_.Name -in @('bajptr-desktop.exe','bajptr-rs.exe') })
        foreach ($process in $processes) {
            if (-not $process.ExecutablePath) { throw 'A running engine path is unavailable.' }
            $folder = Split-Path ([IO.Path]::GetFullPath($process.ExecutablePath)) -Parent
            if ((Split-Path $folder -Parent) -eq $versions) { $null = $keep.Add((Split-Path $folder -Leaf)) }
        }
    } catch {
        Write-Warning "Skipped native cleanup: $_"
        return
    }
    foreach ($directory in Get-ChildItem -LiteralPath $versions -Directory) {
        if ($keep.Contains($directory.Name) -or $directory.Name -notmatch '^[0-9a-f]{20}$' -or
            ($directory.Attributes -band [IO.FileAttributes]::ReparsePoint)) { continue }
        $source = [IO.Path]::GetFullPath($directory.FullName)
        if ((Split-Path $source -Parent) -ne $versions) { continue }
        $files = @(Get-ChildItem -LiteralPath $source -Force)
        if ($files.Count -ne 1 -or $files[0].PSIsContainer -or
            $files[0].Name -notin @('bajptr-desktop.exe','bajptr-rs.exe') -or
            ($files[0].Attributes -band [IO.FileAttributes]::ReparsePoint)) { continue }
        if ((Get-FileHash -LiteralPath $files[0].FullName -Algorithm SHA256).Hash.ToLower().Substring(0,20) -ne $directory.Name) { continue }
        try {
            $moved = $false
            if ($env:BAJPTR_TRASH_DIR) {
                try {
                    $trash = [IO.Path]::GetFullPath($env:BAJPTR_TRASH_DIR)
                    if ($trash -eq $versions -or $trash.StartsWith($versions + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Trash folder is inside native versions.' }
                    $target = Join-Path $trash ('native-' + $directory.Name + '-' + [Guid]::NewGuid().ToString('N'))
                    New-Item -ItemType Directory -Path $trash -Force | Out-Null
                    Move-Item -LiteralPath $source -Destination $target
                    $moved = -not (Test-Path -LiteralPath $source) -and
                        (Test-Path -LiteralPath $target) -and
                        (Split-Path (Resolve-Path -LiteralPath $target).Path -Parent) -eq (Resolve-Path -LiteralPath $trash).Path
                } catch { Write-Warning "Native archive move failed; trying Recycle Bin: $_" }
            }
            if (-not $moved -and (Test-Path -LiteralPath $source)) {
                Add-Type -AssemblyName Microsoft.VisualBasic
                [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory($source, 'OnlyErrorDialogs', 'SendToRecycleBin')
            }
            if (Test-Path -LiteralPath $source) { throw 'Old native directory remains.' }
        } catch { Write-Warning "Kept old native version $($directory.Name): $_" }
    }
}
if (($TestTarget -or $TestFilter) -and -not $Test) { throw 'TestTarget and TestFilter require -Test.' }
$projectRoot = Split-Path $PSScriptRoot -Parent
$localTools = Join-Path $projectRoot '.toolchain'
$cargoExe = Join-Path $localTools 'cargo/bin/cargo.exe'
if (Test-Path -LiteralPath $cargoExe) {
    $env:CARGO_HOME = Join-Path $localTools 'cargo'
    $env:RUSTUP_HOME = Join-Path $localTools 'rustup'
    $runtimeDir = Join-Path $localTools 'rustup/toolchains/stable-x86_64-pc-windows-gnu/lib/rustlib/x86_64-pc-windows-gnu/bin'
    $linkerDir = Join-Path $runtimeDir 'self-contained'
    $env:PATH = "$linkerDir;$(Split-Path $cargoExe);$runtimeDir;$(Join-Path $localTools 'w64devkit/bin');$env:PATH"
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = Join-Path $linkerDir 'x86_64-w64-mingw32-gcc.exe'
    $env:RUSTFLAGS = '-C dlltool=' + (Join-Path $localTools 'w64devkit/bin/dlltool.exe')
    $env:CC = Join-Path $localTools 'w64devkit/bin/gcc.exe'
    $env:AR = Join-Path $localTools 'w64devkit/bin/ar.exe'
    $env:WINDRES = Join-Path $localTools 'w64devkit/bin/x86_64-w64-mingw32-windres.exe'
} else { $cargoExe = 'cargo' }
$cargoArgs = @('build')
if ($Test) { $cargoArgs = @('test') }
elseif ($Check) { $cargoArgs = @('check') }
if ($Package) { $cargoArgs += @('-p', $Package) } else { $cargoArgs += '--workspace' }
if ($TestTarget) { $cargoArgs += @('--test', $TestTarget) }
if (-not $UpdateLock) { $cargoArgs += '--locked' }
if (-not $DebugBuild) { $cargoArgs += '--release' }
if ($TestFilter) { $cargoArgs += @('--', $TestFilter) }
Push-Location $projectRoot
try {
    if ($MetadataPath) {
        $metadata = & $cargoExe metadata --locked --format-version 1 --filter-platform x86_64-pc-windows-gnu
        if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed.' }
        [IO.File]::WriteAllText([IO.Path]::GetFullPath($MetadataPath),($metadata -join "`n"),[Text.UTF8Encoding]::new($false))
        return
    }
    if ($Format -or $FormatCheck) {
        $formatArgs = @('fmt', '--all')
        if ($FormatCheck) { $formatArgs += @('--', '--check') }
        & $cargoExe @formatArgs
        if ($LASTEXITCODE -ne 0) { throw 'Rust formatting check failed.' }
        return
    }
    if ($Lint) {
        if ($FixLint) {
            & $cargoExe clippy --workspace --all-targets --locked --release --fix --allow-dirty --allow-staged
            if ($LASTEXITCODE -ne 0) { throw 'Rust lint fixes failed.' }
            return
        }
        & $cargoExe clippy --workspace --all-targets --locked --release -- -D warnings
        if ($LASTEXITCODE -ne 0) { throw 'Rust lint check failed.' }
        return
    }
    & $cargoExe @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "Cargo failed: $LASTEXITCODE" }
    if ($Check -or $Test -or $NoPublish) { return }
    $profile = 'release'
    if ($DebugBuild) { $profile = 'debug' }
    $nativeDir = Join-Path $projectRoot 'native'
    New-Item -ItemType Directory -Path $nativeDir -Force | Out-Null
    $manifestPath = Join-Path $nativeDir 'engine.json'
    $previousPath = Join-Path $nativeDir 'previous-engine.json'
    $previous = if (Test-Path -LiteralPath $manifestPath) { Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json }
    $manifest = @{ protocol = 1 }
    $names = @{ desktop = 'bajptr-desktop.exe'; cli = 'bajptr-rs.exe' }
    foreach ($kind in $names.Keys) {
        $source = Join-Path $projectRoot "target/$profile/$($names[$kind])"
        $digest = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLower().Substring(0,20)
        $destination = Join-Path $nativeDir "versions/$digest"
        New-Item -ItemType Directory -Path $destination -Force | Out-Null
        if (-not (Test-Path -LiteralPath (Join-Path $destination $names[$kind]))) {
            Copy-Item -LiteralPath $source -Destination $destination
        }
        $manifest[$kind] = "versions/$digest/$($names[$kind])"
    }
    if ($previous -and ($previous.cli -ne $manifest.cli -or $previous.desktop -ne $manifest.desktop)) {
        $previousPending = "$previousPath.pending-$([Guid]::NewGuid().ToString('N'))"
        [IO.File]::WriteAllText($previousPending, ($previous | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
        Move-Item -LiteralPath $previousPending -Destination $previousPath -Force
    }
    $pending = "$manifestPath.pending-$([Guid]::NewGuid().ToString('N'))"
    [IO.File]::WriteAllText($pending, ($manifest | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
    Move-Item -LiteralPath $pending -Destination $manifestPath -Force
    $launcherSource = Join-Path $projectRoot "target/$profile/bajptr-launcher.exe"
    Copy-Item -LiteralPath $launcherSource -Destination (Join-Path $projectRoot 'bajptr.exe') -Force
    Write-Host "Published: $manifestPath"
    $manifests = @($manifest)
    if (Test-Path -LiteralPath $previousPath) { $manifests += (Get-Content -LiteralPath $previousPath -Raw -Encoding UTF8 | ConvertFrom-Json) }
    try { Retire-OldNativeVersions $nativeDir $manifests }
    catch { Write-Warning "Native cleanup incomplete; published binaries are available: $_" }
} finally { Pop-Location }
