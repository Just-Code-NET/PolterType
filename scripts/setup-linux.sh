#!/usr/bin/env bash
# Grant kb-switcher access to /dev/input/event* on Linux/Wayland.
#
# Wayland intentionally provides no global keyboard-snooping protocol,
# so the only realistic way to read keystrokes app-wide is via evdev.
# evdev needs:
#   1. The user to be in the `input` group.
#   2. A udev rule that gives the `input` group read access to
#      keyboard event devices.
#
# This script does both with one `sudo` prompt. Re-run any time —
# it's idempotent.

set -euo pipefail

USER_NAME="${SUDO_USER:-$USER}"
RULE_PATH="/etc/udev/rules.d/99-kb-switcher.rules"

if [[ "$(uname -s)" != "Linux" ]]; then
    echo "kb-switcher: setup-linux.sh is for Linux only." >&2
    exit 1
fi

cat <<EOF

kb-switcher Linux setup
=======================

This script will:
  1. Add user '${USER_NAME}' to the 'input' group (if not already).
  2. Install a udev rule at:
       ${RULE_PATH}
     granting the 'input' group read access to /dev/input/event*.
  3. Reload udev rules.

You will be prompted for your password.
EOF

read -r -p "Continue? [y/N] " ans
case "$ans" in
    y|Y|yes|YES) ;;
    *) echo "Aborted."; exit 0 ;;
esac

# 1. Add to input group.
if id -nG "$USER_NAME" | tr ' ' '\n' | grep -qx input; then
    echo "User '${USER_NAME}' is already in the 'input' group."
else
    echo "Adding '${USER_NAME}' to 'input' group…"
    sudo usermod -aG input "$USER_NAME"
fi

# 2. Install udev rule.
echo "Writing udev rule to ${RULE_PATH}…"
sudo tee "$RULE_PATH" >/dev/null <<'RULE'
# Installed by kb-switcher: scripts/setup-linux.sh
# Grants read access to keyboard event devices for the `input` group.
KERNEL=="event*", SUBSYSTEM=="input", GROUP="input", MODE="0640"
RULE

# 3. Reload.
echo "Reloading udev rules…"
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=input

cat <<EOF

Done. To pick up the new group membership, either:
  • log out and back in, OR
  • run: newgrp input

Then start kb-switcher.
EOF
