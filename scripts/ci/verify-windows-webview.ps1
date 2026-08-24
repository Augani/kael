param(
    [string]$LogDirectory = "target/windows-webview-smoke"
)

$ErrorActionPreference = "Stop"
Remove-Item Env:KAEL_HEADLESS -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $LogDirectory | Out-Null
$logFile = Join-Path $LogDirectory "webview2.log"

$workspace = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$target = Join-Path $workspace "target"
$env:CARGO_TARGET_DIR = $target
& cargo build -p kael --example webview_smoke `
    --no-default-features --features webview,runtime_shaders 2>&1 |
    Tee-Object -FilePath $logFile
if ($LASTEXITCODE -ne 0) {
    throw "Windows WebView2 smoke build failed with exit code $LASTEXITCODE"
}
$webviewBinary = Join-Path $target "debug\examples\webview_smoke.exe"
if (-not (Test-Path $webviewBinary)) {
    throw "Windows WebView2 smoke executable was not built at $webviewBinary"
}
& (Join-Path $PSScriptRoot "capture-windows-pe-imports.ps1") `
    -Executable $webviewBinary `
    -LogPath (Join-Path $LogDirectory "webview2-imports.log")
& $webviewBinary 2>&1 | Tee-Object -FilePath $logFile -Append
if ($LASTEXITCODE -ne 0) {
    throw "Windows WebView2 smoke failed with exit code $LASTEXITCODE"
}

$markers = @(
    "WEBVIEW_SMOKE_STAGE: custom-protocol",
    "WEBVIEW_SMOKE_STAGE: page-load-finished",
    "WEBVIEW_SMOKE_STAGE: page-to-host-ipc",
    "WEBVIEW_SMOKE_STAGE: javascript-result",
    "WEBVIEW_SMOKE_STAGE: current-url",
    "WEBVIEW_SMOKE_STAGE: host-message-round-trip",
    "WEBVIEW_SMOKE_OK:",
    "Kael WebView smoke:42",
    "|url=kael-smoke://assets/probe",
    "|pong=42"
)
$log = Get-Content -Raw -Path $logFile
if ($log.Contains("WEBVIEW_SMOKE_FAIL:")) {
    throw "Windows WebView2 smoke reported failure"
}
foreach ($marker in $markers) {
    if (-not $log.Contains($marker)) {
        throw "Windows WebView2 smoke did not publish: $marker"
    }
}

Write-Host "Windows WebView2 runtime/custom-protocol smoke passed; log: $logFile"
