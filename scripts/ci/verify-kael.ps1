param(
    [ValidateSet("default")]
    [string]$Mode = "default"
)

$ErrorActionPreference = "Stop"

function Invoke-Step {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Command,

        [Parameter(ValueFromRemainingArguments = $true)]
        [string[]]$Arguments
    )

    $rendered = ($Arguments -join " ")
    if ($rendered.Length -gt 0) {
        Write-Host "+ $Command $rendered"
    } else {
        Write-Host "+ $Command"
    }

    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}

function Test-DllExport {
    param([string]$Library, [string]$FunctionName)
    try {
        $handle = [System.Runtime.InteropServices.NativeLibrary]::Load($Library)
        [void][System.Runtime.InteropServices.NativeLibrary]::GetExport($handle, $FunctionName)
        [System.Runtime.InteropServices.NativeLibrary]::Free($handle)
        return $true
    } catch { return $false }
}

function Show-DllAudit {
    Write-Host "=== DLL export audit ==="
    $checks = @(
        @("uiautomationcore.dll", "UiaHostProviderFromHwnd"),
        @("uiautomationcore.dll", "UiaRaiseAutomationEvent"),
        @("uiautomationcore.dll", "UiaReturnRawElementProvider"),
        @("uiautomationcore.dll", "UiaClientsAreListening"),
        @("dwrite.dll",           "DWriteCreateFactory"),
        @("d3d11.dll",            "D3D11CreateDevice"),
        @("dxgi.dll",             "CreateDXGIFactory2"),
        @("dxgi.dll",             "DXGIGetDebugInterface1"),
        @("oleaut32.dll",         "SafeArrayCreateVector")
    )
    foreach ($c in $checks) {
        $ok = Test-DllExport $c[0] $c[1]
        $tag = if ($ok) { "OK     " } else { "MISSING" }
        Write-Host "  [$tag] $($c[0]) :: $($c[1])"
    }
    Write-Host "========================"
}

switch ($Mode) {
    "default" {
        Show-DllAudit
        Invoke-Step cargo clippy --package kael --lib '--' '-D' warnings
        # The hosted Windows Server 2025 runner can fail during process load for
        # GUI-linked GPUI test binaries before Rust's test harness starts. Keep
        # Windows CI as a compile/link proof and run the test binaries on
        # macOS/Linux, where the headless runtime is stable in Actions.
        Invoke-Step cargo test --package kael --lib --no-run
        Invoke-Step cargo test --package kael --test worker_process --no-run
        Invoke-Step cargo test --package kael --test extension_process --no-run
        # The engine crates are pure libraries (no GUI/GPU linkage), so their unit
        # suites run fully on Windows, verifying the cross-platform logic at runtime.
        Invoke-Step cargo clippy --package kael_engines --package kael_media_engines --package kael_render_graph --lib '--' '-D' warnings
        Invoke-Step cargo test --package kael_engines --package kael_media_engines --package kael_render_graph --lib
        Invoke-Step cargo check --package kael --lib --features 'platform-foundation'
        Invoke-Step cargo check --package kael --lib --features 'document'
        Invoke-Step cargo check --package kael --lib --features 'pdf'
        Invoke-Step cargo check --package kael --lib --features 'notifications-full'
        Invoke-Step cargo check --package kael --lib --features 'share'
        Invoke-Step cargo check --package kael --lib --features 'platform-foundation document pdf notifications-full share'
        Invoke-Step cargo check --package kael --example platform_features --example daemon_app --example perf_bench --example capture_demo
        Invoke-Step cargo run --package xtask '--' dry-run
    }
}
