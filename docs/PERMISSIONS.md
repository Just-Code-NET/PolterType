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

The app needs **two** permissions, each granted once per machine:

> System Settings → Privacy & Security → **Accessibility** → enable
> *PolterType*.
>
> System Settings → Privacy & Security → **Input Monitoring** → enable
> *PolterType*.

Why: `CGEventTapCreate(kCGSessionEventTap, …)` (used to listen) and
`CGEventPost` (used to send corrections) require Accessibility;
delivery of the key events themselves requires Input Monitoring.
macOS prompts for both — Input Monitoring when the tap is created,
Accessibility when the tap fails to attach without it. Granting only
one leaves the app running with a tray icon and no corrections, which
is why the tray alert below matters.

Since v0.15.0 the Accessibility grant does a second job: placing the
suggestion tooltip. PolterType asks the focused application, over the
same API, which window has focus and where the text caret is — its
position and size, never its contents, and only at the moment a
suggestion is about to be shown. No text is read out of any
application, on any platform; that is the same boundary the engine
works under.

**Expect to grant both again after an update.** macOS attaches these
permissions to a code identity, and ours is an ad-hoc signature (see
`docs/CODE_SIGNING.md`), whose identity is the hash of the app's own
bytes — a new version is, to macOS, different software. A Developer ID
would carry the grant across updates; until then this is the cost of
the signature that makes the permissions work at all. Builds before
v0.14.4 had no usable identity on Intel Macs, where the permission
could be granted but never took effect.

**Autostart is a third, lighter capability.** With *"Start
automatically when I sign in"* enabled, the app writes a per-user
LaunchAgent to `~/Library/LaunchAgents/dev.opensource.poltertype.plist`
and registers it with `launchctl`. macOS shows a one-time "login item
added" notification. No elevation is involved and nothing is written
outside the user's own home directory; unticking the setting deletes
the file.

> **What exists (0.7.0).** When the keyboard hooks fail to start — the
> usual cause on macOS being exactly this permission — the tray shows a
> **⚠ Keyboard hooks unavailable — Setup…** entry, a tooltip warning,
> and a one-shot notification. The entry opens the Settings window on
> its **Setup** pane (also reachable as `poltertype --setup`), which
> probes *this* machine: on macOS it reports Accessibility and Input
> Monitoring separately, offers to raise each system prompt, and deep-
> links into the matching System Settings pane; on Wayland it reports
> read access and uinput access separately, and distinguishes "not set
> up" from "set up, but this login session predates it". **Check
> again** re-probes. The pane also carries the "layout switching
> unavailable" banner when no switcher backend could be built.
>
> **Still planned, not built:** the screenshots / GIFs of the macOS
> toggles, and showing any of this *before* something fails rather than
> after. The pane is also, like the rest of the macOS backend,
> compiled by CI and never yet run on a Mac.

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
also grants `/dev/uinput`, needed to send the correction back). Since
v0.17.4 the AppImage carries its own copy under
`usr/share/poltertype/scripts/`, and PolterType's error message names
the path it is actually running from — there is nothing to clone.
Equivalent manual commands:

```bash
sudo usermod -aG input "$USER"
sudo tee /etc/udev/rules.d/99-poltertype.rules <<'EOF'
KERNEL=="event*", SUBSYSTEM=="input", GROUP="input", MODE="0640"
EOF
sudo udevadm control --reload-rules && sudo udevadm trigger
# log out and back in, or run `newgrp input`
```

#### NixOS

None of the above can be applied imperatively there — `/etc/udev/rules.d`
is a read-only symlink into the Nix store, and a group added with
`usermod` is dropped again by the next rebuild — so the script does the
same job declaratively. It writes `/etc/nixos/poltertype.nix`:

