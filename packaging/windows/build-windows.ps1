[CmdletBinding()]
param(
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RootDir = Resolve-Path (Join-Path $ScriptDir '..\..')
$RootDir = $RootDir.Path
$Target = 'x86_64-pc-windows-msvc'
$Version = (Select-String -Path (Join-Path $RootDir 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches.Groups[1].Value
if (-not $Version) { $Version = '0.1.0' }
$MsiVersion = if ($Version -match '^(\d+\.\d+\.\d+)') { $Matches[1] } else { '0.1.0' }

$BuildRoot = Join-Path $RootDir 'target\windows'
$StageDir = Join-Path $BuildRoot 'stage'
$DistDir = Join-Path $BuildRoot 'dist'
$WxsPath = Join-Path $BuildRoot 'memory-layer.wxs'
$ZipPath = Join-Path $DistDir "memory-layer-$Version-windows-x86_64.zip"
$MsiPath = Join-Path $DistDir "memory-layer-$Version-windows-x86_64.msi"
$WixPdbPath = [System.IO.Path]::ChangeExtension($MsiPath, '.wixpdb')

function Copy-Tree($Source, $Destination) {
    if (Test-Path $Destination) { Remove-Item -Recurse -Force $Destination }
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    Copy-Item -Recurse -Force -Path (Join-Path $Source '*') -Destination $Destination
}

function Xml-Escape([string]$Value) {
    return [System.Security.SecurityElement]::Escape($Value)
}

function Safe-Id([string]$Value) {
    $safe = $Value -replace '[^A-Za-z0-9_\.]', '_'
    if ($safe -notmatch '^[A-Za-z_]') { $safe = "_$safe" }
    if ($safe.Length -gt 60) { $safe = $safe.Substring(0, 60) }
    return $safe
}

function New-WixDirectoryXml([string]$DirectoryPath, [string]$DirectoryId, [array]$AllFiles, [ref]$ComponentRefs, [int]$Depth) {
    $indent = '  ' * $Depth
    $name = Split-Path -Leaf $DirectoryPath
    $xml = @()
    $xml += "$indent<Directory Id=`"$DirectoryId`" Name=`"$(Xml-Escape $name)`">"

    $files = $AllFiles | Where-Object { (Split-Path -Parent $_.FullName) -eq $DirectoryPath } | Sort-Object FullName
    foreach ($file in $files) {
        $relative = [System.IO.Path]::GetRelativePath($StageDir, $file.FullName)
        $idBase = Safe-Id($relative)
        $componentId = "cmp_$idBase"
        $fileId = "fil_$idBase"
        $ComponentRefs.Value += $componentId
        $xml += "$indent  <Component Id=`"$componentId`" Guid=`"*`">"
        $xml += "$indent    <File Id=`"$fileId`" Source=`"$(Xml-Escape $file.FullName)`" KeyPath=`"yes`" />"
        $xml += "$indent  </Component>"
    }

    $dirs = Get-ChildItem -LiteralPath $DirectoryPath -Directory | Sort-Object FullName
    foreach ($dir in $dirs) {
        $relative = [System.IO.Path]::GetRelativePath($StageDir, $dir.FullName)
        $childId = "dir_$(Safe-Id $relative)"
        $xml += New-WixDirectoryXml -DirectoryPath $dir.FullName -DirectoryId $childId -AllFiles $AllFiles -ComponentRefs $ComponentRefs -Depth ($Depth + 1)
    }

    $xml += "$indent</Directory>"
    return $xml
}

if (-not $SkipBuild) {
    Write-Host 'Building web UI...'
    npm --prefix (Join-Path $RootDir 'web') ci
    npm --prefix (Join-Path $RootDir 'web') run build

    Write-Host 'Building Windows release binary...'
    if (-not $env:CARGO_PROFILE_RELEASE_LTO) {
        $env:CARGO_PROFILE_RELEASE_LTO = 'false'
    }
    if (-not $env:CARGO_PROFILE_RELEASE_CODEGEN_UNITS) {
        $env:CARGO_PROFILE_RELEASE_CODEGEN_UNITS = '16'
    }
    Write-Host "Using Windows Cargo release overrides: LTO=$env:CARGO_PROFILE_RELEASE_LTO CODEGEN_UNITS=$env:CARGO_PROFILE_RELEASE_CODEGEN_UNITS"
    cargo build --release --locked --bin memory --target $Target --manifest-path (Join-Path $RootDir 'Cargo.toml')
}

Remove-Item -Recurse -Force $StageDir, $DistDir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $StageDir, $DistDir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $StageDir 'share\memory-layer') | Out-Null

$ExePath = Join-Path $RootDir "target\$Target\release\memory.exe"
if (-not (Test-Path $ExePath)) { throw "Missing Windows binary: $ExePath" }
Copy-Item -Force $ExePath (Join-Path $StageDir 'memory.exe')
Copy-Item -Force (Join-Path $RootDir 'README.md') (Join-Path $StageDir 'README.md')
Copy-Item -Force (Join-Path $RootDir 'memory-layer.toml.example') (Join-Path $StageDir 'memory-layer.toml.example')
Copy-Tree (Join-Path $RootDir 'web\dist') (Join-Path $StageDir 'share\memory-layer\web')
Copy-Tree (Join-Path $RootDir '.agents\skills') (Join-Path $StageDir 'share\memory-layer\skill-template')

New-Item -ItemType Directory -Force -Path (Join-Path $StageDir 'completions') | Out-Null
& (Join-Path $StageDir 'memory.exe') completion powershell | Out-File -Encoding utf8 (Join-Path $StageDir 'completions\memory.ps1')

if (Test-Path $ZipPath) { Remove-Item -Force $ZipPath }
Compress-Archive -Path (Join-Path $StageDir '*') -DestinationPath $ZipPath -Force

$allFiles = Get-ChildItem -LiteralPath $StageDir -File -Recurse | Sort-Object FullName
$componentRefs = New-Object System.Collections.ArrayList
$rootFiles = $allFiles | Where-Object { (Split-Path -Parent $_.FullName) -eq $StageDir }
$directoryXml = @()

foreach ($file in $rootFiles) {
    $relative = [System.IO.Path]::GetRelativePath($StageDir, $file.FullName)
    $idBase = Safe-Id($relative)
    $componentId = "cmp_$idBase"
    $fileId = "fil_$idBase"
    [void]$componentRefs.Add($componentId)
    $directoryXml += "        <Component Id=`"$componentId`" Guid=`"*`">"
    $directoryXml += "          <File Id=`"$fileId`" Source=`"$(Xml-Escape $file.FullName)`" KeyPath=`"yes`" />"
    $directoryXml += "        </Component>"
}

foreach ($dir in (Get-ChildItem -LiteralPath $StageDir -Directory | Sort-Object FullName)) {
    $relative = [System.IO.Path]::GetRelativePath($StageDir, $dir.FullName)
    $directoryXml += New-WixDirectoryXml -DirectoryPath $dir.FullName -DirectoryId "dir_$(Safe-Id $relative)" -AllFiles $allFiles -ComponentRefs ([ref]$componentRefs) -Depth 4
}

$featureRefs = $componentRefs | Sort-Object | ForEach-Object { "      <ComponentRef Id=`"$_`" />" }
$upgradeCode = '7E6D7DA0-7D74-43F0-B816-E422B9E01B82'
$wxs = @"
<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">
  <Package Name="Memory Layer" Manufacturer="Memory Layer" Version="$MsiVersion" UpgradeCode="{$upgradeCode}" Scope="perMachine">
    <MajorUpgrade DowngradeErrorMessage="A newer version of Memory Layer is already installed." />
    <MediaTemplate EmbedCab="yes" />
    <StandardDirectory Id="ProgramFiles64Folder">
      <Directory Id="INSTALLFOLDER" Name="Memory Layer">
$($directoryXml -join "`n")
      </Directory>
    </StandardDirectory>
    <Feature Id="Main" Title="Memory Layer" Level="1">
$($featureRefs -join "`n")
    </Feature>
  </Package>
</Wix>
"@
Set-Content -Path $WxsPath -Value $wxs -Encoding utf8

$wix = Get-Command wix -ErrorAction SilentlyContinue
if (-not $wix) { throw 'WiX CLI not found. Install with: dotnet tool install --global wix' }
& $wix.Source build $WxsPath -arch x64 -out $MsiPath
Remove-Item -Force $WixPdbPath -ErrorAction SilentlyContinue

foreach ($artifact in @($ZipPath, $MsiPath)) {
    $hash = Get-FileHash -Algorithm SHA256 $artifact
    "$($hash.Hash.ToLowerInvariant())  $(Split-Path -Leaf $artifact)" | Set-Content -Encoding ascii "$artifact.sha256"
}

Write-Host "Built $ZipPath"
Write-Host "Built $MsiPath"
