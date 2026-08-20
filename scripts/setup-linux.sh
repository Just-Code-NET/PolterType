#!/usr/bin/env bash
# Grant poltertype access to /dev/input/event* and /dev/uinput on
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
RULE_PATH="/etc/udev/rules.d/99-poltertype.rules"
MODULES_LOAD_PATH="/etc/modules-load.d/uinput.conf"

ASSUME_YES=0
NIXOS=0
for arg in "$@"; do
    case "$arg" in
        -y|--yes) ASSUME_YES=1 ;;
        -h|--help)
            sed -n '1,18p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo "poltertype: unknown argument: $arg" >&2; exit 2 ;;
    esac
done

if [[ "$(uname -s)" != "Linux" ]]; then
    echo "poltertype: setup-linux.sh is for Linux only." >&2
    exit 1
fi

# 5. Verify, instead of assuming.
#
# Every step can "succeed" and leave the app unable to read a keyboard:
# usermod adds whoever ran the script (root, under a bare `sudo -i`),
# and a udev rule that never reaches the existing nodes leaves them
# owned by someone else. Reported as issue #31 by a user who ran this
# script twice and was told to run it again.
#
# It is a function because the NixOS branch below ends on it too, having
# applied nothing of its own.
verify_and_report() {
echo
echo "Checking the result…"
FAILED=0

if [[ "$USER_NAME" == "root" ]]; then
    echo "  ! Added 'root' to the 'input' group, not your account — this ran as"
    echo "    root with no SUDO_USER. Re-run it as yourself: bash $0"
    FAILED=1
elif id -nG "$USER_NAME" | tr ' ' '\n' | grep -qx input; then
    echo "  ✓ '${USER_NAME}' is in the 'input' group."
elif [[ $NIXOS -eq 1 ]]; then
    echo "  ! '${USER_NAME}' is not in the 'input' group — add it above, rebuild, and"
    echo "    log out and back in."
    FAILED=1
else
    echo "  ! '${USER_NAME}' is still not in the 'input' group — usermod did not take."
    FAILED=1
fi

NODES=$(find /dev/input -maxdepth 1 -name 'event*' 2>/dev/null | wc -l)
if [[ "$NODES" -eq 0 ]]; then
    echo "  ! No /dev/input/event* devices exist on this machine."
    FAILED=1
else
    # Group name plus the group-read bit of the symbolic mode: the two
    # things that decide whether the app can open the node at all.
    UNREADABLE=$(stat -c '%n %G %A' /dev/input/event* \
        | awk '$2 != "input" || substr($3, 5, 1) != "r" { print "      " $1 " (" $2 " " $3 ")" }')
    if [[ -n "$UNREADABLE" ]]; then
        echo "  ! These devices are not readable by the 'input' group:"
        echo "$UNREADABLE"
        echo "    The udev rule did not reach them. A reboot applies it for certain."
        FAILED=1
    else
        echo "  ✓ All ${NODES} input devices are readable by the 'input' group."
    fi
fi

if [[ ! -e /dev/uinput ]]; then
    echo "  ! /dev/uinput is missing — the uinput module is not loaded."
    FAILED=1
else
    # Which group owns the node is distro business — the rule above says
    # `input`, NixOS ships its own saying `uinput`. What decides whether
    # the corrector can type anything back is that the account is in
    # whichever group it is, and that the group may write.
    UINPUT_GROUP=$(stat -c '%G' /dev/uinput)
    UINPUT_MODE=$(stat -c '%A' /dev/uinput)
    if ! id -nG "$USER_NAME" | tr ' ' '\n' | grep -qx "$UINPUT_GROUP"; then
        echo "  ! /dev/uinput belongs to group '${UINPUT_GROUP}', which '${USER_NAME}' is not in"
        echo "    — PolterType could read your typing but not type the correction back."
        FAILED=1
    elif [[ "${UINPUT_MODE:4:2}" != "rw" ]]; then
        echo "  ! /dev/uinput is ${UINPUT_MODE} — group '${UINPUT_GROUP}' cannot write to it."
        FAILED=1
    else
        echo "  ✓ /dev/uinput is writable by '${USER_NAME}' (${UINPUT_GROUP} ${UINPUT_MODE})."
    fi
fi

if [[ $FAILED -ne 0 ]]; then
    cat <<EOF

Setup is NOT complete — see the lines marked ! above.
  https://github.com/Just-Code-NET/PolterType/blob/main/docs/PERMISSIONS.md
EOF
    return 1
fi

if id -nG | tr ' ' '\n' | grep -qx input; then
    cat <<EOF

Done — this session already carries the 'input' group. Start poltertype
(or restart it, if it was running before this script).
EOF
else
    cat <<EOF

Done, but this session was started before the group was granted, so it
does not carry it yet. Log out and back in.

  'newgrp input' is not a substitute: it grants the group to that one
  shell only, so poltertype has to be started from that same shell. An
  app launched from the desktop or the tray will still see nothing.
EOF
fi

return 0
}

# NixOS: every mutation below is either impossible or undone on the next
# rebuild, so refuse to half-apply it and print the declarative
# equivalent instead. /etc/udev/rules.d is a read-only symlink into the
# Nix store, and group membership belongs to
# `users.users.<name>.extraGroups`, which the activation script
# re-applies over whatever usermod did.
if [[ -e /etc/NIXOS ]] || grep -qs '^ID=nixos$' /etc/os-release; then
    NIXOS=1
    cat <<EOF

poltertype setup — NixOS detected
=================================

This script cannot do its work here: udev rules live in the read-only
Nix store, and a group added with usermod is dropped again on the next
rebuild. Add the equivalent to /etc/nixos/configuration.nix:

  # Loads the uinput module and gives its device node to the \`uinput\`
  # group, so the corrector can type the fixed word back.
  hardware.uinput.enable = true;

  # Read keystrokes from /dev/input/event* (\`input\`); write the
  # correction to /dev/uinput (\`uinput\`).
  users.users."${USER_NAME}".extraGroups = [ "input" "uinput" ];

  # NixOS has no /lib64/ld-linux-x86-64.so.2, so a generic AppImage
  # cannot be exec'd at all; \`binfmt\` makes the .AppImage file itself
  # runnable — which is what PolterType's desktop and autostart entries
  # launch.
  programs.appimage = { enable = true; binfmt = true; };

Then \`sudo nixos-rebuild switch\`, and log out and back in.
EOF
    verify_and_report
    exit $?
fi

cat <<EOF

poltertype Linux setup
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
# Installed by poltertype: scripts/setup-linux.sh
# Loads the uinput kernel module on boot so poltertype can synthesise
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
# Installed by poltertype: scripts/setup-linux.sh
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

verify_and_report