```nix
{ ... }:

{
  # Loads the uinput module and gives its device node to the `uinput`
  # group; `input` is what carries read access to /dev/input/event*.
  hardware.uinput.enable = true;
  users.users."<you>".extraGroups = [ "input" "uinput" ];

  # NixOS has no /lib64/ld-linux-x86-64.so.2, so a generic AppImage
  # cannot be exec'd at all. `binfmt` makes the .AppImage file itself
  # runnable — without it `appimage-run poltertype-…AppImage` still
  # works, but the desktop entry, the autostart entry and the updater
  # all launch the file by path, so they do not.
  programs.appimage = { enable = true; binfmt = true; };
}
```

and adds one line, `./poltertype.nix`, to the `imports` list in
`configuration.nix`, keeping the original as
`configuration.nix.poltertype-backup`. Both files are checked with
`nix-instantiate --parse` before they are left in place; a config that
does not parse is restored from the backup rather than handed to your
next rebuild.

If `/etc/nixos` is a git repository — which it is whenever the system is
built from a flake — the new module is staged with `git add`, and
nothing else is. A flake evaluates the *git tree*, not the directory, so
an untracked `poltertype.nix` is invisible to the very rebuild that
imports it, and `nixos-rebuild` fails on the import.

What it will not do:

* **Run `nixos-rebuild switch`.** That is yours — it can fail on things
  that have nothing to do with PolterType, and you want to be able to
  tell the two apart. Run it, then log out and back in.
* **Touch a configuration it does not understand.** No
  `/etc/nixos/configuration.nix`, no `imports` list in it, or no sign of
  your account being declared under `/etc/nixos` — and it prints the
  block for you to paste instead, having changed nothing. (An
  `extraGroups` line for an account the configuration does not declare
  is a rebuild failure, not a fix.)

Re-running it after the rebuild changes nothing and checks the result.

#### When it looks set up and still does not work

All three of these leave a machine that passes a casual look and reads
no keys. Since v0.17.4 the script checks its own work and exits non-zero
rather than printing "Done", and PolterType's own error names which one
it hit instead of telling you to run the script again.

* **`newgrp input` is not a login.** It grants the group to that one
  shell, so PolterType has to be started *from that shell*. Launched
  from the desktop, the tray or an autostart entry, it still sees
  nothing. Log out and back in instead.
* **The udev rule never reached the devices that already existed.**
  `stat -c '%n %G %A' /dev/input/event*` must show `input` and a group
  `r` bit on every line. If it does not, re-run the script (it
  re-triggers udev) or reboot.
* **The script ran as root without `sudo`.** Then `root` is what was
  added to the `input` group, not your account.

### Autostart on a bare compositor

"Start automatically when I sign in" installs a systemd **user
service**, `~/.config/systemd/user/dev.opensource.poltertype.service`,
wanted by `graphical-session.target`. That target is the one thing on a
Linux desktop that means "the session is up and its environment is
published", which is exactly what PolterType needs to be started after:
a compositor socket to talk to, and a `WAYLAND_DISPLAY` / `DISPLAY` to
inherit.

GNOME, KDE, Xfce and anything launched through `uwsm` reach that target
on their own — nothing to do. A **bare Hyprland, Sway or river session
does not**, and neither does it read `~/.config/autostart`; PolterType
says so in the log when it installs the unit into a session that has no
such target, because the alternative is a toggle that silently does
nothing until the next login proves it.

Wiring it once, the way Sway's own package does it — a session target
of your own that pulls in the standard one:

```ini
# ~/.config/systemd/user/hyprland-session.target
[Unit]
Description=Hyprland session
BindsTo=graphical-session.target
Wants=graphical-session-pre.target
After=graphical-session-pre.target
```

and, in `hyprland.conf`, after the environment is published:

```bash
exec-once = dbus-update-activation-environment --systemd --all && \
            systemctl --user start hyprland-session.target
```

Order matters more than it looks: a unit started before that
`dbus-update-activation-environment` inherits a session environment
that does not exist yet. PolterType now survives being early — it
re-probes for a compositor for 15 s and resolves Hyprland from its
socket directory rather than from an environment variable — but a
process with no `WAYLAND_DISPLAY` at all still has no tray to live in.

`~/.config/autostart/dev.opensource.poltertype.desktop` is still
written on a machine with **no** systemd user manager, and removed when
there is one: two mechanisms would start two copies, and the second one
loses to the instance lock with a message that reads like a fault.

