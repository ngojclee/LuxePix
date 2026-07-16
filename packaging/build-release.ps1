[CmdletBinding()]
param(
    [string]$Version,
    [switch]$SkipInstaller
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$ProjectRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$DistRoot = [System.IO.Path]::GetFullPath((Join-Path $ProjectRoot "dist"))
Set-Location -LiteralPath $ProjectRoot

$CargoText = Get-Content -LiteralPath (Join-Path $ProjectRoot "Cargo.toml") -Raw
$CargoVersionMatch = [regex]::Match(
    $CargoText,
    '(?ms)^\[package\].*?^version\s*=\s*"(?<version>\d+\.\d+\.\d+)"'
)
if (-not $CargoVersionMatch.Success) {
    throw "Could not read the package version from Cargo.toml."
}
$CargoVersion = $CargoVersionMatch.Groups["version"].Value
if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = $CargoVersion
}
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Version must use MAJOR.MINOR.PATCH semantic versioning."
}
if ($Version -ne $CargoVersion) {
    throw "Requested version $Version does not match Cargo.toml version $CargoVersion."
}

$Tag = "v$Version"
$ReleaseDir = [System.IO.Path]::GetFullPath((Join-Path $DistRoot $Tag))
if (-not $ReleaseDir.StartsWith($DistRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Resolved release directory is outside dist."
}
if (Test-Path -LiteralPath $ReleaseDir) {
    Remove-Item -LiteralPath $ReleaseDir -Recurse -Force
}
New-Item -ItemType Directory -Path $ReleaseDir | Out-Null

if ([string]::IsNullOrWhiteSpace($env:LUXEPIX_GITHUB_OWNER)) {
    $env:LUXEPIX_GITHUB_OWNER = "ngojclee"
}
if ([string]::IsNullOrWhiteSpace($env:LUXEPIX_GITHUB_REPO)) {
    $env:LUXEPIX_GITHUB_REPO = "LuxePix"
}
$env:LUXEPIX_BUILD_ID = (git rev-parse --short=8 HEAD 2>$null)
if ($LASTEXITCODE -ne 0) {
    $env:LUXEPIX_BUILD_ID = "local"
}

Write-Host "Building LuxePix $Version release binaries..."
cargo build --release --locked --bins
if ($LASTEXITCODE -ne 0) {
    throw "Cargo release build failed."
}

$PortableName = "LuxePix-$Tag-windows-x64-portable.zip"
$SetupName = "LuxePix-$Tag-windows-x64-setup.exe"
$PortablePath = Join-Path $ReleaseDir $PortableName
$SetupPath = Join-Path $ReleaseDir $SetupName
$PortableStage = Join-Path $ReleaseDir "_portable"
New-Item -ItemType Directory -Path $PortableStage | Out-Null
New-Item -ItemType Directory -Path (Join-Path $PortableStage "data") | Out-Null
New-Item -ItemType File -Path (Join-Path $PortableStage ".portable") | Out-Null

Copy-Item -LiteralPath (Join-Path $ProjectRoot "target\release\luxepix.exe") -Destination (Join-Path $PortableStage "LuxePix.exe")
Copy-Item -LiteralPath (Join-Path $ProjectRoot "target\release\LuxePixUpdater.exe") -Destination $PortableStage
Copy-Item -LiteralPath (Join-Path $ProjectRoot "packaging\THIRD_PARTY_NOTICES.txt") -Destination $PortableStage

$PortableReadme = @"
LuxePix $Version Portable

Run LuxePix.exe directly. The .portable marker keeps settings, thumbnails,
the product index, update downloads, and local similarity data inside data\
beside the application.

Do not remove .portable if you want this copy to remain self-contained.
"@
[System.IO.File]::WriteAllText(
    (Join-Path $PortableStage "README.txt"),
    $PortableReadme,
    [System.Text.UTF8Encoding]::new($false)
)

Add-Type -AssemblyName System.IO.Compression.FileSystem
[System.IO.Compression.ZipFile]::CreateFromDirectory(
    $PortableStage,
    $PortablePath,
    [System.IO.Compression.CompressionLevel]::Optimal,
    $false
)

if (-not $SkipInstaller) {
    $IsccCandidates = @(
        (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"),
        (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe"),
        (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe")
    )
    $Iscc = $IsccCandidates |
        Where-Object { $_ -and (Test-Path -LiteralPath $_) } |
        Select-Object -First 1
    if (-not $Iscc) {
        $Command = Get-Command "ISCC.exe" -ErrorAction SilentlyContinue
        if ($Command) {
            $Iscc = $Command.Source
        }
    }
    if (-not $Iscc) {
        throw "Inno Setup 6 is required to create the installer."
    }

    Write-Host "Building Inno Setup installer..."
    & $Iscc `
        "/DMyAppVersion=$Version" `
        "/DMyOutputDir=$ReleaseDir" `
        (Join-Path $ProjectRoot "installer\LuxePix.iss")
    if ($LASTEXITCODE -ne 0) {
        throw "Inno Setup compilation failed."
    }
    if (-not (Test-Path -LiteralPath $SetupPath)) {
        throw "Inno Setup did not create the expected installer $SetupName."
    }
}

$Assets = @()
$PortableInfo = Get-Item -LiteralPath $PortablePath
$PortableHash = (Get-FileHash -LiteralPath $PortablePath -Algorithm SHA256).Hash.ToLowerInvariant()
$Assets += [ordered]@{
    name = $PortableName
    mode = "portable"
    architecture = "x86_64"
    byte_length = $PortableInfo.Length
    sha256 = $PortableHash
}

if (-not $SkipInstaller) {
    $SetupInfo = Get-Item -LiteralPath $SetupPath
    $SetupHash = (Get-FileHash -LiteralPath $SetupPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $Assets += [ordered]@{
        name = $SetupName
        mode = "installed"
        architecture = "x86_64"
        byte_length = $SetupInfo.Length
        sha256 = $SetupHash
    }
}

$ReleaseUrl = "https://github.com/$($env:LUXEPIX_GITHUB_OWNER)/$($env:LUXEPIX_GITHUB_REPO)/releases/tag/$Tag"
$Manifest = [ordered]@{
    schema_version = 1
    app_version = $Version
    published_at = [DateTime]::UtcNow.ToString("yyyy-MM-ddTHH:mm:ssZ")
    minimum_updater_version = "1.1.0"
    entry_point = "LuxePix.exe"
    preserve = @("data", ".portable")
    model_pack_compatibility = $null
    release_notes_url = $ReleaseUrl
    assets = $Assets
}
$ManifestPath = Join-Path $ReleaseDir "release-manifest.json"
[System.IO.File]::WriteAllText(
    $ManifestPath,
    ($Manifest | ConvertTo-Json -Depth 8),
    [System.Text.UTF8Encoding]::new($false)
)

$ReleaseNotesPath = Join-Path $ReleaseDir "release-notes.md"
Copy-Item -LiteralPath (Join-Path $ProjectRoot "RELEASE_NOTES.md") -Destination $ReleaseNotesPath

$ChecksumFiles = @($PortablePath)
if (-not $SkipInstaller) {
    $ChecksumFiles += $SetupPath
}
$ChecksumFiles += $ManifestPath
$ChecksumLines = foreach ($Path in $ChecksumFiles) {
    $Hash = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
    "$Hash  $([System.IO.Path]::GetFileName($Path))"
}
[System.IO.File]::WriteAllLines(
    (Join-Path $ReleaseDir "SHA256SUMS.txt"),
    $ChecksumLines,
    [System.Text.UTF8Encoding]::new($false)
)

Remove-Item -LiteralPath $PortableStage -Recurse -Force

$Result = [ordered]@{
    version = $Version
    tag = $Tag
    output = $ReleaseDir
    portable = $PortablePath
    installer = if ($SkipInstaller) { $null } else { $SetupPath }
    manifest = $ManifestPath
    checksums = (Join-Path $ReleaseDir "SHA256SUMS.txt")
}
$Result | ConvertTo-Json -Depth 4
