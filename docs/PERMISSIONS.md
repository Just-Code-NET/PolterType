# Permissions per OS

`kb-switcher` is a tray-only background app that needs to **observe**
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
> *kb-switcher*.

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
sudo tee /etc/udev/rules.d/99-kb-switcher.rules <<'EOF'
KERNEL=="event*", SUBSYSTEM=="input", GROUP="input", MODE="0640"
EOF
sudo udevadm control --reload-rules && sudo udevadm trigger
# log out and back in, or run `newgrp input`
```

### Option B — AT-SPI (no `sudo` required, less reliable)

If the user's accessibility bus is enabled (default on GNOME, opt-in
on KDE), `kb-switcher` can subscribe to keyboard events via the
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

### Switching layout on Wayland

Pure D-Bus (no special permissions). Backends, in priority order:

1. `org.gnome.desktop.input-sources` (GNOME).
2. `org.kde.keyboard.layouts` (KDE).
3. IBus (`org.freedesktop.IBus`) and Fcitx (`org.fcitx.Fcitx`)
   for users running an input method.

If none respond, the tray surfaces a *layout switching unavailable*
banner with a link back to this document.