### Option B — AT-SPI as a listener: measured, and decided against

The idea was to subscribe to keyboard events on the accessibility bus
and need no `sudo` at all. It does not work:
`RegisterKeystrokeListener` returns false on a wlroots session and
delivers nothing even with injected keys, because `at-spi2-registryd`
can only relay what the compositor hands it — and only Mutter does.
Where it *would* work (X11) there is already a listener that needs no
permissions either, so it would add nothing. See `docs/DECISIONS.md`,
2026-08-01. On Wayland, option A is the listening path.

### The accessibility bus IS used — for the caret, not for keys

Since v0.5.0 `poltertype-input` connects to the session's AT-SPI
bus (plain user-session IPC — no group, no `sudo`, no network) for
one narrow purpose: the **suggestion tooltip's position**. It
subscribes to `object:text-caret-moved` events and asks the focused
widget for the caret's *rectangle* (`GetCharacterExtents`), so the
tooltip can appear next to the text being typed. It never requests
text content — coordinates only, and nothing is logged.

On startup it also raises the session flag
`org.a11y.Status.IsEnabled` (the same flag screen readers raise):
toolkits keep their accessibility bridges dormant until some
assistive client sets it. Apps started while the flag is up expose
caret positions; apps that predate it stay silent until restarted —
PolterType then falls back to pointer/window anchoring. The flag is
session-scoped, is never unset by PolterType (unsetting could break
a real screen reader started later), and disappears at logout. If
the a11y stack is absent or disabled, everything degrades silently.

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

* `uinput`, via the same device permissions `setup-linux.sh` grants —
  the path that actually carries corrections, which is why the setup
  script covers `/dev/uinput` as well as `/dev/input/event*`.
* The `org.freedesktop.portal.RemoteDesktop` portal, since 0.10.0
  (`linux/portal/`), as the no-`sudo` fallback: tried **only** when
  `uinput` cannot be opened, so nobody who ran the setup script ever
  meets a consent dialog. Written against the specification on a
  machine with no RemoteDesktop backend — compiled and unit-tested,
  **never executed**. `libei` was skipped deliberately: the portal's
  `NotifyKeyboardKeycode` does the same job without a second protocol
  implementation.

### Holding keystrokes back during a correction (input remappers)

A correction is a burst of injected keys, and anything the user types
while it is on the wire lands *inside* it. PolterType therefore holds
the keyboard for the length of a burst (`EVIOCGRAB`) and types the held
keystrokes out itself, in order, once the correction is down. No extra
permission is needed — it uses the `/dev/input/event*` access
`setup-linux.sh` already grants.

