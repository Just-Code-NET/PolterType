#!/usr/bin/env bash
# Build a kb-switcher AppImage (x86_64).
#
# Inputs (env vars):
#   VERSION    — release version (any leading "v" is stripped).
#   BIN_PATH   — path to the already-built kb-switcher binary
#                (default: target/x86_64-unknown-linux-gnu/release/kb-switcher).
#   ICON_PNG   — path to a square PNG icon ≥ 256×256
#                (recommended: target/dist/icon-1024.png, generated
#                by `cargo xtask assets icon-png`). Required —
#                linuxdeploy refuses to package without one.
#   OUT_DIR    — output directory (default: target/dist).
#
# Tooling: linuxdeploy + linuxdeploy-plugin-appimage. Both are single
# AppImages downloaded into `.tools/` on first run and reused on
# subsequent runs / CI builds with a Cargo cache.

set -euo pipefail

VERSION="${VERSION:-0.0.0}"
VERSION="${VERSION#v}"
BIN_PATH="${BIN_PATH:-target/x86_64-unknown-linux-gnu/release/kb-switcher}"
ICON_PNG="${ICON_PNG:?ICON_PNG is required (PNG ≥ 256×256)}"
OUT_DIR="${OUT_DIR:-target/dist}"
APP_NAME="kb-switcher"
ARCH="x86_64"

if [[ ! -x "${BIN_PATH}" ]]; then
    echo "Binary not found / not executable: ${BIN_PATH}" >&2
    echo "Build with: cargo build --release --target ${ARCH}-unknown-linux-gnu -p kb-app" >&2
    exit 1
fi
if [[ ! -f "${ICON_PNG}" ]]; then
    echo "Icon not found: ${ICON_PNG}" >&2
    echo "Generate with: cargo xtask assets icon-png ${ICON_PNG}" >&2
    exit 1
fi

mkdir -p "${OUT_DIR}"
mkdir -p .tools

# ─── fetch tooling (cached) ───────────────────────────────────────────
# Pinned to the `continuous` channel — this is the upstream-recommended
# tag for tools-in-development that don't ship versioned releases.
# linuxdeploy itself is a stable AppImage at this tag.
LD_BASE="https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous"
PLUGIN_BASE="https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous"

if [[ ! -x .tools/linuxdeploy ]]; then
    echo "fetch: linuxdeploy"
    curl -fSL -o .tools/linuxdeploy \
        "${LD_BASE}/linuxdeploy-${ARCH}.AppImage"
    chmod +x .tools/linuxdeploy
fi
if [[ ! -x .tools/linuxdeploy-plugin-appimage ]]; then
    echo "fetch: linuxdeploy-plugin-appimage"
    curl -fSL -o .tools/linuxdeploy-plugin-appimage \
        "${PLUGIN_BASE}/linuxdeploy-plugin-appimage-${ARCH}.AppImage"
    chmod +x .tools/linuxdeploy-plugin-appimage
fi

# Ubuntu / Debian GitHub runners run AppImages directly (FUSE is
# preinstalled). No-op outside CI; harmless inside.
export APPIMAGE_EXTRACT_AND_RUN=1

# linuxdeploy locates its plugins (`linuxdeploy-plugin-*`) on $PATH —
# adding `.tools/` here is what wires our cached download in. Without
# this, `--output appimage` fails with "couldn't find appimage plugin".
export PATH="$(pwd)/.tools:$PATH"

# ─── stage the AppDir ─────────────────────────────────────────────────
APPDIR="$(pwd)/AppDir"
rm -rf "${APPDIR}"
mkdir -p "${APPDIR}/usr/bin"
mkdir -p "${APPDIR}/usr/share/applications"
mkdir -p "${APPDIR}/usr/share/icons/hicolor/256x256/apps"

cp "${BIN_PATH}" "${APPDIR}/usr/bin/${APP_NAME}"
chmod +x "${APPDIR}/usr/bin/${APP_NAME}"

DESKTOP_SRC="$(dirname "$0")/${APP_NAME}.desktop"
cp "${DESKTOP_SRC}" "${APPDIR}/usr/share/applications/${APP_NAME}.desktop"

# linuxdeploy needs an icon at AppDir/usr/share/icons/.../<exec-name>.png
# **and** a top-level <exec-name>.png alongside the .desktop file.
# Both come from the same source PNG — easier than managing per-size
# variants until we have a real icon set.
cp "${ICON_PNG}" "${APPDIR}/usr/share/icons/hicolor/256x256/apps/${APP_NAME}.png"

# ─── build AppImage ───────────────────────────────────────────────────
export OUTPUT="${OUT_DIR}/${APP_NAME}-${VERSION}-${ARCH}.AppImage"
export VERSION
export ARCH

# linuxdeploy auto-discovers Qt/GTK plugins via its plugin system,
# bundles needed shared libs, and asks the appimage plugin to wrap
# the result in a self-contained AppImage. Our binary is statically
# linked apart from libc and the system X11/Wayland libs, so the
# bundled set is small (~2-3 MiB extra over the bare binary).
.tools/linuxdeploy \
    --appdir "${APPDIR}" \
    --executable "${APPDIR}/usr/bin/${APP_NAME}" \
    --desktop-file "${APPDIR}/usr/share/applications/${APP_NAME}.desktop" \
    --icon-file "${ICON_PNG}" \
    --icon-filename "${APP_NAME}" \
    --output appimage

echo
echo "Built ${OUTPUT}"
ls -la "${OUTPUT}"
