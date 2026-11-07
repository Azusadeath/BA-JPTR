param([Parameter(Mandatory=$true)][string]$Destination)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path $PSScriptRoot -Parent
$destinationPath=[IO.Path]::GetFullPath($Destination)
if(Test-Path -LiteralPath $destinationPath){throw 'Use a new source-package directory.'}
New-Item -ItemType Directory -Path $destinationPath -Force|Out-Null
# Explicit allow-list: includes uncommitted Rust work, excludes user data and local build caches.
foreach($name in @('crates','tools','docs','licenses','Cargo.toml','Cargo.lock','README.md','LICENSE','THIRD_PARTY_NOTICES.md','.gitattributes','.gitignore')){
    Copy-Item -LiteralPath (Join-Path $projectRoot $name) -Destination $destinationPath -Recurse
}
if(@(Get-ChildItem -LiteralPath $destinationPath -Recurse -Force -File|Where-Object {$_.Extension -in @('.py','.pyc','.pyd','.exe','.dll')}).Count -gt 0){throw 'Unexpected binary or Python source in source package.'}
Write-Host "Source package: $destinationPath"
