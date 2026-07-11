# Permissions per OS

`poltertype` is a tray-only background app that needs to **observe**
keystrokes and **send** synthetic ones to correct words. Different
OSes guard those capabilities differently.

## Windows

**No special permission needed.** The app installs a `WH_KEYBOARD_LL`
hook on its own message-pump thread and reads
`GetKeyboardLayoutList` / `GetKeyboardLayout` for layout state.

If a SmartScreen / antivirus warning appears for unsigned builds,
that's expected — release artifacts will be signed in a later phase.

## macOS

The app needs **Accessibility** permission, granted once per machine:

> System Settings → Privacy & Security → Accessibility → enable
> *PolterType*.

Why: `CGEventTapCreate(kCGSessionEventTap, …)` (used to listen) and
`CGEventPost` (used to send corrections) both require this.

> **Planned, not built:** a first-launch onboarding window walking the
> user through the toggle, and a tray entry surfacing "permission
> denied". Today the user has to know to grant Accessibility on their
> own. The macOS backend as a whole is CI-validated but has not been
> runtime-tuned on hardware.

## Linux

Wayland (the default on modern GNOME/KDE/Hyprland/Sway) intentionally
provides **no protocol for global keyboard snooping** — that's a
security feature, not a bug. The realistic options are:

### Option A — `evdev` (recommended; works on every Wayland compositor)

Read raw events from `/dev/input/event*`. Permissions:

* the user must be in the `input` group, **and**
* a udev rule must grant the group read access to keyboard event
  devices.

`scripts/setup-linux.sh` does both with a single `sudo` prompt (it
also grants `/dev/uinput`, needed to send the correction back).
Equivalent manual commands:

```bash
sudo usermod -aG input "$USER"
sudo tee /etc/udev/rules.d/99-poltertype.rules <<'EOF'
KERNEL=="event*", SUBSYSTEM=="input", GROUP="input", MODE="0640"
EOF
sudo udevadm control --reload-rules && sudo udevadm trigger
# log out and back in, or run `newgrp input`
```

### Option B — AT-SPI (planned, not implemented)

**This does not exist yet.** There is no `atspi` dependency and no
AT-SPI listener in `poltertype-input`; on Wayland, option A is
currently the only path. The idea: if the user's accessibility bus is
enabled (default on GNOME, opt-in on KDE), subscribe to keyboard
events via the `atspi` crate — no `sudo`, but higher latency, and some
inputs (especially in non-toolkit apps) are missed. It would serve as
a fallback when option A is not available.

### Option C — X11

On X11 sessions we select `XInput2` `RawKeyPress` / `RawKeyRelease` on
the root window, and send corrections back with `XTestFakeInput`.

**No permission of any kind is required** — no `input` group, no udev
rule, no `sudo`, no setup script. Any client that can open the display
can select raw events, which makes X11 the one Linux session type where
poltertype works the moment it is installed. (It is also why we don't
grab the keyboard: a grab would make us the *only* recipient of the
keystrokes and stop the user typing into anything else.)

Detected automatically: `XDG_SESSION_TYPE=x11`, or — for the bare-WM
setups that never set it — `DISPLAY` present with no `WAYLAND_DISPLAY`.
Under XWayland both are set, and there the compositor owns input, so we
correctly take the Wayland path instead.

### Sending keys (corrections) on Wayland

* `uinput`, via the same device permissions `setup-linux.sh` grants.
  **This is the only implemented path** — which is why the setup
  script covers `/dev/uinput` as well as `/dev/input/event*`.
* `libei` through the `org.freedesktop.portal.RemoteDesktop` /
  `InputCapture` portal (KDE Plasma 6.0+, GNOME 46+) is the planned
  no-`sudo` alternative. **Not implemented** — there is no portal code
  in the tree today.

### Switching layout

Switching uses whichever backend is alive in the session. No `sudo`
required — every backend talks over the user's session bus or via
the canonical CLI tool of its ecosystem. Backends, in priority order:

1. **Hyprland** (`hyprctl switchxkblayout`) — when
   `HYPRLAND_INSTANCE_SIGNATURE` is set.
2. **KDE Plasma** (`qdbus6`/`qdbus` → `org.kde.keyboard /Layouts`).
3. **GSettings** (`gsettings org.gnome.desktop.input-sources`) —
   covers **GNOME**, **Ubuntu Unity 7+**, **Cinnamon**, **Budgie**,
   **Pantheon** (elementary OS), **MATE**. The probe requires the
   schema to be installed *and* to list at least one input source:
   the schema ships with GTK, so it is present on plenty of machines
   running no GNOME-family desktop at all, where it reads back empty.
4. **IBus** (`ibus engine`) — any DE hosting IBus.
5. **Fcitx5** (`fcitx5-remote -s …`) — any DE hosting Fcitx.
6. **X11 XKB** (`XkbLatchLockState` via `x11rb`) — locks the XKB
   group, which is what the layouts in `setxkbmap -layout us,ua`
   actually are. This is the bare-WM fallback (i3, openbox, plain
   `.xinitrc`), where no desktop environment owns the layout. Probed
   last on purpose: where a DE *is* present it keeps a tray indicator
   in sync with the layout, and locking the group underneath it would
   switch the keyboard while leaving that indicator lying. Stands down
   entirely under XWayland, where the compositor owns layout.

If none respond, layout switching is unavailable and the failure is
logged. (A tray banner pointing back at this document is planned but
not implemented — today the only signal is the log.)
