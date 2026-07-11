#!/usr/bin/env bash
# Build a PolterType AppImage (x86_64).
#
# Inputs (env vars):
#   VERSION    — release version (any leading "v" is stripped).
#   BIN_PATH   — path to the already-built poltertype binary
#                (default: target/x86_64-unknown-linux-gnu/release/poltertype).
#   DATA_DIR   — path to the prepared `data/` tree from
#                `crates/poltertype-core/build.rs` (default: target/dist/data).
#   ICON_PNG   — path to a square PNG icon ≥ 256×256
#                (recommended: target/dist/icon-1024.png, generated
#                by `cargo xtask assets icon-png`). Required —
#                linuxdeploy refuses to package without one.
#   OUT_DIR    — output directory (default: target/dist).
#
# AppDir layout (the AppImage is just an AppDir squashed into a
# self-mounting binary, so this is what users see when the AppImage
# is mounted):
#
#   AppDir/
#     usr/
#       bin/poltertype
#       share/
#         applications/poltertype.desktop
#         icons/hicolor/256x256/apps/poltertype.png
#         poltertype/data/                ← layout mappings + FSTs
#
# The runtime resolver in `poltertype_core::data_dir` finds the data via
# `<exe_dir>/../share/poltertype/data` — that's the FHS-shaped
# location, the third lookup rule.
#
# Tooling: linuxdeploy + linuxdeploy-plugin-appimage. Both are single
# AppImages downloaded into `.tools/` on first run and reused on
# subsequent runs / CI builds with a Cargo cache.

set -euo pipefail

VERSION="${VERSION:-0.0.0}"
VERSION="${VERSION#v}"
BIN_PATH="${BIN_PATH:-target/x86_64-unknown-linux-gnu/release/poltertype}"
DATA_DIR="${DATA_DIR:-target/dist/data}"
ICON_PNG="${ICON_PNG:?ICON_PNG is required (PNG ≥ 256×256)}"
OUT_DIR="${OUT_DIR:-target/dist}"
APP_NAME="poltertype"
ARCH="x86_64"

if [[ ! -x "${BIN_PATH}" ]]; then
    echo "Binary not found / not executable: ${BIN_PATH}" >&2
    echo "Build with: cargo build --release --target ${ARCH}-unknown-linux-gnu -p poltertype-app" >&2
    exit 1
fi
if [[ ! -f "${DATA_DIR}/wordlists/en_us.fst" ]]; then
    echo "Data tree not found at '${DATA_DIR}' (no wordlists/en_us.fst)." >&2
    echo "Build poltertype-core first: cargo build --release -p poltertype-app" >&2
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

# Bundled data tree (layout mappings + FST wordlists). Goes under
# `usr/share/poltertype/data/` so the runtime resolver finds it via
# `<exe_dir>/../share/poltertype/data` (rule #3). Note we copy the
# directory's *contents*, not the directory itself, so the resulting
# path is `…/share/poltertype/data/wordlists/en_us.fst` and not
# `…/share/poltertype/data/data/wordlists/en_us.fst`.
mkdir -p "${APPDIR}/usr/share/${APP_NAME}/data"
cp -R "${DATA_DIR}/." "${APPDIR}/usr/share/${APP_NAME}/data/"

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
