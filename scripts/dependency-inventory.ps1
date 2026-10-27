param([string]$Toolchain = '1.97.1')
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    $rawMetadata = & cargo "+$Toolchain" metadata --locked --offline --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed.' }
    $metadata = $rawMetadata | ConvertFrom-Json
    $artifactDir = Join-Path $projectRoot 'artifacts'
    New-Item -ItemType Directory -Path $artifactDir -Force | Out-Null
    $inventory = @($metadata.packages | Sort-Object name,version | Select-Object name,version,license,repository,source)
    $inventory | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $artifactDir 'dependency-inventory.json') -Encoding UTF8
    $packageIds = @{}
    $packages = @()
    $index = 0
    foreach ($package in $metadata.packages) {
        $spdxId = "SPDXRef-Package-$index"
        $index += 1
        $packageIds[$package.id] = $spdxId
        $license = if ($package.license) { $package.license } else { 'NOASSERTION' }
        $download = if ($package.source -and $package.source.StartsWith('registry+')) { "https://crates.io/crates/$($package.name)/$($package.version)/download" } else { 'NOASSERTION' }
        $packages += @{
            SPDXID=$spdxId; name=$package.name; versionInfo=$package.version; downloadLocation=$download;
            filesAnalyzed=$false; licenseConcluded='NOASSERTION'; licenseDeclared=$license; copyrightText='NOASSERTION';
            externalRefs=@(@{referenceCategory='PACKAGE-MANAGER';referenceType='purl';referenceLocator="pkg:cargo/$($package.name)@$($package.version)"})
        }
    }
    $relations = @()
    foreach ($member in $metadata.workspace_members) { $relations += @{spdxElementId='SPDXRef-DOCUMENT';relationshipType='DESCRIBES';relatedSpdxElement=$packageIds[$member]} }
    foreach ($node in $metadata.resolve.nodes) {
        foreach ($dependency in $node.deps) { $relations += @{spdxElementId=$packageIds[$node.id];relationshipType='DEPENDS_ON';relatedSpdxElement=$packageIds[$dependency.pkg]} }
    }
    $sbom = @{
        spdxVersion='SPDX-2.3';dataLicense='CC0-1.0';SPDXID='SPDXRef-DOCUMENT';name='CloudWeave development Cargo resolution';
        documentNamespace=('https://spdx.org/spdxdocs/cloudweave-' + [guid]::NewGuid().ToString());
        creationInfo=@{creators=@('Tool: CloudWeave dependency-inventory.ps1');created=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ');comment='Cargo resolution includes all targets/features, not verified shipped binary contents. License declarations are unreviewed upstream metadata; no advisory or license-compliance scan is claimed.'};
        packages=$packages;relationships=$relations
    }
    $sbom | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $artifactDir 'cloudweave.spdx.json') -Encoding UTF8
    Write-Output "Inventoried $($packages.Count) resolved packages; SPDX and license metadata written to artifacts. This is not a vulnerability/compliance audit."
} finally { Pop-Location }
