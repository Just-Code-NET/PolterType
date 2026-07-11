# Permissions per OS

`poltertype` is a tray-only background app that needs to **observe**
keystrokes and (eventually) **send** synthetic ones to correct words.
Different OSes guard those capabilities differently.

## Windows

**No special permission needed.** The app installs a `WH_KEYBOARD_LL`
hook on its own message-pump thread and reads
`GetKeyboardLayoutList` / `GetKeyboardLayout` for layout state.

If a SmartScreen / antivirus warning appears for unsigned builds,
that's expected — release artifacts will be signed in a later phase.

## macOS

The app needs **Accessibility** permission, granted once per machine:

> System Settings → Privacy & Security → Accessibility → enable
> *Poltertype*.

Why: `CGEventTapCreate(kCGSessionEventTap, …)` (used to listen) and
`CGEventPost` (used to send corrections) both require this. The first
launch shows an onboarding window with a GIF that walks the user
through the toggle. The app exits cleanly if permission is denied
and surfaces the situation in the tray menu.

## Linux

Wayland (the default on modern GNOME/KDE/Hyprland/Sway) intentionally
provides **no protocol for global keyboard snooping** — that's a
security feature, not a bug. The realistic options are:

### Option A — `evdev` (recommended; works on every Wayland compositor)

Read raw events from `/dev/input/event*`. Permissions:

* the user must be in the `input` group, **and**
* a udev rule must grant the group read access to keyboard event
  devices.

`scripts/setup-linux.sh` (ships in Phase 6) does both with a single
`sudo` prompt. Equivalent manual commands:

```bash
sudo usermod -aG input "$USER"
sudo tee /etc/udev/rules.d/99-poltertype.rules <<'EOF'
KERNEL=="event*", SUBSYSTEM=="input", GROUP="input", MODE="0640"
EOF
sudo udevadm control --reload-rules && sudo udevadm trigger
# log out and back in, or run `newgrp input`
```

### Option B — AT-SPI (no `sudo` required, less reliable)

If the user's accessibility bus is enabled (default on GNOME, opt-in
on KDE), `poltertype` can subscribe to keyboard events via the
`atspi` crate. Latency is higher and some inputs (especially in
non-toolkit apps) are missed. Used as a fallback when option A is
not available.

### Option C — X11 (legacy)

On X11 sessions (`XDG_SESSION_TYPE=x11`) we use `XInput2 RawKeyPress`
events. No special permission needed. Detected automatically.

### Sending keys (corrections) on Wayland

Two paths, picked at runtime:

* `uinput` via the same evdev device permissions (preferred when
  option A is in use).
* `libei` through the `org.freedesktop.portal.RemoteDesktop` /
  `InputCapture` portal — works on KDE Plasma 6.0+ and GNOME 46+.

### Switching layout

Switching uses whichever backend is alive in the session. No `sudo`
required — every backend talks over the user's session bus or via
the canonical CLI tool of its ecosystem. Backends, in priority order:

1. **Hyprland** (`hyprctl switchxkblayout`) — when
   `HYPRLAND_INSTANCE_SIGNATURE` is set.
2. **KDE Plasma** (`qdbus6`/`qdbus` → `org.kde.keyboard /Layouts`).
3. **GSettings** (`gsettings org.gnome.desktop.input-sources`) —
   covers **GNOME**, **Ubuntu Unity 7+**, **Cinnamon**, **Budgie**,
   **Pantheon** (elementary OS), **MATE**. The probe only matches
   when the schema is actually installed.
4. **IBus** (`ibus engine`) — any DE hosting IBus.
5. **Fcitx5** (`fcitx5-remote -s …`) — any DE hosting Fcitx.

If none respond, the tray surfaces a *layout switching unavailable*
banner with a link back to this document.
