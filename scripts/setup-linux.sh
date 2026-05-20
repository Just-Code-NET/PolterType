#!/usr/bin/env bash
# Grant kb-switcher access to /dev/input/event* and /dev/uinput on
# Linux/Wayland.
#
# Wayland intentionally provides no global keyboard-snooping protocol,
# so the only realistic way to read keystrokes app-wide is via evdev,
# and the only realistic way to inject synthetic keystrokes (for the
# corrector that backspaces the wrong-layout word and re-types it) is
# uinput. Both need:
#   1. The user to be in the `input` group.
#   2. udev rules that give the `input` group access to
#      /dev/input/event* (read) and /dev/uinput (read+write).
#
# This script does both with one `sudo` prompt. Re-run any time —
# it's idempotent. Pass `--yes` / `-y` to skip the confirmation prompt
# (useful for unattended provisioning).

set -euo pipefail

USER_NAME="${SUDO_USER:-$USER}"
RULE_PATH="/etc/udev/rules.d/99-kb-switcher.rules"
MODULES_LOAD_PATH="/etc/modules-load.d/uinput.conf"

ASSUME_YES=0
for arg in "$@"; do
    case "$arg" in
        -y|--yes) ASSUME_YES=1 ;;
        -h|--help)
            sed -n '1,18p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo "kb-switcher: unknown argument: $arg" >&2; exit 2 ;;
    esac
done

if [[ "$(uname -s)" != "Linux" ]]; then
    echo "kb-switcher: setup-linux.sh is for Linux only." >&2
    exit 1
fi

cat <<EOF

kb-switcher Linux setup
=======================

This script will:
  1. Add user '${USER_NAME}' to the 'input' group (if not already).
  2. Ensure the 'uinput' kernel module loads on boot
       (${MODULES_LOAD_PATH}).
  3. Install a udev rule at:
       ${RULE_PATH}
     granting the 'input' group:
       - read access to /dev/input/event*
       - read+write access to /dev/uinput
  4. Reload udev rules and re-trigger input devices.

You will be prompted for your password.
EOF

if [[ $ASSUME_YES -ne 1 ]]; then
    read -r -p "Continue? [y/N] " ans
    case "$ans" in
        y|Y|yes|YES) ;;
        *) echo "Aborted."; exit 0 ;;
    esac
fi

# 1. Add to input group.
if id -nG "$USER_NAME" | tr ' ' '\n' | grep -qx input; then
    echo "User '${USER_NAME}' is already in the 'input' group."
else
    echo "Adding '${USER_NAME}' to 'input' group…"
    sudo usermod -aG input "$USER_NAME"
fi

# 2. Make sure uinput is available at boot.
echo "Writing modules-load.d entry to ${MODULES_LOAD_PATH}…"
sudo tee "$MODULES_LOAD_PATH" >/dev/null <<'MOD'
# Installed by kb-switcher: scripts/setup-linux.sh
# Loads the uinput kernel module on boot so kb-switcher can synthesise
# keystrokes via /dev/uinput on Wayland sessions.
uinput
MOD
# Load it now too if it isn't already, so the udev rule below can
# actually find the device to chgrp.
if ! lsmod | grep -q '^uinput'; then
    sudo modprobe uinput || true
fi

# 3. Install udev rule.
echo "Writing udev rule to ${RULE_PATH}…"
sudo tee "$RULE_PATH" >/dev/null <<'RULE'
# Installed by kb-switcher: scripts/setup-linux.sh
# Grants the `input` group:
#   - read access to keyboard event devices
#   - read+write access to /dev/uinput (so the corrector can backspace
#     the wrong-layout word and re-type it on Wayland)
KERNEL=="event*", SUBSYSTEM=="input", GROUP="input", MODE="0640"
KERNEL=="uinput", SUBSYSTEM=="misc", GROUP="input", MODE="0660", \
    OPTIONS+="static_node=uinput"
RULE

# 4. Reload.
echo "Reloading udev rules…"
sudo udevadm control --reload-rules
# `--action=change` is the one that re-evaluates GROUP/MODE on devices
# that already exist. Without it, udev no-ops on misc nodes that were
# created on the previous boot (which is the common case for
# `/dev/uinput` on a long-lived session), and the rule appears to
# silently do nothing.
sudo udevadm trigger --action=change --subsystem-match=input
sudo udevadm trigger --action=change --subsystem-match=misc --attr-match=name=uinput || true
sudo udevadm settle || true

# Belt-and-braces: if the trigger above somehow didn't take effect
# (older udev, weird initramfs, etc.), force the right ownership now
# so the user doesn't have to reboot just to test.
if [[ -e /dev/uinput ]]; then
    sudo chgrp input /dev/uinput || true
    sudo chmod 0660 /dev/uinput || true
fi

cat <<EOF

Done. To pick up the new group membership, either:
  • log out and back in, OR
  • run: newgrp input

Then start kb-switcher.
EOF
