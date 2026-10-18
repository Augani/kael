param(
    [Parameter(Mandatory = $true)]
    [string]$InputJson,

    [ValidateSet("full", "gif", "both")]
    [string]$Mode = "both",

    [double]$MaxFps = 60.0,

    [double]$Scale = 1.0,

    [string]$CargoTargetDir = "target-bench",

    [switch]$AsJson
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\\..")).Path
$inputPath = if ([System.IO.Path]::IsPathRooted($InputJson)) {
    (Resolve-Path $InputJson).Path
} else {
    (Resolve-Path (Join-Path $repoRoot $InputJson)).Path
}
$cargoTargetPath = Join-Path $repoRoot $CargoTargetDir
$benchmarkExe = Join-Path $cargoTargetPath "debug\\examples\\benchmark_render.exe"

$modes = switch ($Mode) {
    "both" { @("full", "gif") }
    default { @($Mode) }
}

Push-Location $repoRoot
try {
    $relativeInput = (Resolve-Path -LiteralPath $inputPath -Relative)
    if ($relativeInput.StartsWith(".\")) {
        $relativeInput = $relativeInput.Substring(2)
    }
    $dockerInputPath = $relativeInput.Replace("\", "/")

    $env:CARGO_TARGET_DIR = $cargoTargetPath
    cargo build --example benchmark_render | Out-Host

    $rasterResults = foreach ($currentMode in $modes) {
        & $benchmarkExe $inputPath --mode $currentMode --max-fps $MaxFps --scale $Scale --json |
            ConvertFrom-Json
    }

    $dockerScript = @'
set -e
apt-get update >/dev/null
apt-get install -y --no-install-recommends librlottie-dev build-essential pkg-config >/dev/null
g++ -O2 -std=c++17 /repo/tools/bench/rlottie_bench.cpp $(pkg-config --cflags --libs rlottie) -o /tmp/rlottie_bench
for mode in __MODES__; do
  /tmp/rlottie_bench "/repo/__INPUT__" "$mode" __MAX_FPS__ __SCALE__
done
'@
    $dockerScript = $dockerScript.Replace("__MODES__", ($modes -join " "))
    $dockerScript = $dockerScript.Replace("__INPUT__", $dockerInputPath)
    $dockerScript = $dockerScript.Replace(
        "__MAX_FPS__",
        $MaxFps.ToString([System.Globalization.CultureInfo]::InvariantCulture)
    )
    $dockerScript = $dockerScript.Replace(
        "__SCALE__",
        $Scale.ToString([System.Globalization.CultureInfo]::InvariantCulture)
    )

    $dockerOutput = docker run --rm -v "${repoRoot}:/repo" ubuntu:24.04 bash -lc $dockerScript
    $rlottieResults = $dockerOutput |
        Where-Object { $_ -match '^\{' } |
        ForEach-Object { $_ | ConvertFrom-Json }

    $comparison = foreach ($currentMode in $modes) {
        $raster = $rasterResults | Where-Object mode -eq $currentMode | Select-Object -First 1
        $rlottie = $rlottieResults | Where-Object mode -eq $currentMode | Select-Object -First 1
        [pscustomobject]@{
            mode                         = $currentMode
            rasterlottie_elapsed_ms      = [math]::Round([double]$raster.elapsed_ms, 3)
            rasterlottie_avg_ms_per_frame = [math]::Round([double]$raster.avg_ms_per_frame, 3)
            rlottie_elapsed_ms           = [math]::Round([double]$rlottie.elapsed_ms, 3)
            rlottie_avg_ms_per_frame     = [math]::Round([double]$rlottie.avg_ms_per_frame, 3)
            raster_vs_rlottie_ratio      = [math]::Round(
                ([double]$raster.elapsed_ms / [double]$rlottie.elapsed_ms),
                3
            )
            rendered_frames              = [int]$raster.rendered_frames
            output_size                  = "{0}x{1}" -f $raster.output_width, $raster.output_height
        }
    }

    if ($AsJson) {
        [pscustomobject]@{
            input        = $inputPath
            scale        = $Scale
            max_fps      = $MaxFps
            rasterlottie = $rasterResults
            rlottie      = $rlottieResults
            comparison   = $comparison
        } | ConvertTo-Json -Depth 5
    } else {
        Write-Host "Input: $inputPath"
        Write-Host "Scale: $Scale"
        Write-Host "Max FPS: $MaxFps"
        $comparison | Format-Table -AutoSize | Out-Host
    }
}
finally {
    Pop-Location
}
