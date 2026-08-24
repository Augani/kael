param(
    [switch]$SkipGeneratedProject
)

$ErrorActionPreference = "Stop"
$workspace = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$target = Join-Path $workspace "target"
$evidence = Join-Path $target "native-renderer-smoke\windows"
$generatedRoot = $null
$generatedProcess = $null

function Invoke-LoggedCommand {
    param(
        [Parameter(Mandatory = $true)]
        [string]$LogPath,
        [Parameter(Mandatory = $true)]
        [string]$Command,
        [Parameter(ValueFromRemainingArguments = $true)]
        [string[]]$Arguments
    )
    & $Command @Arguments 2>&1 | Tee-Object -FilePath $LogPath
    if ($LASTEXITCODE -ne 0) {
        throw "$Command failed with exit code $LASTEXITCODE"
    }
}

try {
    New-Item -ItemType Directory -Force -Path $evidence | Out-Null
    Get-ChildItem -Force $evidence | Remove-Item -Force -Recurse
    Remove-Item Env:KAEL_HEADLESS -ErrorAction SilentlyContinue
    $env:CARGO_TARGET_DIR = $target
    $env:KAEL_FORCE_WARP = "1"
    $env:KAEL_EXPECT_SOFTWARE_RENDERER = "1"
    $env:KAEL_NATIVE_RENDERER_SMOKE_PNG = Join-Path $evidence "native-renderer.png"
    @(
        "os=$([System.Environment]::OSVersion.VersionString)",
        "architecture=$([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture)",
        "interactive=$([System.Environment]::UserInteractive)",
        "headless_set=$([bool](Test-Path Env:KAEL_HEADLESS))",
        "warp_forced=$env:KAEL_FORCE_WARP"
    ) | Set-Content -Encoding utf8 (Join-Path $evidence "environment.txt")

    Invoke-LoggedCommand `
        -LogPath (Join-Path $evidence "native-renderer.log") `
        -Command "cargo" `
        "build", "-p", "kael", "--example", "native_renderer_smoke", `
        "--no-default-features", "--features", "font-kit,runtime_shaders"
    $rendererBinary = Join-Path $target "debug\examples\native_renderer_smoke.exe"
    if (-not (Test-Path $rendererBinary)) {
        throw "Native renderer smoke executable was not built at $rendererBinary"
    }
    & (Join-Path $PSScriptRoot "capture-windows-pe-imports.ps1") `
        -Executable $rendererBinary `
        -LogPath (Join-Path $evidence "native-renderer-imports.log")
    Invoke-LoggedCommand `
        -LogPath (Join-Path $evidence "native-renderer.log") `
        -Command $rendererBinary
    $rendererLog = Get-Content -Raw (Join-Path $evidence "native-renderer.log")
    if (-not $rendererLog.Contains("NATIVE_RENDERER_SMOKE_GPU: backend=direct3d11")) {
        throw "native renderer log did not identify Direct3D 11"
    }
    if (-not $rendererLog.Contains("NATIVE_RENDERER_SMOKE_OK:")) {
        throw "native renderer did not publish its success marker"
    }
    if (-not $rendererLog.Contains("text_probe_pixels=")) {
        throw "native renderer did not prove retained text/glyph-atlas output"
    }
    if (-not (Test-Path $env:KAEL_NATIVE_RENDERER_SMOKE_PNG) -or
        (Get-Item $env:KAEL_NATIVE_RENDERER_SMOKE_PNG).Length -le 1024) {
        throw "native renderer did not produce a non-empty PNG"
    }

    if ($SkipGeneratedProject) {
        "GENERATED_NATIVE_RUNTIME_SKIPPED: explicitly disabled" |
            Set-Content -Encoding utf8 (Join-Path $evidence "generated-native.log")
        exit 0
    }

    Invoke-LoggedCommand `
        -LogPath (Join-Path $evidence "cli-build.log") `
        -Command "cargo" `
        "build", "--manifest-path", (Join-Path $workspace "Cargo.toml"), `
        "-p", "kael-cli", "--bin", "kael"
    $cli = Join-Path $target "debug\kael.exe"
    if (-not (Test-Path $cli)) {
        throw "Kael CLI was not built at $cli"
    }

    $generatedRoot = Join-Path $target ("generated-native-runtime." + [Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $generatedRoot | Out-Null
    Copy-Item (Join-Path $workspace "scripts\fixtures\generated-project-parity.Cargo.toml") `
        (Join-Path $generatedRoot "Cargo.toml")
    Copy-Item (Join-Path $workspace "Cargo.lock") (Join-Path $generatedRoot "Cargo.lock")
    Push-Location $generatedRoot
    try {
        Invoke-LoggedCommand `
            -LogPath (Join-Path $evidence "generated-scaffold.log") `
            -Command $cli `
            "new", "kael-generated-parity"
    } finally {
        Pop-Location
    }

    $project = Join-Path $generatedRoot "kael-generated-parity"
    $mainSource = Join-Path $project "src\main.rs"
    $manifest = Join-Path $project "Cargo.toml"
    $mainSnapshot = Join-Path $evidence "generated-main.rs"
    $manifestSnapshot = Join-Path $evidence "generated-Cargo.toml"
    Copy-Item $mainSource $mainSnapshot
    Copy-Item $manifest $manifestSnapshot
    $mainHashBefore = (Get-FileHash -Algorithm SHA256 $mainSource).Hash

    Invoke-LoggedCommand `
        -LogPath (Join-Path $evidence "generated-build.log") `
        -Command "cargo" `
        "build", "--manifest-path", $manifest, "--bin", "kael-generated-parity"
    $generatedBinary = Join-Path $target "debug\kael-generated-parity.exe"
    if (-not (Test-Path $generatedBinary)) {
        throw "generated native binary was not built at $generatedBinary"
    }

    $stdout = Join-Path $evidence "generated-app.stdout.log"
    $stderr = Join-Path $evidence "generated-app.stderr.log"
    $generatedProcess = Start-Process -FilePath $generatedBinary -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    $windowHandle = [IntPtr]::Zero
    $windowTitle = ""
    while ([DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 50
        $generatedProcess.Refresh()
        if ($generatedProcess.HasExited) {
            throw "generated project exited before mapping its native window (exit $($generatedProcess.ExitCode))"
        }
        if ($generatedProcess.MainWindowHandle -ne [IntPtr]::Zero) {
            $windowHandle = $generatedProcess.MainWindowHandle
            $windowTitle = $generatedProcess.MainWindowTitle
            break
        }
    }
    if ($windowHandle -eq [IntPtr]::Zero) {
        throw "generated project did not map a native Win32 window within 20 seconds"
    }
    if ($windowTitle -ne "kael-generated-parity") {
        throw "generated native window had unexpected title '$windowTitle'"
    }

    Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class KaelNativeWindowProbe {
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool IsWindowVisible(IntPtr hwnd);
}
"@
    $rect = New-Object KaelNativeWindowProbe+Rect
    if (-not [KaelNativeWindowProbe]::IsWindowVisible($windowHandle)) {
        throw "generated Win32 window exists but is not visible"
    }
    if (-not [KaelNativeWindowProbe]::GetWindowRect($windowHandle, [ref]$rect)) {
        throw "GetWindowRect failed for the generated Win32 window"
    }
    $windowWidth = $rect.Right - $rect.Left
    $windowHeight = $rect.Bottom - $rect.Top
    if ($windowWidth -lt 320 -or $windowHeight -lt 200) {
        throw "generated Win32 window is unexpectedly small: ${windowWidth}x${windowHeight}"
    }
    @(
        "title=$windowTitle",
        "handle=$windowHandle",
        "width=$windowWidth",
        "height=$windowHeight",
        "visible=true"
    ) | Set-Content -Encoding utf8 (Join-Path $evidence "generated-window.txt")

    # The unchanged starter intentionally runs until its window is closed. Ask
    # it to close through the real Win32 lifecycle and require a clean exit.
    if (-not $generatedProcess.CloseMainWindow()) {
        throw "CloseMainWindow could not request a clean generated-app shutdown"
    }
    if (-not $generatedProcess.WaitForExit(10000)) {
        throw "generated app did not exit after its native window was closed"
    }
    if ($generatedProcess.ExitCode -ne 0) {
        throw "generated app exited with status $($generatedProcess.ExitCode)"
    }
    $generatedProcess = $null

    $mainHashAfter = (Get-FileHash -Algorithm SHA256 $mainSource).Hash
    if ($mainHashAfter -ne $mainHashBefore) {
        throw "generated src/main.rs changed during the native runtime proof"
    }
    if ((Get-FileHash $mainSource).Hash -ne (Get-FileHash $mainSnapshot).Hash -or
        (Get-FileHash $manifest).Hash -ne (Get-FileHash $manifestSnapshot).Hash) {
        throw "generated project sources differ from their pre-launch snapshots"
    }
    @(
        "GENERATED_NATIVE_RUNTIME_WINDOW: title=$windowTitle handle=$windowHandle dimensions=${windowWidth}x${windowHeight}",
        "GENERATED_NATIVE_RUNTIME_OK: unchanged CLI project built, mapped a visible Win32 window, and closed cleanly"
    ) | Tee-Object -FilePath (Join-Path $evidence "generated-native.log")

    Write-Host "Windows Direct3D 11 release proof passed; evidence: $evidence"
} finally {
    if ($null -ne $generatedProcess -and -not $generatedProcess.HasExited) {
        Stop-Process -Id $generatedProcess.Id -Force -ErrorAction SilentlyContinue
    }
    if ($null -ne $generatedRoot -and
        $generatedRoot.StartsWith((Join-Path $target "generated-native-runtime.")) -and
        (Test-Path $generatedRoot)) {
        Remove-Item -Force -Recurse $generatedRoot
    }
}
