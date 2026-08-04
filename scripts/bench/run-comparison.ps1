#!/usr/bin/env pwsh
$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Resolve-Path (Join-Path $ScriptDir "..\..")
$BaselineDir = Join-Path $ProjectRoot "benchmarks\baselines"
$OutputDir = Join-Path $ProjectRoot "benchmark-results"
$RegressionThreshold = if ($env:REGRESSION_THRESHOLD) { $env:REGRESSION_THRESHOLD } else { "15.0" }

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

Write-Host "=== Kael Benchmark Comparison ==="
Write-Host ""

$BaselineFile = Join-Path $BaselineDir "kael-messaging-baseline.json"
if (-not (Test-Path $BaselineFile)) {
    Write-Host "Baseline not found at $BaselineFile"
    Write-Host "Generate one with: pwsh scripts/bench/generate-baseline.sh"
    exit 1
}

Write-Host "Running benchmarks..."
Set-Location $ProjectRoot
cargo bench --package kael --bench framework -- --output (Join-Path $OutputDir "current.json") --compare

Write-Host ""
Write-Host "Comparison complete."
