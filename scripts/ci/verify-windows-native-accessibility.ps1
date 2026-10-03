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
$savedTextSmoke = $env:KAEL_NATIVE_TEXT_SMOKE
try {
    Set-Location $workspace
    cargo build --locked -p kael_ui --example virtual_tree --no-default-features --features native,kael/font-kit
    if ($LASTEXITCODE -ne 0) { throw "Virtual tree build failed" }
    cargo build --locked -p kael --example windows_accessibility_client --no-default-features --features font-kit
    if ($LASTEXITCODE -ne 0) { throw "Native UIA client build failed" }
    cargo build --locked -p kael_ui --example editor_accessibility --no-default-features --features native,editor,kael/font-kit
    if ($LASTEXITCODE -ne 0) { throw "Native Unicode Editor fixture build failed" }
    $forkLog = Join-Path $evidence "uia-text-provider-tests.log"
    cargo test --locked -p kael_accesskit_windows --lib kael_text_tests -- --nocapture 2>&1 | Tee-Object -FilePath $forkLog
    $summary = [regex]::Match((Get-Content $forkLog -Raw), 'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;')
    if ($LASTEXITCODE -ne 0 -or -not $summary.Success -or [int]$summary.Groups[1].Value -lt 4 -or
        [int]$summary.Groups[2].Value -ne 0 -or [int]$summary.Groups[3].Value -ne 0) {
        throw "Mandatory native TextPattern provider ABI tests did not pass"
    }
    foreach ($required in @("native_find_text_unicode_forward_backward_case_and_nullable_abi",
                           "native_visible_ranges_unknown_geometry_is_a_nonnull_degenerate_array",
                           "native_com_range_rejects_foreign_owner",
                           "native_readonly_value_mutation_is_rejected_before_action_dispatch")) {
        if (-not (Select-String -Path $forkLog -Pattern ("test .*" + $required + " \.\.\. ok") -Quiet)) {
            throw "Mandatory native TextPattern provider ABI case is missing: $required"
        }
    }
    $env:KAEL_FORCE_WARP = "1"
    $geometryLog = Join-Path $evidence "directwrite-native-geometry-tests.log"
    cargo test --locked -p kael --lib --no-default-features --features font-kit,runtime_shaders native_directwrite -- --nocapture 2>&1 | Tee-Object -FilePath $geometryLog
    $summary = [regex]::Match((Get-Content $geometryLog -Raw), 'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;')
    if ($LASTEXITCODE -ne 0 -or -not $summary.Success -or [int]$summary.Groups[1].Value -lt 3 -or
        [int]$summary.Groups[2].Value -ne 0 -or [int]$summary.Groups[3].Value -ne 0) {
        throw "Mandatory native DirectWrite mixed-script geometry tests did not pass"
    }
    foreach ($required in @("native_directwrite_geometry_matches_bidirectional_glyph_origins_and_requested_spans",
                           "native_directwrite_geometry_preserves_combining_clusters_and_rejects_invalid_byte_spans",
                           "native_directwrite_source_seek_supports_visual_callback_order")) {
        if (-not (Select-String -Path $geometryLog -Pattern ("test .*" + $required + " \.\.\. ok") -Quiet)) {
            throw "Mandatory DirectWrite native geometry case is missing: $required"
        }
    }
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
    if (-not $client.WaitForExit(110000)) { Get-Content (Join-Path $evidence "uia-client.stderr.log") -ErrorAction SilentlyContinue; throw "Native UIA client exceeded 110 seconds" }
    $client.Refresh()
    Get-Content $clientLog
    Get-Content ($clientLog -replace "\.log$", ".stderr.log") -ErrorAction SilentlyContinue
    if ($client.ExitCode -ne 0) { throw "Native UIA client failed: $($client.ExitCode)" }
    if (-not (Select-String -Path $clientLog -SimpleMatch "NATIVE_ACCESSIBILITY_RUNTIME_OK: backend=uia" -Quiet)) {
        throw "Native UIA success marker is missing"
    }
    # End only our tree fixture before testing the independently owned Editor.
    $ownedApp.Refresh()
    if (-not $ownedApp.HasExited) { Stop-Process -Id $ownedApp.Id -Force }
    $ownedApp = $null
    $env:KAEL_NATIVE_TEXT_SMOKE = "1"
    $appLog = Join-Path $evidence "unicode-editor.log"
    $ownedApp = Start-Process -FilePath "target/debug/examples/editor_accessibility.exe" -WorkingDirectory $workspace -PassThru `
        -RedirectStandardOutput $appLog -RedirectStandardError (Join-Path $evidence "unicode-editor.stderr.log")
    $clientLog = Join-Path $evidence "uia-text-client.log"
    $client = Start-Process -FilePath "target/debug/examples/windows_accessibility_client.exe" -PassThru `
        -ArgumentList @("$($ownedApp.Id)", ('"' + $appLog + '"'), "--text") `
        -RedirectStandardOutput $clientLog -RedirectStandardError (Join-Path $evidence "uia-text-client.stderr.log")
    if (-not $client.WaitForExit(110000)) { Get-Content (Join-Path $evidence "uia-text-client.stderr.log") -ErrorAction SilentlyContinue; throw "Native Unicode UIA client exceeded 110 seconds" }
    $client.Refresh()
    Get-Content $clientLog
    Get-Content ($clientLog -replace "\.log$", ".stderr.log") -ErrorAction SilentlyContinue
    if ($client.ExitCode -ne 0) { throw "Native Unicode UIA client failed: $($client.ExitCode)" }
    if (-not (Select-String -Path $clientLog -SimpleMatch "NATIVE_TEXT_ACCESSIBILITY_RUNTIME_OK: backend=uia" -Quiet)) {
        throw "Native Unicode UIA success marker is missing"
    }
    if (-not $ownedApp.WaitForExit(10000)) { throw "Native Unicode fixture did not acknowledge the completed checks" }
    $ownedApp.Refresh()
    if ($ownedApp.ExitCode -ne 0 -or -not (Select-String -Path $appLog -SimpleMatch "NATIVE_TEXT_FIXTURE_COMPLETE" -Quiet)) {
        throw "Native Unicode fixture failed or its foreground completion evidence is missing"
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
    $env:KAEL_NATIVE_TEXT_SMOKE = $savedTextSmoke
}
