param(
    [switch]$Check,
    [switch]$Test,
    [switch]$DebugBuild,
    [switch]$UpdateLock,
    [switch]$NoPublish,
    [string]$Package,
    [string]$TestTarget,
    [switch]$Format,
    [switch]$FormatCheck,
    [switch]$Lint,
    [switch]$FixLint,
    [string]$MetadataPath
)
$ErrorActionPreference = 'Stop'
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
    $manifestPath = Join-Path $nativeDir 'engine.json'
    $pending = "$manifestPath.pending-$([Guid]::NewGuid().ToString('N'))"
    [IO.File]::WriteAllText($pending, ($manifest | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
    Move-Item -LiteralPath $pending -Destination $manifestPath -Force
    $launcherSource = Join-Path $projectRoot "target/$profile/bajptr-launcher.exe"
    Copy-Item -LiteralPath $launcherSource -Destination (Join-Path $projectRoot 'bajptr.exe') -Force
    Write-Host "Published: $manifestPath"
} finally { Pop-Location }
