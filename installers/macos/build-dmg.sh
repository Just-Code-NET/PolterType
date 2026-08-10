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
#   SIGN_IDENTITY  — codesign identity for the finished bundle
#                    (default: "-", ad-hoc). Set to a "Developer ID
#                    Application: …" identity once we hold one.
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
# The bundle is signed ad-hoc (see the codesign step below) but *not*
# notarised — that needs a Developer ID we do not hold. macOS Gatekeeper
# will still say "PolterType cannot be opened because the developer
# cannot be verified" on first launch; the documented workaround is in
# CONTRIBUTING.md / the release notes (right-click → Open, or
# `xattr -dr com.apple.quarantine /Applications/poltertype.app`).

set -euo pipefail

VERSION="${VERSION:-0.0.0}"
VERSION="${VERSION#v}"  # strip a leading "v" (tag-like)
BIN_X86_64="${BIN_X86_64:?BIN_X86_64 is required}"
BIN_ARM64="${BIN_ARM64:?BIN_ARM64 is required}"
DATA_DIR="${DATA_DIR:-target/dist/data}"
ICON_ICNS="${ICON_ICNS:-}"
OUT_DIR="${OUT_DIR:-target/dist}"
SIGN_IDENTITY="${SIGN_IDENTITY:--}"   # "-" = ad-hoc; also catches an empty CI secret

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

# ─── codesign (last: the seal covers Resources and Info.plist) ────────
# Must happen after every file is in place — the signature seals
# Contents/Resources into _CodeSignature/CodeResources, and anything
# written afterwards invalidates it.
#
# Signing the *bundle* is the point, not tidiness. On an Apple Silicon
# runner the linker ad-hoc-signs the arm64 binary automatically and
# leaves the cross-compiled x86_64 one bare; lipo merges the two
# without re-signing, so the shipped fat binary had one signed slice
# and one unsigned. macOS runs the x86_64 slice on Intel, and TCC will
# not hold an Accessibility grant for unsigned code — the event tap
# attached and received nothing, silently (issue #28). codesign covers
# every slice of a universal binary in one pass.
SIGN_ARGS=(--force --sign "${SIGN_IDENTITY}")
if [[ "${SIGN_IDENTITY}" != "-" ]]; then
    # Both are notarisation prerequisites, and both are meaningless
    # (or, for --timestamp, a pointless network call) when ad-hoc.
    SIGN_ARGS+=(--options runtime --timestamp)
fi

# Pin the identifier to CFBundleIdentifier rather than letting codesign
# derive one: TCC keys the permission grant on it.
BUNDLE_ID="$(/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" \
    "${APP_DIR}/Contents/Info.plist")"

echo "codesign: identity '${SIGN_IDENTITY}', identifier ${BUNDLE_ID}"
codesign "${SIGN_ARGS[@]}" --identifier "${BUNDLE_ID}" "${APP_DIR}"

# Verify per architecture, not just once. `--verify` alone passes on a
# bundle whose fat binary has an unsigned slice, which is exactly the
# bug this step exists to prevent, so assert each slice by name.
codesign --verify --strict --verbose=2 "${APP_DIR}"
for arch in x86_64 arm64; do
    if ! codesign --display --verbose=2 --arch "${arch}" "${APP_DIR}" 2>&1 \
        | grep -q '^Identifier='; then
        echo "codesign: the ${arch} slice is not signed — refusing to ship it." >&2
        exit 1
    fi
    echo "codesign: ${arch} slice OK"
done

# The designated requirement is what TCC stores against the
# Accessibility grant, so it belongs in the release log: with an ad-hoc
# identity it pins a cdhash, which is why the grant does not survive an
# update. A Developer ID one pins the certificate instead, and does.
codesign --display --requirements - "${APP_DIR}"

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
