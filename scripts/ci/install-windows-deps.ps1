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

$installedRoot = Join-Path $vcpkgRoot 'installed\x64-windows'
$installRoot = Join-Path $vcpkgRoot 'installed'
& $vcpkg install `
    --triplet x64-windows `
    "--x-manifest-root=$PSScriptRoot" `
    "--x-install-root=$installRoot" `
    --clean-after-build
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$pkgconf = Join-Path $installedRoot 'tools\pkgconf\pkgconf.exe'
if (-not (Test-Path $pkgconf)) {
    throw "pkgconf.exe was not found under $installedRoot"
}

$pkgConfigPath = @(
    (Join-Path $installedRoot 'lib\pkgconfig')
    (Join-Path $installedRoot 'share\pkgconfig')
) -join ';'

$env:PKG_CONFIG = $pkgconf
$env:PKG_CONFIG_PATH = $pkgConfigPath
& $pkgconf --atleast-version=1.3.0 dav1d
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

"PKG_CONFIG=$pkgconf" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
"PKG_CONFIG_PATH=$pkgConfigPath" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
