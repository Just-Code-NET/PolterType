#!/usr/bin/env bash
# Build a universal-binary poltertype.app and wrap it in a .dmg.
#
# Inputs (env vars):
#   VERSION        — release version (any leading "v" is stripped).
#   BIN_X86_64     — path to the x86_64-apple-darwin release binary.
#   BIN_ARM64      — path to the aarch64-apple-darwin release binary.
#                    Required: lipo refuses to merge a single arch.
#   DATA_DIR       — path to the prepared `data/` tree from
#                    `crates/poltertype-core/build.rs` (default: target/dist/data).
#   ICON_ICNS      — optional .icns; copied into Contents/Resources.
#   OUT_DIR        — output directory (default: target/dist).
#
# The resulting layout:
#
#   <OUT_DIR>/
#     poltertype-<VERSION>-universal-apple-darwin.dmg
#       └── poltertype.app/
#           └── Contents/
#               ├── Info.plist
#               ├── MacOS/poltertype       (universal: x86_64 + arm64)
#               └── Resources/
#                   ├── AppIcon.icns
#                   └── data/               (layout-mappings + wordlists)
#
# `Resources/data/` is the macOS-bundle slot the runtime resolver in
# `poltertype_core::data_dir` checks for first on darwin. The exe lives in
# `Contents/MacOS/`, so `<exe_dir>/../Resources/data` lands here.
#
# The DMG is *unsigned and unnotarised* — fine for closed beta, not
# for the App Store. macOS Gatekeeper will say "Poltertype cannot be
# opened because the developer cannot be verified" on first launch;
# the documented workaround is in CONTRIBUTING.md / the release notes
# (right-click → Open, or `xattr -dr com.apple.quarantine /Applications/poltertype.app`).

set -euo pipefail

VERSION="${VERSION:-0.0.0}"
VERSION="${VERSION#v}"  # strip a leading "v" (tag-like)
BIN_X86_64="${BIN_X86_64:?BIN_X86_64 is required}"
BIN_ARM64="${BIN_ARM64:?BIN_ARM64 is required}"
DATA_DIR="${DATA_DIR:-target/dist/data}"
ICON_ICNS="${ICON_ICNS:-}"
OUT_DIR="${OUT_DIR:-target/dist}"

# Sanity: the data tree is built by `cargo build` (poltertype-core's build.rs).
# CI runs the cargo step right before this script, so absent data
# almost always means "this script was run from a clean tree without
# building first" — which we want to surface loudly.
if [[ ! -f "${DATA_DIR}/wordlists/en_us.fst" ]]; then
    echo "Data tree not found at '${DATA_DIR}' (no wordlists/en_us.fst)." >&2
    echo "Build poltertype-core first: cargo build --release -p poltertype-app" >&2
    exit 1
fi

APP_NAME="poltertype"
APP_DIR_NAME="${APP_NAME}.app"
STAGING="$(mktemp -d -t poltertype-dmg-XXXXXX)"
APP_DIR="${STAGING}/${APP_DIR_NAME}"

trap 'rm -rf "${STAGING}"' EXIT

mkdir -p "${APP_DIR}/Contents/MacOS"
mkdir -p "${APP_DIR}/Contents/Resources"

# ─── universal binary via lipo ────────────────────────────────────────
# Mac users in 2026 are nearly all on Apple Silicon, but x86_64 holdouts
# still exist (Intel Mac mini, late-2020 / 2019 MBPs). Shipping one
# .app that runs natively on both is half a megabyte heavier than a
# single arch and saves us from running two parallel build matrices.
echo "lipo: ${BIN_X86_64} + ${BIN_ARM64} → universal ${APP_NAME}"
lipo -create -output "${APP_DIR}/Contents/MacOS/${APP_NAME}" \
    "${BIN_X86_64}" "${BIN_ARM64}"
chmod +x "${APP_DIR}/Contents/MacOS/${APP_NAME}"
lipo -info "${APP_DIR}/Contents/MacOS/${APP_NAME}"

# ─── Info.plist (with version substituted) ────────────────────────────
sed "s|__VERSION__|${VERSION}|g" \
    "$(dirname "$0")/Info.plist.in" \
    > "${APP_DIR}/Contents/Info.plist"

# ─── icon (optional) ──────────────────────────────────────────────────
if [[ -n "${ICON_ICNS}" && -f "${ICON_ICNS}" ]]; then
    cp "${ICON_ICNS}" "${APP_DIR}/Contents/Resources/AppIcon.icns"
    echo "icon: ${ICON_ICNS} → AppIcon.icns"
else
    # Drop the CFBundleIconFile key so Finder uses its generic .app
    # icon instead of looking for a missing AppIcon.icns.
    /usr/libexec/PlistBuddy -c "Delete :CFBundleIconFile" \
        "${APP_DIR}/Contents/Info.plist" 2>/dev/null || true
    echo "icon: none — bundle uses default Finder icon"
fi

# ─── data tree (layout mappings + FST wordlists) ──────────────────────
# Copy the prepared tree into Contents/Resources/data/. The runtime
# resolver finds it via the `<exe_dir>/../Resources/data` rule.
echo "data: ${DATA_DIR} → Contents/Resources/data"
cp -R "${DATA_DIR}" "${APP_DIR}/Contents/Resources/data"

# ─── DMG ──────────────────────────────────────────────────────────────
mkdir -p "${OUT_DIR}"
DMG_PATH="${OUT_DIR}/${APP_NAME}-${VERSION}-universal-apple-darwin.dmg"
rm -f "${DMG_PATH}"

# UDZO = compressed DMG, the standard format for distribution. We
# stage just the .app — no Applications symlink, no fancy background.
# A user who downloads this gets a window with one icon they drag to
# /Applications themselves; that's the convention even big apps use.
hdiutil create \
    -volname "${APP_NAME}" \
    -srcfolder "${STAGING}" \
    -fs HFS+ \
    -format UDZO \
    -ov \
    "${DMG_PATH}"

echo
echo "Built ${DMG_PATH}"
ls -la "${DMG_PATH}"
