# Build the Windows MSI installer.
#
# Inputs:
#   $env:VERSION     — release version, e.g. "0.1.0" or "v0.1.0-alpha.1".
#                      A leading "v" is stripped everywhere; the
#                      pre-release suffix survives into the file name
#                      but not into the WiX ProductVersion, which only
#                      accepts M.m.b[.r] numerics. See the normalise
#                      block below.
#
# Output is `poltertype-<version>-x86_64-pc-windows-msvc.msi`, matching
# the AppImage and DMG naming — all three strip the tag's "v".
#   $env:BIN_PATH    — path to the already-built poltertype.exe
#   $env:DATA_DIR    — path to the prepared data tree (FSTs + TOMLs);
#                      defaults to `target/dist/data`, which is where
#                      `poltertype-core/build.rs` writes them.
#   $env:OUT_DIR     — directory for the .msi (created if missing)
#   $env:ICON_PATH   — optional .ico for Add/Remove Programs entry
#
# Assumes WiX Toolset 3.x is on PATH (`candle.exe` and `light.exe`).
# CI installs it via `choco install wixtoolset --no-progress -y`. The
# Chocolatey package adds itself to the system PATH, but a fresh shell
# won't see that until reload — hence the manual fallback search in
# Get-WixBinDir below.
#
# Run from the repo root:
#     pwsh installers\windows\build-msi.ps1

$ErrorActionPreference = 'Stop'

function Get-WixBinDir {
    # 1. Already on PATH? (Local dev, second CI step, etc.)
    if (Get-Command candle.exe -ErrorAction SilentlyContinue) {
        return (Get-Command candle.exe).Source | Split-Path
    }
    # 2. Standard install dirs — search Program Files for the binary.
    foreach ($base in @(${Env:ProgramFiles(x86)}, $Env:ProgramFiles)) {
        if (-not $base) { continue }
        $found = Get-ChildItem -Path $base -Recurse -Filter candle.exe `
                               -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found) { return $found.Directory.FullName }
    }
    throw "WiX Toolset (candle.exe / light.exe) not found. Install with `choco install wixtoolset -y`."
}

# ─── normalise inputs ─────────────────────────────────────────────────
$rawVersion = if ($env:VERSION) { $env:VERSION } else { '0.0.0' }

# Two different versions come out of the tag, and they are not
# interchangeable:
#
#   $fileVersion — for the .msi *file name*. Strips only the tag's
#                  leading "v", exactly like build-appimage.sh and
#                  build-dmg.sh do, so all three artifacts of a release
#                  are named alike. The pre-release suffix stays: an
#                  rc has to be a distinct file from the final, or
#                  `poltertype-0.4.0-rc.1` and `poltertype-0.4.0` would
#                  collide under one name.
#
#   $msiVersion  — for WiX's ProductVersion. Also drops the
#                  "-prerelease" suffix, because WiX does its own
#                  M.m.b[.r] parsing and rejects semver tags outright.
$fileVersion = $rawVersion.TrimStart('v')
$msiVersion  = $fileVersion.Split('-')[0]
if ($msiVersion -notmatch '^\d+\.\d+(\.\d+(\.\d+)?)?$') {
    throw "Cannot derive an MSI-compatible version from VERSION='$rawVersion' (got '$msiVersion')."
}

$binPath = if ($env:BIN_PATH) { $env:BIN_PATH } `
           else { 'target\x86_64-pc-windows-msvc\release\poltertype.exe' }
if (-not (Test-Path $binPath)) {
    throw "Binary not found at '$binPath'. Build with `cargo build --release --target x86_64-pc-windows-msvc -p poltertype-app` first."
}
$binAbs = (Resolve-Path $binPath).Path

# `target/dist/data/` is populated by `crates/poltertype-core/build.rs` on
# every cargo build of poltertype-core (poltertype-app pulls it in transitively).
# CI runs the cargo build right before this script, so the directory
# is guaranteed-fresh by the time we get here. If someone runs
# build-msi.ps1 standalone without a recent build, the explicit
# Test-Path below makes the failure mode obvious.
$dataDir = if ($env:DATA_DIR) { $env:DATA_DIR } else { 'target\dist\data' }
if (-not (Test-Path (Join-Path $dataDir 'wordlists\en_us.fst'))) {
    throw "Data tree not found at '$dataDir' (no en_us.fst). Build poltertype-core first: `cargo build --release -p poltertype-app`."
}
$dataDirAbs = (Resolve-Path $dataDir).Path

$outDir = if ($env:OUT_DIR) { $env:OUT_DIR } else { 'target\dist' }
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$outDirAbs = (Resolve-Path $outDir).Path

$licenseAbs = (Resolve-Path 'LICENSE').Path

# Optional icon — the WiX template gates `<?ifdef IconPath ?>` on the
# var, so we just don't pass -d if there's nothing to set.
$iconArgs = @()
if ($env:ICON_PATH -and (Test-Path $env:ICON_PATH)) {
    $iconAbs = (Resolve-Path $env:ICON_PATH).Path
    $iconArgs = @("-dIconPath=$iconAbs")
}

# ─── build ────────────────────────────────────────────────────────────
$wix = Get-WixBinDir
$wxsPath = (Resolve-Path 'installers\wix\main.wxs').Path
$tagged = "poltertype-$fileVersion-x86_64-pc-windows-msvc"
$wixobj = Join-Path $outDirAbs "$tagged.wixobj"
$msi    = Join-Path $outDirAbs "$tagged.msi"

Write-Host "candle: $wxsPath  →  $wixobj"
& "$wix\candle.exe" `
    "-dProductVersion=$msiVersion" `
    "-dBinPath=$binAbs" `
    "-dLicensePath=$licenseAbs" `
    "-dDataDir=$dataDirAbs" `
    @iconArgs `
    -arch x64 `
    -out $wixobj `
    $wxsPath
if ($LASTEXITCODE -ne 0) { throw "candle.exe failed (exit $LASTEXITCODE)" }

Write-Host "light:  $wixobj  →  $msi"
# -spdb skips the .wixpdb (we don't need symbol info for MSI). No UI
# extension: a perUser install runs through the system's default
# dialog flow; pulling WixUIExtension in just adds a wizard with a
# license screen that nobody reads on a beta build.
& "$wix\light.exe" `
    -spdb `
    -out $msi `
    $wixobj
if ($LASTEXITCODE -ne 0) { throw "light.exe failed (exit $LASTEXITCODE)" }

Write-Host ""
Write-Host "Built $msi"
Get-Item $msi | Format-List Name, Length, LastWriteTime
