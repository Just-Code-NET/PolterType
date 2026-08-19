#!/usr/bin/env bash
# Build a PolterType AppImage (x86_64 or aarch64).
#
# Inputs (env vars):
#   VERSION    — release version (any leading "v" is stripped).
#   ARCH       — x86_64 (default) or aarch64. This is the AppImage
#                naming convention, which happens to match both the
#                Rust target triple prefix and linuxdeploy's own asset
#                names, so one variable drives all three. There is no
#                cross-compilation here: the release workflow runs this
#                natively on a runner of the matching architecture.
#   BIN_PATH   — path to the already-built poltertype binary
#                (default: target/<ARCH>-unknown-linux-gnu/release/poltertype).
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
APP_NAME="poltertype"
ARCH="${ARCH:-x86_64}"

# Refuse an arch we have no linuxdeploy asset for rather than fetching a
# 404 and failing three steps later with a confusing message.
case "${ARCH}" in
    x86_64 | aarch64) ;;
    *)
        echo "Unsupported ARCH '${ARCH}' (expected x86_64 or aarch64)." >&2
        exit 1
        ;;
esac

BIN_PATH="${BIN_PATH:-target/${ARCH}-unknown-linux-gnu/release/poltertype}"
DATA_DIR="${DATA_DIR:-target/dist/data}"
ICON_PNG="${ICON_PNG:?ICON_PNG is required (PNG ≥ 256×256)}"
OUT_DIR="${OUT_DIR:-target/dist}"

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
# Both come from the same source PNG — the mark is rendered at whatever
# size we ask for, so there is nothing to gain from per-size variants.
cp "${ICON_PNG}" "${APPDIR}/usr/share/icons/hicolor/256x256/apps/${APP_NAME}.png"

# Bundled data tree (layout mappings + FST wordlists). Goes under
# `usr/share/poltertype/data/` so the runtime resolver finds it via
# `<exe_dir>/../share/poltertype/data` (rule #3). Note we copy the
# directory's *contents*, not the directory itself, so the resulting
# path is `…/share/poltertype/data/wordlists/en_us.fst` and not
# `…/share/poltertype/data/data/wordlists/en_us.fst`.
mkdir -p "${APPDIR}/usr/share/${APP_NAME}/data"
cp -R "${DATA_DIR}/." "${APPDIR}/usr/share/${APP_NAME}/data/"

# ─── locate the tray library ──────────────────────────────────────────
# `tray-icon` reaches the tray through `libappindicator-sys`, which
# `dlopen`s the object by soname. That keeps it out of the binary's
# DT_NEEDED, so linuxdeploy's dependency walk never sees it and the
# AppImage shipped without it — aborting on every desktop that does not
# install it itself, KDE on Arch being the common one (issue #31).
# Naming it explicitly makes linuxdeploy deploy it *and* its own deps.
APPINDICATOR_SO="$(ldconfig -p | awk '/libayatana-appindicator3\.so\.1/ {print $NF; exit}')"
if [[ -z "${APPINDICATOR_SO}" || ! -f "${APPINDICATOR_SO}" ]]; then
    echo "libayatana-appindicator3.so.1 not found via ldconfig." >&2
    echo "Install it before packaging (Debian/Ubuntu: libayatana-appindicator3-dev)." >&2
    exit 1
fi
echo "tray library: ${APPINDICATOR_SO}"

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
    --library "${APPINDICATOR_SO}" \
    --desktop-file "${APPDIR}/usr/share/applications/${APP_NAME}.desktop" \
    --icon-file "${ICON_PNG}" \
    --icon-filename "${APP_NAME}" \
    --output appimage

# The dlopen above is by soname with no path, so it resolves through
# the loader's search path rather than the AppDir's rpath. linuxdeploy
# points the AppRun at `usr/lib`, but assert the file is actually there
# — a silently un-deployed library is exactly the failure this step
# exists to prevent, and it would only surface on a user's machine.
if ! ls "${APPDIR}/usr/lib/"libayatana-appindicator3.so.1* >/dev/null 2>&1; then
    echo "linuxdeploy did not place libayatana-appindicator3.so.1 in the AppDir." >&2
    exit 1
fi

echo
echo "Built ${OUTPUT}"
ls -la "${OUTPUT}"
