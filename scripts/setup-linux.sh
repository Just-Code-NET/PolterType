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
#
# On NixOS none of that can be done imperatively, so the same settings
# are written as a module next to configuration.nix and imported from
# it; `nixos-rebuild switch` is left to you.

set -euo pipefail

USER_NAME="${SUDO_USER:-$USER}"
RULE_PATH="/etc/udev/rules.d/99-poltertype.rules"
MODULES_LOAD_PATH="/etc/modules-load.d/uinput.conf"

ASSUME_YES=0
NIXOS=0
NIX_WROTE=0
for arg in "$@"; do
    case "$arg" in
        -y|--yes) ASSUME_YES=1 ;;
        -h|--help)
            sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
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
    echo "  ! '${USER_NAME}' is not in the 'input' group — run"
    echo "    'sudo nixos-rebuild switch', then log out and back in."
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
    if [[ ${NIX_WROTE:-0} -eq 1 ]]; then
        cat <<EOF

Expected — the configuration above is written but not yet applied. Run
'sudo nixos-rebuild switch', log out and back in, then re-run this
script: every line should read ✓.
EOF
    else
        cat <<EOF

Setup is NOT complete — see the lines marked ! above.
  https://github.com/Just-Code-NET/PolterType/blob/main/docs/PERMISSIONS.md
EOF
    fi
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

NIX_DIR="/etc/nixos"
NIX_MAIN="$NIX_DIR/configuration.nix"
NIX_MODULE="$NIX_DIR/poltertype.nix"
NIX_BACKUP="$NIX_MAIN.poltertype-backup"

# Write stdin to $1, with sudo only if the file is not ours to write.
# /etc/nixos is root-owned on a stock install and a symlink into a
# user-owned git repo on plenty of others; using sudo unconditionally
# would leave root-owned files in the second kind.
nix_write() {
    if [[ -w "$1" ]] || { [[ ! -e "$1" ]] && [[ -w "$(dirname "$1")" ]]; }; then
        cat >"$1"
    else
        sudo tee "$1" >/dev/null
    fi
}

nix_copy() {
    if [[ -w "$(dirname "$2")" ]]; then
        cp "$1" "$2"
    else
        sudo cp "$1" "$2"
    fi
}

# The block a user would otherwise paste by hand. Also what gets printed
# when this script decides not to touch their configuration.
nixos_manual_block() {
    cat <<EOF
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
EOF
}

nixos_module_body() {
    cat <<EOF
# PolterType — written by scripts/setup-linux.sh, imported from
# configuration.nix. Delete both when you uninstall PolterType.
#
# Everything here is what other distributions get imperatively from that
# script: on NixOS the udev rules live in the read-only Nix store, and
# group membership is decided by users.users.<name>.extraGroups, which
# the activation script re-applies over anything usermod did.
{ ... }:

{
  # Loads the uinput module and gives its device node to the \`uinput\`
  # group, so the corrector can type the fixed word back.
  hardware.uinput.enable = true;

  # \`input\` carries read access to /dev/input/event*, \`uinput\` the
  # write access above. List options merge, so this adds to whatever the
  # rest of the configuration already grants the account.
  users.users."${USER_NAME}".extraGroups = [ "input" "uinput" ];

  # NixOS has no /lib64/ld-linux-x86-64.so.2, so a generic AppImage
  # cannot be exec'd at all; binfmt makes the .AppImage file itself
  # runnable — which is what PolterType's desktop entry, its autostart
  # entry and its updater all launch.
  programs.appimage = {
    enable = true;
    binfmt = true;
  };
}
EOF
}

# Make the new module visible to a flake.
#
# `nixos-rebuild` on a flake evaluates the configuration from the git
# tree, not the working directory, and a file git has never heard of is
# not in it: the import resolves to nothing and the rebuild fails on the
# import rather than on the file. So stage it — and nothing else, since
# whatever else is uncommitted in there is the user's business.
nix_git_track() {
    git -C "$NIX_DIR" rev-parse --is-inside-work-tree >/dev/null 2>&1 || return 0
    if git -C "$NIX_DIR" add -- poltertype.nix 2>/dev/null; then
        echo "Staged poltertype.nix — a flake evaluates the git tree, not the directory."
    else
        echo "  ! ${NIX_DIR} is a git repository and poltertype.nix is untracked, which"
        echo "    a flake-based rebuild cannot see. Stage it yourself:"
        echo "      git -C \"${NIX_DIR}\" add poltertype.nix"
    fi
}

