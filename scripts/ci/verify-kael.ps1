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
        # Compile every crate, target, optional battery, template, and the
        # repository-only Astryx showcase under the native Windows toolchain.
        Invoke-Step cargo clippy --workspace --all-targets --all-features '--' '-D' warnings
        # The hosted Windows Server 2025 runner can fail during process load for
        # GUI-linked Kael test binaries before Rust's test harness starts. Keep
        # Windows CI as a compile/link proof and run the test binaries on
        # macOS/Linux, where the headless runtime is stable in Actions.
        Invoke-Step cargo test --workspace --all-targets --all-features --no-run
        Invoke-Step cargo check --package kael_http_client --no-default-features
        # These engine crates are hardware-free, so their tests also execute on
        # Windows rather than stopping at the compile/link proof.
        Invoke-Step cargo test --package kael_cache --package kael_engines --package kael_media_engines --package kael_render_graph --package kael_storage --lib
        Invoke-Step cargo check --package kael --lib --features 'platform-foundation'
        Invoke-Step cargo check --package kael --lib --features 'document'
        Invoke-Step cargo check --package kael --lib --features 'pdf'
        Invoke-Step cargo check --package kael --lib --features 'office'
        Invoke-Step cargo check --package kael --lib --features 'notifications-full'
        Invoke-Step cargo check --package kael --lib --features 'share'
        Invoke-Step cargo check --package kael --lib --features 'platform-foundation document pdf office notifications-full share'
        Invoke-Step cargo check --package kael --bench framework
        Invoke-Step cargo run --package xtask '--' dry-run
    }
}
