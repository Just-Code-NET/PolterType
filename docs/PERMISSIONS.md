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

### Option B — AT-SPI (listener: planned, not implemented)

**The keyboard listener via AT-SPI does not exist.** On Wayland,
option A is currently the only listening path. The idea: if the
user's accessibility bus is enabled, subscribe to keyboard events —
no `sudo`, but higher latency, and some inputs (especially in
non-toolkit apps) are missed. It would serve as a fallback when
option A is not available.

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

* `uinput`, via the same device permissions `setup-linux.sh` grants.
  **This is the only implemented path** — which is why the setup
  script covers `/dev/uinput` as well as `/dev/input/event*`.
* `libei` through the `org.freedesktop.portal.RemoteDesktop` /
  `InputCapture` portal (KDE Plasma 6.0+, GNOME 46+) is the planned
  no-`sudo` alternative. **Not implemented** — there is no portal code
  in the tree today.

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