# Add `./poltertype.nix` to the `imports` list of $1, on stdout.
#
# Appended just before the list's closing bracket rather than after the
# opening one: `imports =` and its `[` are often on separate lines, and
# that `[` is usually followed by the comment about the hardware scan,
# which an entry inserted after it would silently adopt.
nix_add_import() {
    awk '
        BEGIN { state = 0 }
        state == 0 && /^[[:space:]]*imports[[:space:]]*=/ { state = 1 }
        state == 1 && index($0, "]") > 0 {
            close_at = index($0, "]")
            before = substr($0, 1, close_at - 1)
            if (before ~ /[^[:space:]\[]/ && before ~ /\[/) {
                # A one-line list: `imports = [ ./hardware.nix ];`
                sub(/[[:space:]]+$/, "", before)
                print before " ./poltertype.nix " substr($0, close_at)
            } else {
                match($0, /^[[:space:]]*/)
                print substr($0, 1, RLENGTH) "  ./poltertype.nix"
                print
            }
            state = 2
            next
        }
        { print }
        END { if (state != 2) exit 3 }
    ' "$1"
}

# Nix files that do not parse take the whole system down at the next
# rebuild, and this script writes one and edits another. `--parse` is
# syntax only: no evaluation, no network, no store writes.
nix_parses() {
    if command -v nix-instantiate >/dev/null 2>&1; then
        nix-instantiate --parse "$1" >/dev/null 2>&1
    else
        true
    fi
}

nixos_setup() {
    cat <<EOF

poltertype setup — NixOS detected
=================================

Nothing here can be set up imperatively: udev rules live in the
read-only Nix store, and a group added with usermod is dropped again by
the next rebuild. The same three settings go into the configuration
instead, as a module of their own.

EOF

    if [[ ! -f "$NIX_MAIN" ]]; then
        echo "No ${NIX_MAIN} — this system's configuration lives somewhere else."
        echo "Add this to it by hand, wherever it is:"
        echo
        nixos_manual_block
        echo
        echo "Then \`sudo nixos-rebuild switch\`, and log out and back in."
        return
    fi

    # `users.users.<name>.extraGroups` on an account this configuration
    # does not declare creates a half-defined user, and the rebuild fails
    # on it — a confusing break, and one this script would have caused.
    if ! grep -qsE "users\.users\.\"?${USER_NAME}\"?" "$NIX_DIR"/*.nix; then
        echo "Could not find where '${USER_NAME}' is declared under ${NIX_DIR},"
        echo "so this script will not edit anything. Add this by hand, next to"
        echo "that account's own definition:"
        echo
        nixos_manual_block
        echo
        echo "Then \`sudo nixos-rebuild switch\`, and log out and back in."
        return
    fi

    echo "This script will:"
    echo "  1. Write ${NIX_MODULE}."
    echo "  2. Add ./poltertype.nix to the 'imports' list in ${NIX_MAIN}"
    echo "     (keeping a copy at ${NIX_BACKUP})."
    echo "  3. Leave 'nixos-rebuild switch' to you — it is your system's"
    echo "     rebuild, and it can fail on things that have nothing to do"
    echo "     with PolterType."
    echo

    if [[ $ASSUME_YES -ne 1 ]]; then
        if ! read -r -p "Continue? [y/N] " ans; then
            ans=""
            echo
        fi
        case "$ans" in
            y|Y|yes|YES) ;;
            *) echo "Aborted."; exit 0 ;;
        esac
    fi

    NIX_WROTE=1
    nixos_module_body | nix_write "$NIX_MODULE"
    if ! nix_parses "$NIX_MODULE"; then
        echo "  ! ${NIX_MODULE} does not parse — this is a bug in this script."
        echo "    Delete it and add the block above by hand."
        return
    fi
    echo "Wrote ${NIX_MODULE}."
    nix_git_track


    if grep -q 'poltertype\.nix' "$NIX_MAIN"; then
        echo "${NIX_MAIN} already imports it."
    else
        nix_copy "$NIX_MAIN" "$NIX_BACKUP"
        patched=$(mktemp)
        if ! nix_add_import "$NIX_MAIN" >"$patched"; then
            echo "  ! No 'imports = [ … ];' list found in ${NIX_MAIN}."
            echo "    Add this line to it by hand:  ./poltertype.nix"
        else
            nix_write "$NIX_MAIN" <"$patched"
            if nix_parses "$NIX_MAIN"; then
                echo "Added ./poltertype.nix to the imports in ${NIX_MAIN}."
            else
                nix_copy "$NIX_BACKUP" "$NIX_MAIN"
                echo "  ! The edited ${NIX_MAIN} did not parse; restored the original."
                echo "    Add this line to its 'imports' list by hand:  ./poltertype.nix"
            fi
        fi
        rm -f "$patched"
    fi

    cat <<EOF

Now run:

  sudo nixos-rebuild switch

and log out and back in — the groups do not reach a session that started
before them. Re-run this script afterwards to check the result.
EOF
}

# NixOS: nothing below can be applied imperatively, and the parts that
# look like they can are worse than useless — /etc/udev/rules.d is a
# read-only symlink into the Nix store, and a group added with usermod
# is dropped again by the next rebuild, because
# `users.users.<name>.extraGroups` is what decides membership. So the
# same job is done declaratively: a module of our own next to
# configuration.nix, and one line added to its `imports`.
if [[ -e /etc/NIXOS ]] || grep -qs '^ID=nixos$' /etc/os-release; then
    NIXOS=1
    nixos_setup
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
