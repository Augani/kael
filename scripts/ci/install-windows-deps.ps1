$ErrorActionPreference = "Stop"

$vcpkgRoot = $env:VCPKG_ROOT
if (-not $vcpkgRoot) {
    throw "VCPKG_ROOT is not set"
}

$vcpkg = Join-Path $vcpkgRoot "vcpkg.exe"
if (-not (Test-Path $vcpkg)) {
    throw "vcpkg.exe was not found under $vcpkgRoot"
}

& $vcpkg install ffmpeg:x64-windows --clean-after-build
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
