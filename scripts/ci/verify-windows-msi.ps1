param(
    [Parameter(Mandatory = $true)]
    [string]$Msi,

    [string]$ExpectedName = "Kael",
    [string]$ExpectedVersion = "",
    [string]$SourceExecutable = "target/release/kael.exe",
    [string]$OutputDirectory = "target/windows-msi-verification"
)

$ErrorActionPreference = "Stop"

$msiPath = (Resolve-Path -LiteralPath $Msi).Path
$metadata = Get-Item -LiteralPath $msiPath
if (-not $metadata.PSIsContainer -and $metadata.Length -lt 4096) {
    throw "MSI is implausibly small: $msiPath ($($metadata.Length) bytes)"
}
if ($metadata.PSIsContainer) {
    throw "MSI path is a directory: $msiPath"
}

# Windows Installer packages use the OLE compound-document signature. Checking it
# here catches empty files, HTML error responses, and renamed staging directories
# before invoking any installer tooling.
$expectedHeader = [byte[]](0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1)
$stream = [System.IO.File]::OpenRead($msiPath)
try {
    $actualHeader = New-Object byte[] $expectedHeader.Length
    $bytesRead = $stream.Read($actualHeader, 0, $actualHeader.Length)
} finally {
    $stream.Dispose()
}
if ($bytesRead -ne $expectedHeader.Length) {
    throw "MSI header is truncated: $msiPath"
}
for ($index = 0; $index -lt $expectedHeader.Length; $index++) {
    if ($actualHeader[$index] -ne $expectedHeader[$index]) {
        throw "MSI does not have a Windows Installer compound-document header: $msiPath"
    }
}

$workspaceRoot = (Resolve-Path -LiteralPath ".").Path
$sourceExecutablePath = (Resolve-Path -LiteralPath $SourceExecutable).Path
$sourceExecutableMetadata = Get-Item -LiteralPath $sourceExecutablePath
if ($sourceExecutableMetadata.PSIsContainer -or $sourceExecutableMetadata.Length -eq 0) {
    throw "source executable must be a non-empty regular file: $sourceExecutablePath"
}
$escapedName = [Regex]::Escape($ExpectedName)
$sourceVersionOutput = (& $sourceExecutablePath --version 2>&1 | Out-String).Trim()
if ($LASTEXITCODE -ne 0) {
    throw "source executable failed --version with exit $LASTEXITCODE`: $sourceVersionOutput"
}
$sourceVersionPattern = "(?i)^$escapedName\s+(?<version>[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?)$"
if ($sourceVersionOutput -notmatch $sourceVersionPattern) {
    throw "source executable returned an unexpected version string: $sourceVersionOutput"
}
$binaryVersion = $Matches["version"]
if ([string]::IsNullOrWhiteSpace($ExpectedVersion)) {
    $ExpectedVersion = $binaryVersion
} elseif ($binaryVersion -ne $ExpectedVersion) {
    throw "source executable version mismatch: expected $ExpectedVersion, found $binaryVersion"
}
$targetRoot = [System.IO.Path]::GetFullPath((Join-Path $workspaceRoot "target"))
$verificationRoot = [System.IO.Path]::GetFullPath($OutputDirectory)
$targetPrefix = $targetRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
if (-not $verificationRoot.StartsWith($targetPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "verification output must be a child of the workspace target directory: $verificationRoot"
}
if (Test-Path -LiteralPath $verificationRoot) {
    Remove-Item -LiteralPath $verificationRoot -Recurse -Force
}
New-Item -ItemType Directory -Path $verificationRoot | Out-Null

$decompiled = Join-Path $verificationRoot "$ExpectedName.decompiled.wxs"
& wix msi decompile $msiPath -o $decompiled
if ($LASTEXITCODE -ne 0) {
    throw "WiX could not read/decompile the generated MSI (exit $LASTEXITCODE)"
}
if (-not (Test-Path -LiteralPath $decompiled)) {
    throw "WiX reported success without producing decompiled MSI authoring"
}
$authoring = Get-Content -LiteralPath $decompiled -Raw
try {
    [xml]$authoringDocument = $authoring
} catch {
    throw "WiX decompiled the MSI into invalid XML: $($_.Exception.Message)"
}
$packageNodes = @($authoringDocument.SelectNodes("//*[local-name()='Package']"))
if ($packageNodes.Count -ne 1) {
    throw "decompiled MSI must contain exactly one Package element; found $($packageNodes.Count)"
}
$package = $packageNodes[0]
if ($package.GetAttribute("Name") -ne $ExpectedName) {
    throw "MSI package name mismatch: expected $ExpectedName, found $($package.GetAttribute('Name'))"
}
if ($package.GetAttribute("Version") -ne $ExpectedVersion) {
    throw "MSI package version mismatch: expected $ExpectedVersion, found $($package.GetAttribute('Version'))"
}
$upgradeCode = [Guid]::Empty
if (-not [Guid]::TryParse($package.GetAttribute("UpgradeCode"), [ref]$upgradeCode)) {
    throw "MSI package does not contain a valid UpgradeCode"
}

# An administrative extraction verifies that the embedded cabinet and component
# table are usable without installing or registering the package on the runner.
# This is intentionally the hosted-CI gate instead of ICE validation, whose
# COM custom actions are not reliable in non-interactive runner sessions.
$extracted = Join-Path $verificationRoot "extracted"
$installerLog = Join-Path $verificationRoot "msiexec.log"
New-Item -ItemType Directory -Path $extracted | Out-Null
& msiexec.exe /a $msiPath /qn "TARGETDIR=$extracted" /l*v $installerLog
$installerExit = $LASTEXITCODE
if ($installerExit -notin @(0, 3010)) {
    throw "MSI administrative extraction failed with exit $installerExit; inspect $installerLog"
}

$expectedExecutableName = "$ExpectedName.exe"
$executables = @(Get-ChildItem -LiteralPath $extracted -Filter $expectedExecutableName -File -Recurse)
if ($executables.Count -ne 1) {
    throw "expected exactly one extracted $expectedExecutableName, found $($executables.Count)"
}
$sourceHash = (Get-FileHash -LiteralPath $sourceExecutablePath -Algorithm SHA256).Hash
$extractedHash = (Get-FileHash -LiteralPath $executables[0].FullName -Algorithm SHA256).Hash
if ($extractedHash -ne $sourceHash) {
    throw "extracted executable does not match the release executable (source=$sourceHash extracted=$extractedHash)"
}
$versionOutput = (& $executables[0].FullName --version 2>&1 | Out-String).Trim()
if ($LASTEXITCODE -ne 0) {
    throw "extracted executable failed --version with exit $LASTEXITCODE`: $versionOutput"
}
$escapedVersion = [Regex]::Escape($ExpectedVersion)
if ($versionOutput -notmatch "(?i)^$escapedName\s+$escapedVersion$") {
    throw "extracted executable version mismatch: $versionOutput"
}

$signature = Get-AuthenticodeSignature -LiteralPath $msiPath
$signatureStatus = [string]$signature.Status
if ($signatureStatus -notin @("NotSigned", "Valid")) {
    throw "unexpected MSI Authenticode status: $($signature.Status)"
}

Write-Host "WINDOWS_MSI_OK: $msiPath"
Write-Host "  package=$ExpectedName version=$ExpectedVersion bytes=$($metadata.Length)"
Write-Host "  signature=$signatureStatus sha256=$extractedHash"
Write-Host "  source=$sourceExecutablePath extracted=$($executables[0].FullName)"