> **On Windows this exists, works, and is still switched off.** The
> low-level keyboard hook swallows events for the length of a
> correction and types them out behind it, with the same contract as
> above, and it needs no permission at all there.
>
> 0.11.0 is the first release in which it has run on real hardware, and
> that run changed both what it does and why it is off. Two bugs were
> fixed that lost keystrokes outright rather than scrambling them — the
> replay had no path back on Windows at all, and once it had one it
> still dropped the **spacebar**, which is the boundary that triggers
> most corrections.
>
> It stays opt-in (`POLTERTYPE_HOLD_KEYS=1`) for a different reason
> than before: not fear, but **latency**. Holding means your keys are
> withheld from the application for something like 75–100 ms after
> every correction and then arrive together, which reads as the caret
> lagging behind your typing. That trades a rare mangled word for a
> constant delay, and it is not a trade to make on everybody's behalf.
> Switch it on if you type fast enough to hit the race — and see
> [issue #7](https://github.com/Just-Code-NET/PolterType/issues/7) for
> the measurements. No keyboard wedge was observed at any point.
>
> **On macOS the gate exists and is likewise opt-in**
> (`POLTERTYPE_HOLD_KEYS=1`) — same mechanism, same latency trade:
> held keys arrive together after the burst instead of interleaving
> with it. The event tap moves from listen-only to active only when
> the gate is on. Validated on Intel hardware; the replay path (the
> `send_text` fallback that both bugs above were fixed in) is what
> makes it safe to enable there, exactly as predicted above.

**It stands down behind an input remapper.** keyd (and anything with
the same design) holds every keyboard exclusively — *including
PolterType's own virtual one* — and re-emits through a single virtual
device. Grabbing that device would block PolterType's own corrections
along with the user's typing, so PolterType checks whether it can grab
its own emitter — at startup, and again before **every** hold. The
re-check matters: a remapper grabs a freshly created device
*asynchronously*, so the startup probe alone can win the race, arm the
gate, and let the first correction funnel the whole session's input
into PolterType. If the emitter turns out proxied at hold time, the
gate switches itself off for the rest of the run. The log line says so
at `INFO` (startup) or `WARN` (hold time):

```
key gate off: an input remapper holds our emitter …
key gate off: an input remapper grabbed our emitter after startup …
```

Corrections still work — they just fall back to detecting and repairing
a keystroke that got in, rather than preventing it.

To get the stronger behaviour back under keyd, exclude PolterType's
device in `/etc/keyd/default.conf` so it is not proxied:

```ini
[ids]
*
-1234:5678   # poltertype virtual keyboard — leave it unproxied
```

Restart `keyd` and PolterType; the startup line should become
`key gate ready`. Verify the id against your own machine first — it is
whatever `poltertype virtual keyboard` reports:

```bash
sudo libinput list-devices | grep -A2 'poltertype virtual keyboard'
```

`POLTERTYPE_HOLD_KEYS=0` in the environment turns the whole mechanism
off regardless.

### Switching layout

Switching uses whichever backend is alive in the session. No `sudo`
required — every backend talks over the user's session bus or via
the canonical CLI tool of its ecosystem. Backends, in priority order:

1. **Hyprland** (`hyprctl switchxkblayout`) — when
   `HYPRLAND_INSTANCE_SIGNATURE` is set.
2. **KDE Plasma** (`qdbus6`/`qdbus` → `org.kde.keyboard /Layouts`).
3. **sway** (`swaymsg input type:keyboard xkb_switch_layout`) — sway
   keeps its keyboard configuration to itself, so nothing else in this
   list can reach it.
4. **Cinnamon** — two routes, and the probe picks by asking rather
   than by version:
   - **6.6 and newer**: `gdbus` → `org.Cinnamon.GetInputSources` and
     `org.Cinnamon.ActivateInputSourceIndex`, the entry point
     Cinnamon's own keyboard applet uses, so the tray indicator
     follows.
   - **6.4 and older** (Linux Mint 22.x): no such API. There layouts
     are ordinary XKB groups — the applet drives
     `XAppKbdLayoutController` → libgnomekbd → `XkbLockGroup`, and it
     listens for group changes — so this routes to backend 8 on
     purpose, logged as `linux-cinnamon-xkb` rather than
     `linux-x11-xkb` so a bug report can tell a deliberate choice
     from a fallback.
5. **GSettings** (`gsettings org.gnome.desktop.input-sources`) —
   **GNOME**, and the desktops that genuinely read that schema. The
   probe requires the schema to be installed *and* to list at least
   one input source: the schema ships with GTK, so it is present on
   plenty of machines running no GNOME-family desktop at all, where it
   reads back empty. Populating it is not enough either — Cinnamon
   populates it and reads a different one
   ([#26](https://github.com/Just-Code-NET/PolterType/issues/26)),
   MATE keeps its layouts in `org.mate.peripherals-keyboard-xkb`, and
   a wlroots compositor (labwc, and with it Budgie's and Xfce's
   Wayland sessions) reads no schema at all, so the backend stands
   down for each rather than writing a key nobody reads.

   On GNOME the written key is also not the whole story. Since GNOME
   45 the shell keeps the live source in `mru-sources`, which is what
   PolterType reads back, and on 49 no writable key moves the keyboard
   at all — so when the write does not take, the switch is sent as the
   desktop's own `switch-input-source` shortcut.
6. **IBus** (`ibus engine`) — a DE that lets IBus own the layout.
   Note that running an IBus daemon is not that: most desktops run one
   for CJK input while switching layouts by another route entirely,
   so this backend now requires the session to *name* an input method
   (`XMODIFIERS`, `GTK_IM_MODULE`, `QT_IM_MODULE`).
7. **Fcitx5** (`fcitx5-remote -s …`) — any DE hosting Fcitx, under the
   same rule: Ubuntu starts fcitx5 with language support, where it
   owns nothing.
8. **X11 XKB** (`XkbLatchLockState` via `x11rb`) — locks the XKB
   group, which is what the layouts in `setxkbmap -layout us,ua`
   actually are. This is the bare-WM fallback (i3, openbox, plain
   `.xinitrc`), where no desktop environment owns the layout. Probed
   last on purpose: where a DE *is* present it usually keeps a tray
   indicator in sync with the layout, and locking the group underneath
   it would switch the keyboard while leaving that indicator lying.
   (Cinnamon 6.4 is the exception, which is why backend 4 routes here
   knowingly.) Stands down entirely under XWayland, where the
   compositor owns layout, and on MATE, whose group state tracks
   neither our write nor the session's own switch.

**Pinning a backend.** `POLTERTYPE_LAYOUT_BACKEND` skips the probe and
uses exactly the backend you name — `hyprland`, `kde`, `sway`,
`cinnamon`, `gnome`, `ibus`, `fcitx`, `x11`, or `auto` for the default
probe:

```bash
POLTERTYPE_LAYOUT_BACKEND=ibus poltertype
```

It exists because the probe is a set of guesses about somebody else's
input stack, and guesses are wrong sometimes. If the name is not one
of the above, or that backend cannot start on this machine,
PolterType exits with a message saying so — it does not quietly fall
back to probing, because "we chose something else and didn't mention
it" is the failure the variable is there to diagnose. `gnome`
additionally skips the "this desktop ignores the schema" check, for
the session where our list of desktops is wrong and gsettings really
does work. The chosen backend is logged at startup either way
(`layout switcher ready backend=…`), which is the first line to read
in any report about switching.

If none respond, PolterType **does not start**: it logs `no layout
switcher backend; aborting` and exits. There is no degraded mode where
the app sits in the tray unable to switch anything — a layout switcher
is a hard requirement, not a nice-to-have.

(The separate case — keyboard *hooks* failing while layout switching
works — does keep the app running, and surfaces the ⚠ Setup tray entry
described under macOS above, which opens the Setup pane.)

## Network

PolterType asks for no network permission from the OS, but it does use
the network, and a document that enumerates the app's capabilities
should say so rather than leave you to find out from the firewall.

**One outbound connection exists, and it is on by default:** the
updater checks `github.com` for a new release once a day, and
downloads an installer when there is one. It sends no body, no query
string and no identifier — GitHub sees your IP and a User-Agent naming
the running version, exactly as it would for any download. Nothing
about what you type ever leaves the machine; there is no telemetry of
any kind, and this connection must never become a place to add any.

Turn it off with the checkbox on the Settings window's **General**
pane, or `[updates].enabled = false` in `config.toml`. The manifest URL
is printed on that pane so you can verify the destination yourself.
See [DECISIONS.md](DECISIONS.md) for the trust model and its limits.

The AI subsystem ([docs/AI.md](AI.md)) is the *second* network
capability, and it is opt-in at every layer. Since v0.12.0 the
official installers ship it compiled in — but it makes no request of
any kind until you write an `[[ai.plugins]]` entry naming an endpoint:
PolterType ships no model, no vendor SDK and no default endpoint.
`[ai].enabled` defaults to off; an endpoint that is not loopback
additionally requires `[ai].allow_remote = true`, because a model on
your own machine (`127.0.0.1`) never sends a typed word off it; API
keys live in the OS keychain only. With nothing configured — the
default — the subsystem builds no detectors and opens no socket.

**macOS note:** the updater strips `com.apple.quarantine` from the
bundle it installs. That is defensible only while the app is unsigned —
it must come out the day we ship notarised builds.
