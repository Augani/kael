#!/usr/bin/env pwsh
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
if (-not $IsWindows) { throw "Native UIA verification requires Windows" }

$workspace = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$evidence = Join-Path $workspace "target/native-accessibility-smoke/windows"
New-Item -ItemType Directory -Force $evidence | Out-Null
$ownedApp = $null
$client = $null
$savedHeadless = $env:KAEL_HEADLESS
$savedSmoke = $env:KAEL_ACCESSIBILITY_SMOKE
$savedWarp = $env:KAEL_FORCE_WARP
try {
    Set-Location $workspace
    cargo build --locked -p kael_ui --example virtual_tree --no-default-features --features native,kael/font-kit
    if ($LASTEXITCODE -ne 0) { throw "Virtual tree build failed" }
    cargo build --locked -p kael --example windows_accessibility_client --no-default-features --features font-kit
    if ($LASTEXITCODE -ne 0) { throw "Native UIA client build failed" }
    Remove-Item Env:KAEL_HEADLESS -ErrorAction SilentlyContinue
    $env:KAEL_ACCESSIBILITY_SMOKE = "1"
    $env:KAEL_FORCE_WARP = "1"
    $appLog = Join-Path $evidence "virtual-tree.log"
    $ownedApp = Start-Process -FilePath "target/debug/examples/virtual_tree.exe" -WorkingDirectory $workspace -PassThru `
        -RedirectStandardOutput $appLog -RedirectStandardError (Join-Path $evidence "virtual-tree.stderr.log")
    "os=$([System.Environment]::OSVersion.VersionString)`npid=$($ownedApp.Id)`nrenderer=warp`nclient=Windows UI Automation" |
        Set-Content (Join-Path $evidence "environment.txt")
    $clientLog = Join-Path $evidence "uia-client.log"
    $client = Start-Process -FilePath "target/debug/examples/windows_accessibility_client.exe" -PassThru `
        -ArgumentList @("$($ownedApp.Id)", ('"' + $appLog + '"')) `
        -RedirectStandardOutput $clientLog -RedirectStandardError (Join-Path $evidence "uia-client.stderr.log")
    if (-not $client.WaitForExit(110000)) { throw "Native UIA client exceeded 110 seconds" }
    $client.Refresh()
    Get-Content $clientLog
    if ($client.ExitCode -ne 0) { throw "Native UIA client failed: $($client.ExitCode)" }
    if (-not (Select-String -Path $clientLog -SimpleMatch "NATIVE_ACCESSIBILITY_RUNTIME_OK: backend=uia" -Quiet)) {
        throw "Native UIA success marker is missing"
    }
} finally {
    foreach ($process in @($client, $ownedApp)) {
        if ($null -ne $process) {
            $process.Refresh()
            if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force }
        }
    }
    $env:KAEL_HEADLESS = $savedHeadless
    $env:KAEL_ACCESSIBILITY_SMOKE = $savedSmoke
    $env:KAEL_FORCE_WARP = $savedWarp
}
