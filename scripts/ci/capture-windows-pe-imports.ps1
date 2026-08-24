param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [Parameter(Mandatory = $true)]
    [string]$LogPath
)

$ErrorActionPreference = "Stop"
$executablePath = (Resolve-Path $Executable).Path
$logDirectory = Split-Path -Parent $LogPath
New-Item -ItemType Directory -Force -Path $logDirectory | Out-Null

$vswhere = Join-Path ${env:ProgramFiles(x86)} `
    "Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path $vswhere)) {
    throw "vswhere.exe was not found at $vswhere"
}
$visualStudio = (& $vswhere -latest -products * -property installationPath |
    Select-Object -First 1)
if (-not $visualStudio) {
    throw "Visual Studio was not found"
}
$dumpbin = Get-ChildItem -Path (Join-Path $visualStudio "VC\Tools\MSVC") `
    -Filter "dumpbin.exe" -File -Recurse |
    Where-Object { $_.FullName -like "*\bin\Hostx64\x64\dumpbin.exe" } |
    Sort-Object -Property FullName -Descending |
    Select-Object -First 1
if (-not $dumpbin) {
    throw "The x64 dumpbin.exe was not found below $visualStudio"
}

@(
    "executable=$executablePath",
    "dumpbin=$($dumpbin.FullName)"
) | Set-Content -Encoding utf8 $LogPath
& $dumpbin.FullName /nologo /imports $executablePath 2>&1 |
    Tee-Object -FilePath $LogPath -Append
if ($LASTEXITCODE -ne 0) {
    throw "dumpbin failed with exit code $LASTEXITCODE"
}
$imports = Get-Content -Raw $LogPath
if ($imports -match "(?m)^\s+[0-9A-F]+\s+DXGIGetDebugInterface1\s*$") {
    throw "DXGIGetDebugInterface1 must be resolved dynamically, not imported at process load"
}
if ($imports -match "(?m)^\s+[0-9A-F]+\s+TaskDialogIndirect\s*$") {
    throw "TaskDialogIndirect must be resolved dynamically, not imported at process load"
}
