#!/usr/bin/env bash
# Re-point every packaging manifest at a published release.
#
#     packaging/bump.sh 0.7.0
#
# Downloads that release's artifacts, hashes them, and rewrites the
# version + checksum in the AUR PKGBUILDs, the winget manifests and the
# Homebrew cask. It writes nothing outside this repository and publishes
# nothing — see packaging/README.md for the per-target publish step,
# which stays manual on purpose.
#
# Run it only against a release that is actually published: the point of
# a checksum is that it came from the bytes users will download.

set -euo pipefail

VERSION="${1:?usage: packaging/bump.sh <version>   (e.g. 0.7.0)}"
VERSION="${VERSION#v}"
REPO="Just-Code-NET/PolterType"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT

fetch_sha() {
    local name="$1"
    if ! gh release download "v${VERSION}" -R "${REPO}" \
            --pattern "${name}" --dir "${WORK}" >/dev/null 2>&1; then
        echo "missing from release v${VERSION}: ${name}" >&2
        return 1
    fi
    sha256sum "${WORK}/${name}" | cut -d' ' -f1
}

echo "==> hashing the v${VERSION} artifacts"
APPIMAGE_SHA="$(fetch_sha "poltertype-${VERSION}-x86_64.AppImage")"
MSI_SHA="$(fetch_sha "poltertype-${VERSION}-x86_64-pc-windows-msvc.msi")"
DMG_SHA="$(fetch_sha "poltertype-${VERSION}-universal-apple-darwin.dmg")"

# `sed -i` with anchored patterns rather than a templating pass: every
# line below is one we want to be able to read in a diff and recognise.
echo "==> AUR"
sed -i "s/^pkgver=.*/pkgver=${VERSION}/;s/^pkgrel=.*/pkgrel=1/" \
    "${ROOT}/packaging/aur/poltertype/PKGBUILD" \
    "${ROOT}/packaging/aur/poltertype-bin/PKGBUILD"
sed -i "s/^  '[0-9a-f]\{64\}'$/  '${APPIMAGE_SHA}'/" \
    "${ROOT}/packaging/aur/poltertype-bin/PKGBUILD"

echo "==> winget"
sed -i "s/^PackageVersion: .*/PackageVersion: ${VERSION}/" \
    "${ROOT}/packaging/winget/"*.yaml
sed -i \
    -e "s|InstallerUrl: .*|InstallerUrl: https://github.com/${REPO}/releases/download/v${VERSION}/poltertype-${VERSION}-x86_64-pc-windows-msvc.msi|" \
    -e "s/InstallerSha256: .*/InstallerSha256: $(echo "${MSI_SHA}" | tr 'a-f' 'A-F')/" \
    "${ROOT}/packaging/winget/JustCode.PolterType.installer.yaml"

echo "==> homebrew"
sed -i \
    -e "s/^  version \".*\"/  version \"${VERSION}\"/" \
    -e "s/^  sha256 \".*\"/  sha256 \"${DMG_SHA}\"/" \
    "${ROOT}/packaging/homebrew/poltertype.rb"

echo
echo "Updated to ${VERSION}:"
git -C "${ROOT}" diff --stat -- packaging/ || true
echo
echo "Nothing has been published. packaging/README.md has the three"
echo "publish steps, each of which is a deliberate human action."
