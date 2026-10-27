param([string]$Toolchain = '1.97.1', [switch]$DebugBuild, [string]$TargetDir = '')
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Run on Windows with native C/C++ build tools and Rust installed.' }
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    $buildProfile = if ($DebugBuild) { 'debug' } else { 'release' }
    $cargoArgs = @('build', '--locked', '-p', 'cloudweave-desktop', '-p', 'cloudweave-cli')
    if ($TargetDir) { $cargoArgs += @('--target-dir', $TargetDir) }
    if (-not $DebugBuild) { $cargoArgs += '--release' }
    & cargo "+$Toolchain" @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw 'Cargo build failed.' }
    $outputDir = Join-Path $projectRoot 'dist/cloudweave-windows-x64'
    $buildRoot = if ($TargetDir) { $TargetDir } else { Join-Path $projectRoot 'target' }
    New-Item -ItemType Directory -Path $outputDir -Force | Out-Null
    foreach ($programName in @('cloudweave-desktop.exe','cloudweave-cli.exe')) {
        Copy-Item -LiteralPath (Join-Path $buildRoot "$buildProfile/$programName") -Destination $outputDir
    }
    foreach ($docName in @('LICENSE','README.md','SECURITY.md','RECOVERY.md','PROVIDER_SUPPORT.md','STATUS.md','PLATFORM_SUPPORT.md','ARCHITECTURE.md','FORMAT.md','THREAT_MODEL.md','DESIGN.md')) {
        Copy-Item -LiteralPath (Join-Path $projectRoot $docName) -Destination $outputDir
    }
    foreach ($inventoryName in @('dependency-inventory.json','cloudweave.spdx.json')) {
        $inventoryPath = Join-Path $projectRoot "artifacts/$inventoryName"
        if (Test-Path -LiteralPath $inventoryPath) { Copy-Item -LiteralPath $inventoryPath -Destination $outputDir }
    }
    $wryLicenses = Join-Path $outputDir 'licenses/wry'
    New-Item -ItemType Directory -Path $wryLicenses -Force | Out-Null
    Get-ChildItem -LiteralPath (Join-Path $projectRoot 'vendor/wry') -Filter 'LICENSE*' -File | Copy-Item -Destination $wryLicenses
    Get-ChildItem -LiteralPath $outputDir -File | Where-Object Name -ne 'SHA256SUMS.txt' | Get-FileHash -Algorithm SHA256 | ForEach-Object { '{0}  {1}' -f $_.Hash.ToLowerInvariant(), (Split-Path $_.Path -Leaf) } | Set-Content -LiteralPath (Join-Path $outputDir 'SHA256SUMS.txt') -Encoding ASCII
    Write-Output "Portable unsigned build: $outputDir"
} finally { Pop-Location }
