param([Parameter(Mandatory=$true)][string]$MetadataPath)
$ErrorActionPreference='Stop'
$root=Split-Path $PSScriptRoot -Parent
$metadata=Get-Content -LiteralPath $MetadataPath -Raw -Encoding UTF8|ConvertFrom-Json
$packages=@{};$nodes=@{}
foreach($package in $metadata.packages){$packages[$package.id]=$package}
foreach($node in $metadata.resolve.nodes){$nodes[$node.id]=$node}
$queue=[Collections.Generic.Queue[string]]::new()
foreach($package in $metadata.packages){if($package.name -in @('bajptr-desktop','bajptr-rs','bajptr-launcher')){$queue.Enqueue($package.id)}}
$seen=[Collections.Generic.HashSet[string]]::new()
while($queue.Count -gt 0){
    $id=$queue.Dequeue();if(-not $seen.Add($id)){continue}
    foreach($dependency in $nodes[$id].deps){if(@($dependency.dep_kinds|Where-Object {$_.kind -ne 'dev'}).Count -gt 0){$queue.Enqueue($dependency.pkg)}}
}
$destination=Join-Path $root 'licenses/rust-dependencies'
New-Item -ItemType Directory -Path $destination -Force|Out-Null
$apacheSource=$metadata.packages|Where-Object {$_.source -and (Test-Path -LiteralPath (Join-Path (Split-Path $_.manifest_path -Parent) 'LICENSE-APACHE'))}|Select-Object -First 1
if(-not $apacheSource){throw 'Apache license text not available in resolved dependencies.'}
Copy-Item -LiteralPath (Join-Path (Split-Path $apacheSource.manifest_path -Parent) 'LICENSE-APACHE') -Destination (Join-Path $destination 'STANDARD-APACHE-2.0.txt')
# Some published crates omit their standard Boost license text. Reuse the
# complete BSL-1.0 text distributed by another resolved BSL-1.0 dependency.
$boostSource=$metadata.packages|Where-Object {$_.source -and $_.license -eq 'BSL-1.0' -and (Test-Path -LiteralPath (Join-Path (Split-Path $_.manifest_path -Parent) 'LICENSE'))}|Select-Object -First 1
if($boostSource){
    $boostText=[IO.File]::ReadAllText((Join-Path (Split-Path $boostSource.manifest_path -Parent) 'LICENSE'))
    if(-not $boostText.StartsWith('Boost Software License - Version 1.0')){throw 'Unexpected Boost license text.'}
    [IO.File]::WriteAllText((Join-Path $destination 'STANDARD-BSL-1.0.txt'),$boostText,[Text.UTF8Encoding]::new($false))
}
$inventory=@()
foreach($id in $seen){
    $package=$packages[$id]
    if(-not $package.source){continue}
    $source=Split-Path $package.manifest_path -Parent
    $folder=Join-Path $destination ($package.name+'-'+$package.version)
    $texts=@(Get-ChildItem -LiteralPath $source -Recurse -File|Where-Object {$_.Name -match '^(LICENSE|LICENCE|COPYING|NOTICE|OFL|UNLICENSE|AUTHORS|COPYRIGHT)([._-]|$)'})
    if($package.license_file){$texts+=Get-Item -LiteralPath (Join-Path $source $package.license_file)}
    $paths=@()
    foreach($file in ($texts|Sort-Object FullName -Unique)){
        $relative=$file.FullName.Substring($source.Length+1)
        $target=Join-Path $folder $relative
        New-Item -ItemType Directory -Path (Split-Path $target -Parent) -Force|Out-Null
        Copy-Item -LiteralPath $file.FullName -Destination $target
        $paths+=$relative.Replace('\','/')
    }
    $selection=$package.license
    if($paths.Count -eq 0){
        if($package.license -match 'Apache-2\.0'){$paths=@('../STANDARD-APACHE-2.0.txt');$selection='Apache-2.0'}
        elseif($package.license -eq 'BSL-1.0' -and $boostSource){$paths=@('../STANDARD-BSL-1.0.txt');$selection='BSL-1.0'}
        else{throw ('No license text found for '+$package.name)}
    }
    $inventory+=[pscustomobject][ordered]@{name=$package.name;version=$package.version;license=$package.license;distributed_license=$selection;authors=$package.authors;repository=$package.repository;license_texts=$paths}
}
$utf8=[Text.UTF8Encoding]::new($false)
[IO.File]::WriteAllText((Join-Path $destination 'inventory.json'),(($inventory|Sort-Object name,version)|ConvertTo-Json -Depth 8),$utf8)
Write-Output ('Dependency notices collected: '+$inventory.Count)
