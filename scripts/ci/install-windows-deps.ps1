$ErrorActionPreference = "Stop"

$vcpkgRoot = $env:VCPKG_ROOT
if (-not $vcpkgRoot) {
    throw "VCPKG_ROOT is not set"
}

$vcpkg = Join-Path $vcpkgRoot "vcpkg.exe"
if (-not (Test-Path $vcpkg)) {
    throw "vcpkg.exe was not found under $vcpkgRoot"
}

$binaryCache = $env:VCPKG_DEFAULT_BINARY_CACHE
if (-not $binaryCache) {
    throw "VCPKG_DEFAULT_BINARY_CACHE is not set"
}
[void][System.IO.Directory]::CreateDirectory($binaryCache)

& $vcpkg install ffmpeg:x64-windows --clean-after-build
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
