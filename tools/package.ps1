param([string]$Destination)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
if (-not $Destination) { $Destination = Join-Path $projectRoot 'dist/BA-JPTR' }
$destinationPath = [IO.Path]::GetFullPath($Destination)
$archivePath = $destinationPath + '.zip'
if (Test-Path -LiteralPath $archivePath) { throw 'Package archive already exists. Archive the previous output before rebuilding.' }
if ((Test-Path -LiteralPath $destinationPath) -and (Get-ChildItem -LiteralPath $destinationPath -Force | Select-Object -First 1)) {
    throw 'Use a new or empty package directory.'
}
New-Item -ItemType Directory -Path $destinationPath -Force | Out-Null
$sourceNative = Join-Path $projectRoot 'native'
$manifest = Get-Content -LiteralPath (Join-Path $sourceNative 'engine.json') -Raw -Encoding UTF8 | ConvertFrom-Json
foreach ($kind in @('desktop','cli')) {
    $relative = $manifest.$kind
    if (-not $relative -or [IO.Path]::IsPathRooted($relative) -or $relative.Split('/') -contains '..') { throw 'Invalid binary manifest.' }
    $source = Join-Path $sourceNative $relative
    $destination = Join-Path $destinationPath "native/$relative"
    New-Item -ItemType Directory -Path (Split-Path $destination -Parent) -Force | Out-Null
    Copy-Item -LiteralPath $source -Destination $destination
}
Copy-Item -LiteralPath (Join-Path $sourceNative 'engine.json') -Destination (Join-Path $destinationPath 'native/engine.json')
foreach ($name in @('BA-JPTR.exe','LICENSE','THIRD_PARTY_NOTICES.md')) { Copy-Item -LiteralPath (Join-Path $projectRoot $name) -Destination $destinationPath }
$guide = [IO.File]::ReadAllText((Join-Path $projectRoot 'README.md'),[Text.Encoding]::UTF8)
$guide = [regex]::Replace($guide, '(?m)^#{1,6} +', '')
$guide = (($guide -split '\r?\n') | Where-Object { $_ -notmatch 'docs/DEVELOPMENT\.md' }) -join "`n"
$guide = [regex]::Replace($guide, '(?m)^> ?', '')
$guide = [regex]::Replace($guide, '\[([^\]]+)\]\((https?://[^)]+)\)', '$1 ($2)')
$guide = [regex]::Replace($guide, '\[([^\]]+)\]\([^)]+\)', '$1')
$guide = $guide.Replace('**','').Replace('`','').Replace("`r`n","`n").Replace("`n","`r`n")
[IO.File]::WriteAllText((Join-Path $destinationPath 'README.txt'),$guide,[Text.UTF8Encoding]::new($true))
Copy-Item -LiteralPath (Join-Path $projectRoot 'licenses') -Destination $destinationPath -Recurse
Compress-Archive -LiteralPath $destinationPath -DestinationPath $archivePath -CompressionLevel Optimal
Write-Host "Portable package: $destinationPath"
Write-Host "Package archive: $archivePath"
