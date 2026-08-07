# Decision log

Short-form record of non-obvious technical choices made while
implementing PolterType. Each entry: **what** was decided, **why**,
and any **alternatives** considered.

---

## 2026-08-05 — A plug-in service that dies must be noticed by the app, not by the user weeks later

`Supervisor::reap` was written to be "called from the tray's heartbeat".
It never was: the only call site was the menu-click handler, so a
service that died was reaped the next time the user happened to open the
tray menu — and until then it was also a zombie process, because nobody
else waits on these children.

Measured here, on the machine this is written on. The capture daemon of
the `poltertype-autopilot` plug-in exited one second after startup, at
11:30, on an error it printed to an inherited stderr nobody was reading.
It was found at 22:11 — **ten and a half hours** during which the user
believed a plug-in was capturing and it was not. The tray had been
reporting `Mode: learn` the whole time, correctly and uselessly: a
plug-in's state comes from a one-shot command that answers exactly the
same whether the service behind it is alive or dead.

**What changed.**

* `reap` now runs on the plug-in heartbeat — the 15-second tick that
  already existed for refreshing state — and *before* the refresh, so
  the menu is never redrawn from a state nothing is enforcing. The
  heartbeat is armed if any service is being supervised, not only if a
  plug-in reports state.
* A departure produces a **notification**, on the error path that is
  not gated by `[general].show_notifications`. A `warn!` line in a file
  the user does not know about is not a user interface — the same
  reasoning as the startup input alert.
* A service's own stdout and stderr now go to
  `logs/plugin-<id>.log`, truncated at every start, instead of being
  inherited. A tray app launched from a desktop entry has no terminal,
  so "inherited" means the one line explaining the failure goes
  nowhere. The last non-blank line of that file is what the
  notification quotes.

**What deliberately did not change.** No restart. A service that dies
stays dead until the user acts: restarting a plug-in that crashes on
startup produces a fork bomb that also fills the log, and hides the
failure instead of showing it. The whole point of this entry is that
the failure was hidden.

**Alternative considered.** Capturing the pipe in-process and keeping
the last N lines in memory. Rejected: a reader thread per service, a
plug-in blocked on a full pipe if that thread ever failed to start, and
nothing left on disk for the user to read afterwards. A file has none
of those failure modes and is the artefact you actually want at 22:11.

---

## 2026-08-04 — Windows plug-in shutdown: console control events measured, refused, replaced by a declared command

`request_stop` was an honest no-op on Windows, so a plug-in service was
always killed 400 ms after PolterType decided to quit, losing whatever
it held. The obvious remedy is the console control event — Windows has
no signal, and `CTRL_BREAK_EVENT` is what stands in for one. We
implemented it fully and measured it on Windows 11 rather than
reasoning about it.

**What was measured.** Children spawned with
`CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW`, the console borrowed
with `AttachConsole`, a real `SetConsoleCtrlHandler` routine installed
*first* so the event could not come back at us, then
`GenerateConsoleCtrlEvent`. Two addressings, two failures:

* **To the child's own process group** (`dwProcessGroupId = pid`, the
  documented shape for a group root): returned `Ok(())` and the child
  went on running. Nothing observable happened at all.
* **To the whole borrowed console** (`0`): the child died — and so did
  the sender.

**Why the guard did not hold.** `SetConsoleCtrlHandler(NULL, TRUE)`,
the documented way to ignore console events, covers **Ctrl+C only**;
this is Ctrl+Break. A real handler returning `TRUE` was installed
instead and the process still went down, which is where we stopped:
the sender here is the process that owns the global keyboard hook, and
losing it mid-shutdown is worse than any buffer a plug-in might flush.

**What we did instead.** The request goes through the interface
plug-ins already have. A plug-in may declare a command with the
reserved id `stop`; the supervisor runs it before the grace period. It
is the plug-in's own program, invoked the way its every other action
is, and what "stop cleanly" means belongs to its author. One path on
three platforms, no `unsafe`, and `poltertype-shell` keeps
`forbid(unsafe_code)` — which the console route would have cost.

**Alternatives considered.** A named event or socket the plug-in waits
on: a new contract to specify, document and get authors to implement,
for no more than the declared command already gives. Keeping the no-op:
rejected, since Unix plug-ins got a real request and Windows ones got
silence. SIGTERM still goes out on Unix afterwards, and the kill still
ends anything that ignores both — nothing here is a guarantee, and the
caller must always have the kill.

---

## 2026-08-01 — No AT-SPI keystroke listener: measured, refused, and the reason is architectural

`PLAN.md` has carried "Wayland AT-SPI fallback listener via `atspi`"
as an open item since Phase 6, on the theory that the accessibility
stack could read keystrokes without `input`-group membership and so
retire `scripts/setup-linux.sh` for Wayland users. We finally measured
it instead of assuming it. It does not work, and the reason is not
something we can fix.

**What was measured.** A probe registered a keystroke listener with
`org.a11y.atspi.DeviceEventController` exactly as an assistive
technology does — empty key set (all keys), press+release, global mode
— on this Hyprland session, whose a11y bus is live and whose registry
publishes the full `DeviceEventController` interface including
`RegisterKeystrokeListener`. Two results:

* `RegisterKeystrokeListener` returned **false**. The registry
  declined outright.
* With five real keystrokes injected through `uinput` inside the
  listening window, **zero** `NotifyEvent` callbacks arrived.

**Why it cannot be fixed here.** `at-spi2-registryd` has no privileged
path to the keyboard of its own. On X11 it snoops via the X server; on
Wayland it can only relay what the *compositor* hands it, and only
mutter does that. A wlroots compositor never feeds the registry, so
the interface exists, answers introspection, and refuses to register —
which is exactly the failure shape that made this look plausible for
so long.

**And where it would work, it is redundant.** The matrix:

| Session | Existing listener | What AT-SPI would add |
|---|---|---|
| X11 | XInput2, **needs no permissions at all** | nothing |
| GNOME Wayland | evdev (`input` group) | possibly a permission-free path — untested, no GNOME box here |
| KDE / wlroots Wayland | evdev (`input` group) | nothing; registration refused |

So the feature is redundant on the one session type where it reliably
works, unverifiable on the one where it might help, and dead on the
rest. Writing it would mean several hundred lines of code that this
project could never honestly describe as working — the same standard
that keeps the macOS caveats in `CLAUDE.md` explicit.

**Alternatives considered.** `libei` through the
`org.freedesktop.portal.RemoteDesktop` portal is the *emitting* half
of the same wish and remains genuinely promising — mutter and KWin
both implement that portal. It is not implemented either, for a
narrower reason: no RemoteDesktop backend exists on this machine
(`hyprland.portal` declares Screenshot, ScreenCast, GlobalShortcuts
and InputCapture; only `kde.portal` declares RemoteDesktop), so it
cannot be exercised here at all. That one stays open rather than
decided — see the entry in `PLAN.md`.

**Consequence for users.** Wayland still needs `setup-linux.sh` once.
That is the honest state, and the Setup pane already says so. Anyone
who wants a zero-permission session today has one: X11, where the
listener and emitter both need nothing.

---

## 2026-07-31 — The setup walkthrough probes, and refuses to act on the user's behalf

Replacing "here is a link to PERMISSIONS.md" with a screen that knows
what this machine is missing. Four choices worth defending.

**The probe lives in `poltertype-input`, the rendering in the app.**
"Is the user in the `input` group" is platform code, and platform code
lives in the seven crates that are allowed to hold `#[cfg(target_os)]`.
The Settings window is handed a `SetupReport` — a list of steps with a
state and at most one action — and knows nothing about udev rules. It
also means the per-OS logic has tests without a GUI in the loop.

**Nothing on the pane runs anything privileged.** The Linux fix needs
`sudo`, and the obvious "Run setup" button would mean an app quietly
acquiring root on a machine where it already reads every keystroke.
That is trust we would not get back. The button copies the command
instead; the user reads it and runs it in their own terminal.
Similarly, macOS permission prompts are always the *system's* dialog
(`AXIsProcessTrustedWithOptions`, `IOHIDRequestAccess`) — we never
draw something that looks like one.

**Four states, not two.** `Done` / `Todo` would have been enough to
render a tick and a cross, and would have given the wrong advice in
the case that actually costs people an evening: `usermod -aG input`
writes the group database and cannot touch the credentials of a
session that already exists. Everything looks configured, nothing
works, and "re-run the setup script" — the only advice a two-state
model can give — changes nothing. `NeedsRelogin` exists to say *log
out* instead. The fourth, `Unknown`, is for what we genuinely cannot
determine (no `/dev/input` entries at all, an `IOHIDCheckAccess` that
returns "undecided"): a setup guide that invents a problem loses the
reader, and one that asserts a fix it has not verified is worse.

**Read and write are separate rows.** They are separate permissions
and they fail separately, and the half-granted case — evdev readable,
uinput not — is the confusing one: detection works, corrections never
land, and the app looks like it is deciding wrongly rather than being
unable to act.

*Not built:* the screenshots and GIFs of the macOS toggles that issue
#10 asked for. They would be the most useful part of that pane for a
first-time Mac user and they cannot be produced from a machine without
a Mac. The deep links into the exact System Settings panes are the
half we could do honestly.

*Untested on hardware.* Verified on Wayland/evdev here, including the
unresolved states. The macOS half compiles in CI and has never run.

---

## 2026-07-31 — The tooltip already worked on KDE; only the documentation said otherwise

Issue #6 asked for a plan to bring the suggestion tooltip to GNOME and
KDE Wayland, where it was documented as a no-op. Before designing
anything, we ran the actual backend against an actual KWin.

**KDE was never broken.** KWin has implemented `zwlr_layer_shell_v1`
for years and hands it to third-party clients. Against a nested
`kwin_wayland` 6.7.3 the popup crate selects
`linux-wayland-layer-shell` and logs `popup surface mapped` — the
surface is created, configured by KWin and mapped, exactly as on
Hyprland. No code was needed. The claim that it didn't work appeared
in five places (`poltertype-popup`'s crate docs, its factory, the
Wayland backend's module docs, the `NoLayerShell` error *message*, and
`CLAUDE.md`'s known-gaps list) and had been wrong since it was
written.

The lesson is not "KDE works". It is that **the backend is a probe and
the documentation was a lookup table.** `create_popup` tries
layer-shell, then X11, then noop, and never asks what desktop it is
on; the prose asserted a mapping from desktop names to outcomes that
nothing in the code computed. A probe stays right when the world
changes underneath it. A hand-maintained list of desktop names is
wrong the moment one of them ships a protocol, and nothing fails to
tell you.

**GNOME is not a no-op either.** Mutter genuinely has no layer-shell
(GNOME/mutter#973, open since 2019 and not implemented). But the
factory's second probe is `DISPLAY`, which on a GNOME Wayland session
points at XWayland — and an override-redirect X11 window maps and
displays there. Verified in the shape that matters: forcing the
Wayland probe to fail in a live Wayland session selects
`linux-x11-override-redirect` and logs `popup window mapped`. So the
honest gap is not "GNOME and KDE"; it is "a Wayland session with
neither layer-shell nor XWayland", which is rare and getting rarer.

**What actually was missing on both, and is now fixed: the anchor.**
`create_linux_focus_tracker` returned `NoopFocusTracker` on anything
that is not Hyprland or X11, so the tooltip had no window rect *and*
no caret and fell back to screen-bottom-centre. The window rect is
genuinely unavailable there. The **caret is not**: the AT-SPI watcher
talks to the session bus and does not care which compositor is
running. It was simply never constructed, because it was built inside
the two branches that also had a window query. A `CaretOnlyFocusTracker`
now takes that path — `focused_exe()` stays `None`, so nothing keyed
off the focused app starts guessing, while `caret_hint()` gives
GNOME and KDE the *best* anchor in the chain rather than the worst.

*Not attempted:* a GNOME Shell extension talking to the app over
D-Bus. It would buy the window rect (a worse anchor than the caret we
now have) at the cost of a second distribution channel, a review
process, and a component that breaks on every GNOME release. Should
Mutter ever implement layer-shell, the existing probe picks it up with
no code change.

---

## 2026-07-31 — No Flatpak: `uinput` is not grantable, and the holes that would fake it are the ones Flathub rejects

Asked and answered before anyone spends a weekend on it. The question
was not "can we build a Flatpak" — anything can be built — but "can
PolterType work inside the sandbox without punching holes so wide the
listing gets rejected, or so wide that the sandbox stops meaning
anything." **No, on both counts.** AppImage plus native packages is
the honest Linux story.

**What we need from the host.**

| Need | Where | Sandbox status |
|---|---|---|
| Read `/dev/input/event*` | evdev listener | `--device=input` (flatpak ≥ 1.15.6) |
| Write `/dev/uinput` | the emitter — *every correction* | **not grantable except via `--device=all`** |
| Grab the keyboard (`EVIOCGRAB`) | the key gate | same, and only via `--device=all` |
| `hyprctl` / Hyprland IPC socket | layout switching on wlroots | host binary + `$XDG_RUNTIME_DIR/hypr` socket |
| `gsettings` | GNOME layout switching | host binary + dconf |
| `gdbus` | Cinnamon layout switching | host binary + session bus name |
| `qdbus` / `qdbus6` | KDE layout switching | host binary + session bus name |
| `ibus`, `fcitx5-remote` | IME layout switching | host binaries |
| AT-SPI bus | caret position for the tooltip | `--talk-name=org.a11y.Bus` |
| `~/.config/autostart/*.desktop` | run at login | portal exists — the one clean case |

**The blocker is `uinput`, and it is a hard one.** `device=input` was
introduced for game controllers and *deliberately does not include
`/dev/uinput`*: the original patch carried `/dev/uinput` and
`/dev/hidraw*` and that part was removed before it landed
([Flathub discourse][fh-input]). So the only permission that gets us a
virtual keyboard is `--device=all`, which is the whole device tree —
webcam, disks, everything. Flathub's own requirement is that "static
permissions must be kept to an absolute minimum" and that a portal, if
one exists, is mandatory rather than optional
([Flathub requirements][fh-req]); there is no portal for creating a
virtual input device. Without `uinput` PolterType cannot type, which
is the entire product.

**The precedent points the same way.** input-remapper's Flathub
request was closed as technically infeasible, on exactly this
reasoning — the sandbox is above the layer this class of app has to
operate at ([Flathub discourse][fh-remap]). We are in that class.

**Even granting everything, the DE integration would still be
broken.** Layout switching shells out to `hyprctl`, `gsettings`,
`gdbus`, `qdbus`, `ibus` and `fcitx5-remote` — host binaries that do not exist
inside the runtime. `flatpak-spawn --host` would work and is exactly
"disabling or bypassing security mechanisms" in Flathub's words. A
Flatpak that needed `--device=all` *and* host command execution is not
a sandboxed app; it is an AppImage with extra steps and a misleading
padlock.

**What a Flatpak would genuinely improve** is autostart:
`org.freedesktop.portal.Background` is a cleaner mechanism than the
XDG `.desktop` file `poltertype-autostart` writes today. One clean win
against a load-bearing blocker is not a trade.

**Verified, not assumed:** the self-updater already stands down inside
a Flatpak, and by the right mechanism. `apply/linux.rs` requires
`$APPIMAGE` to be set and non-empty — an allowlist, not a denylist of
`$FLATPAK_ID` and friends — so any install that is not our own
AppImage refuses and the user is pointed at the Releases page. Nothing
to change there whatever we decide. (It still *downloads* before
discovering it cannot install, which is a small waste for every
packaged user and worth fixing on its own merits, independently of
this decision.)

**Revisit if** a portal for virtual input devices appears (a
`org.freedesktop.portal.InputCapture`-shaped API that covers *emitting*
rather than only capturing), or if `device=input` grows `/dev/uinput`
back. Both would have to land *and* be widely deployed before the
listing would be worth the maintenance.

[fh-req]: https://docs.flathub.org/docs/for-app-authors/requirements
[fh-input]: https://discourse.flathub.org/t/support-for-device-input/6645
[fh-remap]: https://discourse.flathub.org/t/input-remapper-flatpak-request/3814

---

## 2026-07-31 — The release manifest is signed by a human, not by CI

The updater verified each download's SHA-256 against `latest.json` and
nothing verified `latest.json`. Since the checksum ships in the same
GitHub release as the artifact, anyone who could publish a release
could publish both — the checksum bought integrity against a broken
transfer or a tampered CDN, and nothing against the attacker who
actually matters.

**`Manifest.signature` is now a real detached ed25519 signature**,
verified the moment the manifest parses and before any URL in it is
read. Three choices inside that are worth writing down.

**The key never enters CI.** The obvious implementation is an Actions
secret and a signing step in `release.yml`. It would also have been
security theatre: the threat model is "someone can publish a GitHub
release", and someone who can do that can read the repository's
secrets. Signing therefore happens on the maintainer's machine,
between the draft CI produces and the moment a human publishes it
(`cargo xtask manifest sign`, `docs/RELEASING.md` §7). The cost is a
manual step that can be forgotten; the mitigation is that the workflow
summary and the release checklist both spell it out, and that a
forgotten signature degrades to today's behaviour rather than breaking
anything.

**We sign a rendering, not the JSON.** Signing raw JSON makes the
check hostage to formatting — re-serialise with different whitespace
or key order and a valid signature stops verifying — and "canonical
JSON" is a second specification to get wrong. So the signature covers
a flat, newline-delimited rendering of the meaningful fields
(`crates/poltertype-update/src/signature.rs`), artifacts ordered by
key so a `HashMap`'s iteration order cannot leak in. Since `\n` is the
only separator, a value containing one could describe two different
manifests with the same bytes — both the signer and the verifier
refuse such a manifest outright, which is the format's entire
ambiguity surface. One function renders it and both ends call it:
`xtask` depends on `poltertype-update` precisely so there is no second
implementation to drift.

*Alternative considered:* publish `latest.json.sig` as a second asset
and sign the file bytes verbatim. Simpler to reason about, no
canonicalisation at all — but a second network request on every update
check, and a manifest whose signature can be dropped without the
manifest looking any different.

**Verification lands one release before enforcement.**
`REQUIRE_SIGNATURE` is `false` in the release that introduces all of
this. A present signature must verify; an absent one warns. Shipping
it as `true` would strand every user whose updater resolves to the
last unsigned manifest — including the one published before anyone
had the tooling. It gets flipped once a signed manifest has been the
published `latest.json` for a full cycle, and that flip, not this
release, is when the README gets to say "signed".

---

## 2026-07-31 — macOS subscribes to `FlagsChanged`, and clears flags on everything it posts

Two macOS gaps that turned out to be one story.

**The dead arms (issue #4).** The event tap subscribed to `KeyDown` and
`KeyUp` only. macOS never sends those for a modifier — a Shift press is
a `kCGEventFlagsChanged` event (type 12) — so the Apple-modifier arms
of the keycode table could never be reached, and modifier *state* came
only from folding the flags carried by ordinary key events. The choice
was to subscribe (Option A) or delete the arms and document
flags-folding as the single source of truth (Option B).

**Chose Option A.** Folding is accurate the instant a character key
arrives and stale at every other moment: let go of Ctrl and nothing
tells us until the next keystroke. `held_modifiers` is read at the
*start* of a correction — often triggered by a chord, i.e. precisely
when no ordinary key is flowing — so the stale window is the window
that matters. Subscribing also puts macOS on the same footing as the
Windows and X11 backends, which do get discrete modifier edges, and
costs one extra event per modifier edge through a callback that only
translates and `try_send`s.

The subscription makes the keycode table's modifier arms live, which is
a behaviour change in the engine's word buffer: Apple 0x3C (RShift)
would otherwise land on SC-1 0x3C, inside the classifier's
`0x3B..=0x53` "navigation — end the word and discard it" range. The
arms were already written for exactly this, and now there are tests.
`kVK_Function` (0x3F) is the one that was *not* covered: it rides the
same `FlagsChanged` stream, has no SC Set-1 equivalent, and the
identity fallback would put it at SC-1 0x3F — inside that same range,
so holding Fn to reach an arrow key would have silently eaten the word
in progress. Untracked modifier keycodes are dropped in the listener
instead.

**The missing release (issue #5).** `KeyEmitter::release_modifiers`
had a default no-op that macOS inherited, so accepting a suggestion
with `Ctrl+Shift+<digit>` while still holding the chord retyped the
word under those modifiers.

The macOS fix is in two independent halves, both of which we want:

1. **`release_modifiers` posts `FlagsChanged` events.** There is no
   key-up for a modifier on macOS; the release *is* a flags-changed
   event whose flags describe what remains down. We post one per held
   modifier, each carrying the picture after that key is up. Caps Lock
   is excluded on purpose — it is a latch, and clearing it would turn
   the user's Caps light off behind their back.
2. **Every event we post has its flags cleared.** An event built from a
   `HIDSystemState` source inherits the *live hardware* modifier flags,
   so with the chord still held our backspaces would post as ⌘⌫ —
   "delete to start of line" — and wreck far more than the word. This
   half needs nothing to have worked at the OS level and covers the
   case where the engine did not think anything was held.

**Windows had the same no-op** and the same bug; it now sends key-ups
for both sides of each held modifier via `SendInput`. Issue #5 assumed
this already worked there.

*Untested on hardware.* The tables and the direction rules moved into
`macos/codes.rs`, which carries no Apple dependency and therefore
compiles — and runs its tests — on Linux and Windows CI. The FFI
either side of it is compiled by CI's `macos-latest` job and executed
by nobody yet; the tap change in particular wants a real Mac before
0.7.0 ships.

---

## 2026-07-30 — Every macOS TIS call goes through the main dispatch queue

`TISCopyCurrentKeyboardInputSource` and friends look like ordinary C
functions and are documented as thread-safe nowhere in particular.
Since macOS 14/15 the HIToolbox Text-Services layer asserts the main
dispatch queue inside them
(`TSMGetInputSourceProperty` → `isValidateInputSourceRef` →
`islGetInputSourceListWithAdditions`). Our layout poller runs every
250 ms on its own thread and the engine switches from a third; both
killed the process outright with `SIGILL` / `EXC_BAD_INSTRUCTION`,
seconds after launch.

**All TIS calls now go through the main queue** — inline when the
caller is already the main thread, `dispatch_sync_f` on
`_dispatch_main_q` otherwise. We link the queue object directly
because `dispatch_get_main_queue` is a header alias, not an exported
symbol.

*Alternative considered:* funnel layout access through a dedicated
actor thread that happens to be main. Same effect, more machinery, and
it would still have to be the main thread — the constraint is
HIToolbox's, not ours.

*Cost:* the layout poller now wakes the main thread four times a
second. `TISCopyCurrentKeyboardInputSource` is cheap and the main
thread is otherwise idle in its run loop, so this has not been
measurable — but it is the reason to keep the poll interval honest.

Found by an outside contributor running v0.5.0 on real hardware, which
is also the whole argument for not shipping a backend that only CI has
ever executed.

---

## 2026-07-30 — Autostart drives each OS mechanism directly, and never `bootout`s

`auto-launch` had been in the manifest since the first commit and was
wired to nothing: `[general].autostart` defaulted to `true`, the
Settings checkbox wrote it, and no code ever read it. The app had
never once started at login, on any platform, while README, the
settings schema and the landing page all said it did.

**Replaced with a `poltertype-autostart` crate** that writes each
platform's own artefact — a LaunchAgent plist, an `HKCU` run-key
value, an XDG `.desktop` entry. That is one crate with three small
backends instead of a dependency, and it costs no per-OS crates at all
(`launchctl`, `reg.exe` and a file write are the whole surface), so
the crate is `#![forbid(unsafe_code)]` with no target-specific
dependencies.

**The part worth defending: no `bootout`, ever.** On macOS the obvious
way to make a changed LaunchAgent take effect is `launchctl bootout`
followed by `bootstrap`. But `bootout` terminates the job's running
processes — and when launchd started us at login, *we are that job*.
The first implementation did this on both paths:

* on every startup, so a login-started instance killed itself and the
  replacement then lost the single-instance lock race against its own
  dying predecessor — leaving nothing running at all;
* on disable, so unticking the checkbox terminated the app.

Neither reproduces when the app is launched from Finder, because then
the process is not a launchd job and there is nothing to kill. It
would have shipped.

So the enable path `bootstrap`s only when `launchctl print` says the
label is unknown, and the disable path just deletes the plist —
launchd reads it at login, and a label already loaded has `RunAtLoad`
behind it and no `KeepAlive`, so it will never start anything again.

*Accepted cost:* if the plist contents drift while the label is
already loaded (an update moved the executable), launchd keeps the old
spec until the next login. The file on disk is corrected immediately,
so the next login is right. Trading one stale session against killing
the user's running app is not a close call.

---

## 2026-07-29 — A `poltertype-tray` crate, for one function

Building the tray makes `libayatana-appindicator` print a deprecation
notice to stderr on every start — addressed to whoever links it, which
is `tray-icon`, not us. There is no lever on our side: the sys crate
`dlopen`s `libayatana-appindicator3.so.1` by name, its `backcompat`
feature only adds unversioned-`.so` fallbacks, and `tray-icon` 0.24 —
five versions ahead of the one we pin — still loads the same object.

**Redirected, not silenced.** A GLib log handler on that one domain
hands the text to `tracing` at debug level. The message stays reachable
the day the library actually disappears (which would break the tray),
without sitting in the journal of every Linux user. Every other GLib
domain keeps GLib's default handler.

**Why a whole crate for fifteen lines.** `poltertype-app` contains no
`#[cfg(target_os = "...")]` at all — platform code lives in dedicated
crates, and that rule is worth more than the crate it costs. The
alternatives were to put the first `#[cfg]` in the binary and amend the
rule, or to hide a GTK concern inside an unrelated platform crate. So
`poltertype-tray` exists, holding per-OS tray quirks; the `TrayIcon`
itself is still built in the app, because `tray-icon` already
abstracts it. The list in `CLAUDE.md` is now five crates, not four.

**`cargo deny` gained an entry on the way.** RUSTSEC-2024-0429 (glib
0.18's unsound `VariantStrIter`) fails the advisory check — it already
did before this change, since `tray-icon` and `tao` have pulled glib
into the lockfile all along; the advisory is simply newer than the last
release. The fix is glib >=0.20, which needs gtk-rs 0.20, which needs a
`tray-icon` that has left GTK3 — the same wait as the nine GTK3 entries
above it. Nothing here calls the affected API.

---

## 2026-07-29 — The tooltip anchors to the caret or the window, never the mouse

The suggestion tooltip's anchor chain had the pointer position sitting
between the AT-SPI caret and the focused window, justified as "after a
click into the text the pointer hovers near the caret". That premise
holds for about a second. Nothing in the chain could tell a pointer
resting where the user last clicked from one parked in the middle of
the screen while they typed, and the second case is the common one —
you click into a chat box, take your hand back to the keyboard, and the
mouse stays wherever it was. Reported against a chat input at the very
bottom of a display, with the tooltip appearing in the centre of it;
reproduced with the caret 600 px below the pointer, the tooltip landing
on the pointer every time.

**Removed rather than repaired.** The honest fix would need to know
when the pointer was last moved *and* that it was moved into text,
which neither Hyprland's `cursorpos` nor X11's `QueryPointer` can say
and which no amount of click-tracking recovers once the pointer drifts.
Without a caret the tooltip now falls to the window rect — bottom-centre,
`BOTTOM_OFFSET` above the window's bottom edge. That is coarse for a
caret in the middle of a code editor, and right for the chat inputs and
shell prompts that dominate this feature; more to the point it is always
in the focused window, which the pointer anchor could not promise. The
`FocusTracker::pointer_position` method and its three backends went with
it.

**A second bug hid behind the first.** The Wayland popup thread blocks
on its command channel while no popup is up, so it reads nothing from
the compositor between shows. `OutputState`'s replies — the output
names, logical sizes and scales that every placement depends on — had
not arrived when the *first* popup of a session was built: it got
`bounds: None` (no edge clamping at all) and `output: None` on the layer
surface, which hands the compositor the choice of monitor while the
margins were computed against a different one's origin. The second popup
onwards worked, because the tick loop had pumped the queue by then,
which is exactly why it looked intermittent. The thread now round-trips
the queue before serving a `Show` — one round-trip on a thread that has
nothing else to do, and it picks up hotplugs and mode changes that
happened while parked as a bonus.

Verified live on a four-output Hyprland session, including a
`transform: 3` rotated output (logical bounds correctly `1440×2560`) and
a fractional-scale one (`2048×1280` at scale 2).

---

## 2026-07-29 — Hold keystrokes back with `EVIOCGRAB` during a correction

A correction is a burst of injected keys, and the compositor
interleaves whatever the user types into it. Counting after the fact
cannot place a key that landed *inside* our own text (`зтзь ш ` came
out as `ipnpm `), so the fix is to stop the user's keys from reaching
applications until the burst has landed — `EVIOCGRAB`, the evdev
equivalent of a Windows low-level hook swallowing events. We keep
reading the grabbed devices, so the engine still sees every keystroke
and types them out behind the correction, in order
(`poltertype-input::KeyGate`).

Measured on Hyprland + keyd (uinput injection at fixed inter-key gaps,
result read back through the clipboard). Typing a whole command
straight through — `зтзь ш кгт `, 90–190 ms gaps:

| | wrong |
|---|---|
| absorb + repair only | 4 of 6 |
| first working gate | 6 of 6 (worse: characters *missing*) |
| gate, after the three fixes below | **0 of 6** |

The gate only became a win once three things were fixed, none of them
in the gate's own logic — each was found by measuring rather than
reasoning:

1. **The 2 s device rescan blocked the read loop for 70–140 ms.**
   `evdev::enumerate()` opens every node under `/dev/input` and reads
   its capabilities; doing that on the thread that reads key events
   left the engine blind for ~5 % of wall-clock time, events arriving
   late and in bursts. It now reads the directory and opens only
   genuinely new paths — and remembers the verdict per path, since most
   nodes are sound cards that will never be keyboards. (A win with or
   without the gate.)
2. **Releasing a device costs 13–25 ms.** The gate grabbed every open
   device — mice, lid switch, idle HID endpoints — so a correction
   spent ~100 ms in `EVIOCGRAB(0)` inside the very thread that has to
   notice the user typing. It now holds only keyboards, and only ones
   used in the last 30 s: in practice one device.
3. **A grab that outlived its correction was catastrophic.** The next
   correction then counted held keystrokes as though they were on
   screen and deleted text that was never there — a whole word gone,
   far worse than a transposition. `release()` waits for the device
   thread to confirm (250 ms, which costs the user nothing since their
   text is already on screen), and `hold()` refuses to start on top of
   a stale grab.

Safety: the device thread owns the grab and drops it after
[`MAX_HOLD`] (1.2 s) whatever the engine does, so a hung or panicking
correction cannot leave the keyboard dead; a crashed process is safe by
construction, as the kernel releases on close. Held keys are never
silently eaten — Backspace, arrows and Esc are re-emitted after our
text, which is where they would have landed anyway. Shortcuts are the
one gap: they need modifiers the emitter cannot reproduce, so the gate
lets go immediately instead.

**Behind an input remapper the gate cannot run, and knows it.** keyd
holds every physical keyboard *and our own uinput device* exclusively
and re-emits through one virtual keyboard, so the only grabbable source
of the user's keys also carries ours — grabbing it silently gags the
correction itself (verified: injection from a keyd-claimed device under
that grab produces nothing at all). The probe is exact and cheap: if we
can grab our own emitter, nobody is proxying it. Those users keep the
detect-and-repair path, and `docs/PERMISSIONS.md` documents the keyd
one-liner that gets them the gate.

Also worth recording, since it looked like an answer for a while: keyd
claims a uinput device by the *breadth of keys it declares*, not by
name or vendor. Declaring only what the emitter can actually type (51
keys) is still claimed; only an implausibly small keyboard escapes.
Masquerading under keyd's own vendor id works but is a lie about what
the device is, so it is not shipped.

## 2026-05-02 — Use Win SC Set-1 scancodes as the canonical key identity

The engine indexes layout-mapping tables by *scancode*, not by
*virtual-key code*. Reasons:

* Scancode is stable across layouts (the physical key labelled "Q"
  is always `0x10`); VK changes per layout (Q under en-US is `'Q'`,
  under uk-UA it's `'Й'`).
* On Windows we already get the scancode for free in
  `KBDLLHOOKSTRUCT.scanCode`. macOS provides keycode; Linux evdev
  provides EV_KEY codes — both will be normalised into the same Set-1
  space in their respective backend modules.

## 2026-05-02 — Layout mappings embedded via `include_str!`

> **SUPERSEDED** by the 2026-05-07 externalisation entry below.
> Nothing is baked into the binary any more: mappings and wordlists
> are read from `<data_dir>/` at run time, and user overrides live in
> `<config-dir>/poltertype/layouts/` (note: `layouts/`, not
> `layout-mappings/`). They shipped in v0.1, not "Phase 8+".

For v0.1 the EN/UK mapping TOMLs live in `data/layout-mappings/` and
are baked into the binary at compile time. Runtime overrides from
`$XDG_CONFIG_HOME/poltertype/layout-mappings/` are a Phase 8+ task.

Reason: avoids a "where's my data dir?" failure mode on first launch
and keeps the binary self-contained for distribution.

## 2026-05-02 — Use `KEYEVENTF_UNICODE` for text replay (Windows)

When the corrector replays text in the new layout, we use
`SendInput` with `KEYEVENTF_UNICODE` and the codepoint instead of
synthesising scancode + VK presses. Reason: it works regardless of
the currently active layout and bypasses layout-induced corner cases
(dead keys, non-spacing marks). The cost — apps that handle raw
key events specially (e.g. games) will see the synthetic events as
"some text was pasted" rather than "user typed this". Acceptable for
v0.1 since we're explicitly out of scope for games (per-app exception
list handles those).

## 2026-05-02 — Word-buffer classifies by produced character, not raw scancode

Earlier the `WordBuffer` mapped scancodes to "letter / boundary /
backspace / discard" via a hard-coded table that **assumed US-ANSI
positions**. That works for en-US but is silently wrong for any
non-Latin layout: scancode `0x33` is `,` under en-US (a sentence
boundary) but the letter `б` under uk-UA (a word character).

Concrete bug it produced: a Ukrainian user typing `будь ` was
parsed as a wholly empty boundary (the `б` reset the word-in-progress
to nothing), then a 3-letter word `удь` (uk render) ↔ `elm` (en
render). `елm` is a real EN dictionary word, so the engine
"helpfully" auto-switched and replayed `елm `. Same shape applies to
0x34 (en `.` → uk `ю`), 0x29 backtick under any Cyrillic layout, etc.

The fix: `WordBuffer::feed` now takes the character the layout
actually produced (`Option<char>`); the engine queries
`current_layout.translate_key(...)` per-keystroke and threads the
result through. Classification is in two layers:

1. **Control / structural keys** (Esc, Backspace, Tab, Enter,
   Space, modifiers, function row, navigation cluster) — keyed by
   scancode alone, layout-independent. Fast path.
2. **Data keys** — keyed by the produced character class:
   * `is_alphabetic` / digit / `'` `ʼ` `'` / `-` → word
   * everything else → boundary

Cost: one extra Win32 call per keystroke (`GetForegroundWindow` →
`GetWindowThreadProcessId` → `GetKeyboardLayout`). Microseconds.
Worth it for correctness.

Regression locked in by `classifies_by_produced_char_not_scancode`
in `poltertype-core::engine::buffer::tests`.

## 2026-05-02 — Plausibility-keep + runtime-reloadable user overlay

Two related fixes from real-world testing:

### 1. `keep_threshold` on the plausibility detector

User report: `kubectl` (a perfectly normal en-US word for any DevOps
person) was getting auto-switched to `лгиусед` (the Cyrillic render
of the same scancodes). Trace: `kubectl` isn't in the
`dwyl/english-words` FST (general English dict, no tech vocab), so
the dictionary detector returned NoOpinion. Plausibility then scored
`kubectl` at 0.75 (good) vs `лгиусед` at 1.0 (also good — comparable
vowel ratio, no consonant pile-up under uk-UA). Diff 0.25 ≥ the
`min_advantage` threshold → switch.

Fix: `WordPlausibilityDetector` now has a `keep_threshold = 0.7`. If
the current text already scores at this level for its own layout,
the detector emits `Verdict::Keep` instead of looking at alternates.
That's the right semantics — "current is already plausibly its own
language, leave it alone."

This catches the whole class: surnames, brand names, modern tech
vocabulary (kubectl, helm, terraform, docker, nginx), single-token
abbreviations, Cyrillic forms not in the Hunspell stems file, etc.
The Punto cases (real cross-script gibberish like `руддщ` ↔ `hello`)
still switch correctly because gibberish scores well below 0.7.

### 2. Runtime-reloadable user wordlist overlay

The original "Reload Settings" only re-read `config.toml`. The
embedded dictionaries (FST + short-stop) are baked at compile time
and can't be reloaded — but the user-overlay files at
`<config-dir>/poltertype/wordlists/<stem>.txt` SHOULD be reloadable
so users can add tech vocab without restarting.

Implementation: `DictionaryDetector` now holds its dicts behind
`Arc<RwLock<HashMap<…>>>` so they can be swapped atomically. The app
keeps a cheap clone (`detector.handle()`) and on "Reload Settings"
calls `LayoutDb::load_embedded_with_user_overlay(...)` to re-read
the user files, then `handle.replace_dicts(new)` to swap in.

Read locks are taken per-word during decision; write only on reload.
Lock contention is negligible.

## 2026-05-02 — Real Hunspell-grade dictionaries via FST

The hand-curated ~280-word lists shipped earlier worked for the
common case but missed long-tail vocabulary. Switched to:

* **EN**: [`dwyl/english-words`](https://github.com/dwyl/english-words)
  `words_alpha.txt` — Public Domain — ~370k entries.
* **UK**: [LibreOffice/dictionaries](https://github.com/LibreOffice/dictionaries)
  `uk_UA/uk_UA.dic` — MPL 1.1 — ~333k entries (derived from
  brown-uk/dict_uk).

Storage: not `HashSet<String>` (too much heap overhead at this
scale). [BurntSushi `fst` crate](https://docs.rs/fst) compresses the
sorted, deduped wordlist into an immutable byte-buffer set. At
build time `poltertype-core/build.rs` reads `data/wordlists/<id>.txt` and
emits `<OUT_DIR>/<stem>.fst`. At runtime we `include_bytes!` the
blob and wrap in `fst::Set::new(&'static [u8])` — O(len) lookup,
no per-word allocation, lives in `.rodata`.

Concrete cost: release binary 5 → 6.85 MB (+1.85 MB for both FSTs);
~3 MB additional resident memory at runtime. 700k+ words for
~5 bytes per word storage cost — FST is the right tool.

> **PARTLY SUPERSEDED** by the 2026-05-07 externalisation entry.
> The FST choice stands; the plumbing around it changed. Today
> `build.rs` reads `data/wordlists/<stem>.txt.gz` and writes the FST
> into `<workspace>/target/dist/data/wordlists/`, and the runtime
> reads it from disk — there is no `include_bytes!` and nothing in
> `.rodata`, so the binary-size figures above no longer apply.

User overlay path: drop a one-word-per-line text file at
`<config-dir>/poltertype/wordlists/<stem>.txt` to extend a
dictionary with project-specific vocabulary (proper nouns, slang,
domain terms). The overlay is loaded at startup and merged on top
of the embedded FST.

`xtask wordlists fetch` re-downloads upstream sources, runs the
Hunspell-format normalisation (strip `/affixflags`, drop `+cs=`
metadata, lowercase, dedupe, sort), and writes the cleaned txt
files for review and commit.

## 2026-05-02 — Detection in v0.1 = vowel/consonant plausibility

A pure script detector can't separate "real word in this layout" from
"keyboard noise that uses this script" (e.g. `руддщ` is fully Cyrillic
yet gibberish in Ukrainian). For v0.1 we ship `WordPlausibilityDetector`
which scores each candidate text by:

* **vowel ratio** in [0.25, 0.55] — real words land here in both EN
  and UK; pure noise rarely does.
* **max consonant cluster** ≤ 3 — `ддщ` (3 consecutive consonants
  with no separating vowel) is a strong negative signal.
* **script fit** — guards against accidental cross-script chars
  (paste, IME).

The vowel sets are language-specific (Cyrillic UK ≠ Cyrillic RU), so
the detector loads them per `LayoutId` from the layout-mapping TOMLs.

Real n-gram / dictionary / ML detectors land in Phase 7 (the AI
subsystem). Until then we lean on the manual hotkey
`Ctrl+Shift+Backspace` ("fix this word") as the always-works fallback.

> **PARTLY SUPERSEDED.** The `DictionaryDetector` did not wait for
> Phase 7 — it shipped in v0.1 as an FST over Hunspell-expanded
> wordlists, and it is now the highest-priority detector. Only the ML
> ones are still outstanding (and the AI crate remains unwired — see
> `AI.md`). No n-gram model was ever built; `lingua-rs` was dropped.

## 2026-05-02 — Settings format: TOML

Rationale in PLAN.md §3.5 — human-readable and editable, plays nicely
with `serde`. JSON was the initial idea; we picked TOML once we
dropped Tauri (which gave us the `tauri-plugin-store` JSON workflow
for free).

## 2026-05-02 — `injected = true` events are dropped before reaching the engine

The Corrector itself synthesises keystrokes via `SendInput`; those
events come back through the LL hook with `LLKHF_INJECTED` set. The
engine must ignore them to avoid feedback loops where a correction
triggers another correction.

## 2026-05-02 — Dev-friendly behaviour: skip auto-switch in IDEs and on identifiers

The product target audience includes developers, and they specifically
need the corrector to **stay out of code**. Switching layouts mid-
identifier would actively harm the user; the cost of a missed prose
correction inside an IDE is much lower than the cost of corrupting
a function name.

The trade-off in v0.1 is two complementary filters, both
opt-out-able via `config.toml`:

* **Per-app**: `[exceptions].disabled_apps`. The focus tracker
  (`poltertype-input::focus`) reads the foreground process executable
  and the engine matches case-insensitively. Match → skip
  auto-decision. **The list is empty by default** — see "Reversed: no
  default app skip-list" below.
* **Per-token**: even outside the IDE list, the engine checks
  `looks_like_code_token(buffer)` from `poltertype-detect`. If the just-
  finished token contains an underscore, has a mid-token capital
  (camelCase / PascalCase), mixes letters and digits, or carries
  code punctuation (`\\`, `;`, `` ` ``) — skip. This catches
  identifiers in chat / browser / wiki / wherever.
  Acronyms (`URL`, `HTML`) and ordinary capitalised prose
  (`Hello`, `Привіт`) deliberately do NOT trip the heuristic.

The **manual** switch hotkey (`Ctrl+Shift+Backspace`) bypasses both
filters. That's the explicit user-asked-for-it path: when you actually
do want to fix a wrong-layout identifier or a comment line, hit the
hotkey and the engine acts unconditionally.

What this does not (yet) do: distinguish "code" vs "comment" inside
the same editor. That requires per-IDE integration — out of scope for
v0.1. Until then, dev users hit the hotkey when writing comments in
a non-default language.

The forward-compat side: every settings struct now carries
`#[serde(default)]`, so future versions adding new fields read
existing user configs without scary parse errors.

## 2026-05-02 — Phase 4: deferred full GUI; settings = open `config.toml` in editor

PLAN.md §10 originally pencilled `iced` settings pages for Phase 4.
On reflection:

* `iced` (or `egui`) integrated with `tao` + `tray-icon` +
  `global-hotkey` requires careful event-loop juggling, especially
  on macOS where only one runtime can own the main thread.
* The most-used flows (toggle autostart, pick active languages, set
  hotkeys) are perfectly serviceable via direct TOML editing —
  Karabiner-Elements, Alfred and many other tray apps work this way.
* Building the GUI now would lock in choices that may need redoing
  once we know how macOS / Wayland event loops play with iced.

So Phase 4 ships:

* "Open Settings" tray item opens `config.toml` in the user's default
  editor via the cross-platform `opener` crate.
* "Open Logs" tray item opens the log directory.
* "Reload Settings" re-reads `config.toml` and notifies the engine.
* File-backed logging via `tracing-appender` (rotates daily).
* Engine respects `[languages].active` to scope candidate layouts.

Full visual settings UI (iced or egui) is deferred to Phase 8 / v0.2,
when we already know how macOS / Linux event loops behave from
Phases 5 / 6.

> **SUPERSEDED** — see the later 2026-05-07 entry on the settings
> window. The iced GUI shipped during 0.1.0-beta, well ahead of this
> plan, and now has seven panes. It runs as a `--settings` child
> process, which is what defused the macOS main-thread concern that
> motivated the deferral.


## 2026-05-07 — Hunspell stems gap + plateau widening (multi-layout regression)

### The bug

A user typing `має` (Ukrainian for "has") under uk-UA reported the
word being silently deleted. Tracing the pipeline:

1. Buffer captured scancodes `0x2F 0x21 0x28` (the keys `M`, `A`,
   `'` on a US-physical keyboard).
2. Renders, by layout: `vf'` (en-US), `має` (uk-UA), `маэ` (ru-RU),
   **`vfä`** (de-DE), `vf´` (es-ES), `vfù` (fr-FR).
3. `DictionaryDetector` ran first — `має` is **not** in the embedded
   uk-UA FST (next paragraph) — and returned `NoOpinion` because the
   alt renderings also miss the dictionaries.
4. `WordPlausibilityDetector`:
   * `має` (uk-UA) — vowel-ratio = 2/3 = **0.667**, just outside the
     plateau `0.25..=0.55` → `vowel_fit = 0.325`, **`fit = 0.66`**.
     Below the `keep_threshold = 0.7` → no Keep.
   * `vfä` (de-DE) — vowel-ratio = 1/3 = 0.333, *inside* plateau →
     `vowel_fit = 1.0`, `fit = 1.0`. Best alt.
   * Advantage `1.0 − 0.66 = 0.34 ≥ min_advantage 0.25` → **Switch
     to de-DE**.
5. The corrector backspaced `має ` and re-emitted `vfä `. Visually
   the user saw their Ukrainian word vanish under a layout switch.

The regression was introduced when the de-DE / fr-FR layouts joined
the candidate set — with only en-US ↔ uk-UA, the EN render `vf'`
has no Latin vowels and scores ≈ 0.5, never beating `має`'s 0.66 by
the required advantage.

### Why `має` isn't in the FST

The LibreOffice `uk_UA.dic` Hunspell file ships **stems only** —
`мати`, `робити`, `знати` — and expects an `.aff` rules file to
expand them at runtime into the actual inflected forms (`має`,
`робить`, `знає`, …). Our `cargo xtask wordlists fetch` pipeline
processes the `.dic` *without* applying the affix rules, so the
~600+ inflected forms of common verbs are missing from the FST.

A proper Hunspell-aware expander would solve this categorically.
The fix landed in three stages:

### Fix A — extras list (data, the immediate plug)

`data/wordlists/uk_ua-extras.txt` initially shipped the present /
past / future forms of the ~30 highest-frequency Ukrainian verbs
(167 entries). Generated locally by cross-checking against the FST
and keeping only the missing forms. Once Fix C below was in place,
all 167 entries were redundant and the file is back to its
original "escape hatch for genuine gaps" content — but the data
fix is documented here because it's the right reach when a future
gap surfaces and the expander hasn't caught up yet.

### Fix B — plateau widening (algorithm)

`WordPlausibilityDetector::fit` now uses a `0.25..=0.67` plateau
(was `0.25..=0.55`). The wider band catches V-C-V short words like
`має` / `оса` / `eye` / `our` (vowel-ratio = 0.667) which read as
perfectly normal language but missed the old plateau by a hair.
The decay formula's centre shifted from 0.4 to 0.46 (midpoint of
the new range) to keep the off-plateau slope symmetric.

Verified: `руддщ` (gibberish, vowel-ratio = 0.2) still scores 0.42
— below `keep_threshold` — so the symmetric "user typed Cyrillic
but uk-UA was the *active* layout for what was meant to be EN
prose" auto-switch still fires correctly.

### Fix C — Hunspell affix expander (long-term, structural)

`xtask/src/hunspell.rs` implements a small Hunspell `.aff` parser +
`.dic` expander that reads each stem's flag string and produces
all surface forms via the rules. The xtask `wordlists fetch`
command was rewritten to download both `.dic` AND `.aff` from
LibreOffice/dictionaries (we already had `.dic`) and run the
expansion at fetch time rather than just stripping affix flags.

Coverage results (per `cargo xtask wordlists fetch` log):

| Lang | Stems  | Surface forms | Multiplier |
|------|-------:|--------------:|-----------:|
| uk   | 350656 | 3 486 848     |  9.9 ×     |
| ru   | 146269 | 1 436 553     |  9.8 ×     |
| de   | 258202 |   789 398     |  3.1 ×     |
| es   |  58221 |   652 463     | 11.2 ×     |
| fr   |  84139 | 2 139 550     | 25.4 ×     |

The expander is a deliberately *lossy* port — it skips compound-
word generation, PFX × SFX cross-products, and the `ICONV` /
`OCONV` machinery (the latter only matters for spell-check input
normalisation, not vocabulary). The file's module doc-comment
spells out exactly what's in and out of scope so the next person
to extend it knows where to look.

Encoding handling: most modern dictionaries ship UTF-8, but
`de_DE_frami` is still ISO-8859-1. `read_hunspell_text` tries
UTF-8 first, falls back to scanning for the `SET` directive in the
first 2 KB, and decodes byte-for-byte as Latin-1 if the source says
`ISO8859-*` / `LATIN1` / `WINDOWS-1252`. Adding a new dictionary
in another encoding is a single match arm.

> **Superseded 2026-08-01 — that rule was wrong, and it shipped
> mojibake.** Two faults. `ISO8859-*` was matched as a prefix and
> then decoded as Latin-1, so ISO-8859-2 and ISO-8859-7 came out
> scrambled; and the scan ran per-file, but a Hunspell `.dic`
> carries no `SET` of its own — the `.aff` declares the encoding
> for the pair. The `.dic` therefore hit the "no SET, assume
> Latin-1" default every time, which was invisible for the five
> languages bundled then because four are UTF-8 and German is
> genuinely Latin-1. Adding Polish and Greek exposed it: `słowo`
> became `s³owo`, which matches nothing and looks like a word.
> Now `encoding_of_aff` reads the `.aff` once and that encoding
> decodes both halves, Latin-2 and Greek have real 96-entry tables
> (generated from Python's codecs, not typed by hand), and an
> unknown or missing `SET` is a hard error. Guessing an encoding is
> the failure mode here, not a convenience.

Storage on disk: bulk wordlists ship as `data/wordlists/<id>.txt.gz`
rather than raw `.txt`. Raw, the fifteen bundled languages total
583 MB (uk_ua alone is 84 MB after expansion); gzipped they're
97 MB.
Both `poltertype-core/build.rs` (`flate2::read::GzDecoder`) and the xtask
generator (`flate2::write::GzEncoder`) handle the format
transparently, and the build script falls back to a plain `.txt`
of the same stem if the `.gz` is absent — useful when a contributor
has decompressed one to grep through it. Curated `-extras.txt` and
`-stop.txt` files stay plain text; they're small enough that
compression has zero meaningful impact and editing them in any
text editor needs to keep working.

### Why three layers

Defense in depth. The data fix (A) is what closes a real gap on a
specific build; the algorithm fix (B) is what keeps the engine
honest when *some other* legitimate word also misses the dict;
the structural fix (C) is what removes the gap class altogether
for ~95 % of inflected verb forms going forward.

Regression test lives at `poltertype_detect::tests::plausibility_keeps_short_vcv_cyrillic_word`
and replays the exact 6-layout candidate set the engine produces.
The expander itself has eight unit tests under
`xtask::hunspell::tests` covering the SFX / PFX / class / negclass
/ FLAG-mode / continuation / unknown-directive / FLAG-num-rejection
shapes.


## 2026-05-07 — Data files externalised + lazy-loading by OS-active

Two structural problems with the v0.1 baked-in data approach:

1. **Wasteful RAM** — `include_bytes!` baked all six bundled FSTs
   into `poltertype.exe`. A user with `en-US / uk-UA / ru-RU`
   active in Windows still paid for the fr-FR / de-DE / es-ES FSTs
   sitting resident.
2. **The `http ` bug.** `LayoutDb` exposed every bundled layout to
   the detector regardless of whether the user could actually
   switch to it. fr-FR scored well on `http` (latin script, no
   vowels, all letters legal) and the detector picked it; the
   layout switcher then returned `LayoutError::NotActive` *after*
   `apply_correction` had already sent the backspaces, destroying
   the user's word.

### What changed

* **`crates/poltertype-core/build.rs`** writes layout TOMLs, FSTs, and
  stop-word lists to `<workspace>/target/dist/data/` instead of
  embedding them. The workspace target dir is deduced from
  `OUT_DIR` (walks up to a `target` ancestor), which keeps
  `CARGO_TARGET_DIR` overrides working.
* **`crates/poltertype-core/src/data_dir.rs`** — new module that resolves
  the data directory at runtime. Order: `POLTERTYPE_DATA_DIR` env
  → `<exe_dir>/data` (Windows MSI, AppImage AppDir) →
  `<exe_dir>/../Resources/data` (macOS .app) →
  `<exe_dir>/../share/poltertype/data` (FHS Linux) →
  `<workspace>/target/dist/data` (dev fallback). Unit-tested
  against synthesised exe paths so the per-platform shape is
  pinned.
* **`LayoutDb::load(LoadOptions { active_filter, … })`** — new
  loader that takes the OS-active layout list and skips bundled
  TOMLs whose `id` isn't in it. Pre-parsing the `id` line via the
  small `peek_layout_id` helper means we don't even read the FST
  for filtered-out languages.
* **`crates/poltertype-app`** queries `LayoutSwitcher::list_active()` at
  startup (right after building the switcher, before loading
  layouts) and feeds the result into `LoadOptions::active_filter`.
  Adding a language in the OS now needs a PolterType restart,
  which is a documented trade — the alternative is OS-event
  plumbing on three platforms for a one-line restart cost.

### Installer changes

Each installer copies the prepared `data/` tree into the
expected runtime location:

* WiX MSI — two new `<Component>` entries (`DataLayoutMappings`,
  `DataWordlists`) under a fresh `<Directory Id="DataDir" Name="data">`
  inside `APPLICATIONROOTFOLDER`. Component GUIDs are fixed (CNDL0230
  forbids `Guid="*"` once a Component holds both Files and a
  RegistryValue keypath, and ICE38 forces the perUser registry
  keypath). `RemoveFolder` directives walk the tree on uninstall.
* macOS DMG — `cp -R ${DATA_DIR} ${APP_DIR}/Contents/Resources/data`,
  matching `<exe_dir>/../Resources/data`.
* Linux AppImage — `mkdir -p ${APPDIR}/usr/share/${APP_NAME}/data &&
  cp -R ${DATA_DIR}/. <there>/`, the FHS layout the resolver looks
  for at rule 4.

### Plug-in foundations

The data layout reserves `<data_dir>/plugins/<pack-id>/` for the
future language-pack marketplace. v1's plug-in surface will be
**data-only** — TOMLs and FSTs, no native code, no network calls,
no settings hooks — to keep the security review small and the
release cycle quick. Full contract documented in `docs/DATA_LAYOUT.md`.

### Settings UI

Added an iced 0.13–based Settings window (`tiny-skia` renderer to
keep build time and binary size tame). Exposed via:

* Tray menu **"Settings…"** entry — spawns
  `poltertype --settings` as a child process. The subprocess form
  side-steps the macOS main-thread fight between `tray-icon` and
  `iced/winit`: each gets its own process and its own NSApplication.
  When the child exits, the tray sends `EngineCommand::SettingsReloaded`
  so changes apply without an explicit "Reload" click.

Three panes for v1:

* **Languages** — checkboxes for every OS-active layout against
  `[languages].active` (allow-list) and `[languages].ignored`
  (veto). Empty allow-list = "use every OS-active layout", which
  is the default and what most users want.
* **General** — autostart, sound on correction, suppress-in-
  identifiers, idle timeout. Plus shortcut buttons to open the
  raw config.toml, logs dir, user-wordlists dir, user-layouts dir.
* **About** — version, repo links, "Reset to defaults" + "Reload
  from disk" power-user escape hatches.

Hotkey rebinding and exception-app management aren't in v1 — both
need richer UI and live config diffing. Power users still edit the
TOML via the **"Edit config.toml…"** tray entry (which the GUI
"Open config.toml" button also exposes).

> **SUPERSEDED** (by a later entry the same day, and by the code).
> The window did not stop at three panes: **Hotkeys** and
> **Exceptions** both became panes of their own, joined later by
> **Commands** and **Wordlists** — seven in total. Don't read the
> three-pane scope above as current.

## 2026-05-07 (later) — Settings UI completion + plug-in loader v1

Three follow-ups landed in the same day as the externalisation:

### 1. Languages pane: render *effective* state, not the raw list

`[languages].active = []` means "every OS layout is considered" (the
default). The earlier UI rendered the raw list, which meant a fresh
install showed zero ticked checkboxes even though every layout was
working. User-reported confusion.

Fix: the Active checkbox now reflects the engine's actual decision
rule (`list.is_empty() || list.contains(id)`), so on first open every
OS-active layout is shown ticked. When the user un-ticks a box from
that implicit-all state, we materialise the allow-list as "every OS
layout *except* this one" — preserving the user's intent across save.

The narrow alternative — auto-populating `[languages].active` with
every OS layout on first save — would have been simpler but breaks
the "use whatever the OS reports today" semantic. Materialising only
on the first un-tick keeps that semantic free for users who never
visit this pane.

### 2. Hotkey rebinding — capture mode + persisted bindings

`crates/poltertype-app/src/main.rs` now reads `[hotkeys]` from settings
(previously hardcoded `Ctrl+Shift+Space` / `Ctrl+Shift+Backspace`).
Parser is `global-hotkey`'s native `FromStr`, which accepts the same
`Ctrl+Shift+Space` shape we already document. Bad strings fall back
to the documented default with a warn — same loud-but-graceful
contract as malformed user-layout TOMLs.

Settings UI gains a **Hotkeys** pane with one row per binding +
"Rebind" button. Clicking flips the app into capture mode; an iced
`keyboard::on_key_press` subscription routes the next combo. Rules:

* Lone modifier presses (`Ctrl`, `Shift`, `Alt`, `Meta`) are filtered
  — the user hasn't finished composing yet.
* At least one modifier required — single-letter hotkeys would clash
  with normal typing.
* `Esc` cancels capture without rebinding.

The capture serialiser is unit-tested for round-trip through
`global-hotkey::HotKey::from_str` so the GUI can never produce a
combo that the next tray launch silently drops.

Why a subscription rather than per-widget event hooks: capture is
window-global (the user shouldn't have to focus the "Press a
combination..." field first), and a Subscription lets us toggle
listening on/off via the captured `Option<HotkeyKind>`. Outside
capture mode the subscription is `Subscription::none()`, so the
window doesn't allocate a Message on every keystroke.

### 3. Exceptions pane

Simple list-edit over `[exceptions].disabled_apps`: one row per
entry with a `×` button, plus an Add row at the bottom. Add accepts
both Enter-key and Add-button. Case-insensitive dedup (matches the
engine's runtime comparison via `eq_ignore_ascii_case`).

### 4. Plug-in loader v1

`<data_dir>/plugins/<pack-id>/` is now enumerated at every `LayoutDb`
load. Pack shape (per `docs/DATA_LAYOUT.md`):

```
<pack-dir>/
  manifest.toml          {id, name, version, supported_layouts}
  layout-mappings/*.toml
  wordlists/<stem>.fst   (optional; falls back to plausibility-only)
  wordlists/<stem>-stop.txt  (optional)
```

Precedence: `bundled ← plug-ins ← user-overlay` (last writer wins
on `id` collision). Pack dirs sorted alphabetically for
deterministic load order.

**v1 surface is data-only** — no native code, no network, no
settings injection. The loader function is ~80 LOC, every error
path warns and skips, and four unit tests cover happy-path /
missing-manifest / invalid-manifest / user-override-of-plug-in.
This keeps the security review tractable for the eventual
marketplace launch — when remote downloads + signed packs land,
the existing loader's "data only" assumptions stay sound.

### 5. Wordlists pane

A sixth pane in the Settings window for editing the per-layout
user-overlay text files in `<config-dir>/poltertype/wordlists/`.
Two files per layout, mirroring the loader contract documented in
`crates/poltertype-core/src/layouts/files.rs::build_dictionary`:

* `<stem>.txt` — Extras: full-form words merged into the layout's
  `user_overlay` set.
* `<stem>-stop.txt` — Stop list: short tokens (≤2 letters) merged
  into the layout's `short_stop_words`.

The layout id → stem mapping (`en-US` → `en_us`, `kk-Cyrl-KZ` →
`kk_cyrl_kz`) is the same convention used by the bundled
`data/wordlists/<stem>.fst` filenames and by the loader itself, so
the GUI never writes to a path the engine doesn't read. A unit
test pins this mapping for the canonical 6 bundled layouts plus a
hyphen-rich edge case to catch any future drift.

**Why a separate pane and not inline on Languages**

Languages is a yes / no / ignore decision per layout — checkboxes
fit. Wordlist editing is free-form multiline text — needs a real
editor widget (`iced::widget::text_editor`). Combining the two
would cram a dropdown + editor into every language row and dwarf
the simple toggles users hit most often.

**Why no hot-reload**

The engine loads `<stem>.txt` once at startup via
`LayoutDb::load(...)` and merges it into a `LayoutDictionary`
that's then frozen for the life of the process. Hot-reloading
would mean rebuilding every dictionary on the fly while the engine
might be in the middle of a detector pass — extra synchronisation
for a feature users hit rarely (you tweak your wordlist a couple
times a week, max). The pane shows "Saved to ... Restart
PolterType to apply" so the constraint is visible.

> **SUPERSEDED** (0.1.0-beta.10). Wordlist edits now apply when the
> Settings window closes — a three-step reload rebuilds the config,
> the global wordlists and the per-profile cache. The pane text was
> changed to "Close this window to apply"; a full tray restart is no
> longer required.

**Buffer normalisation**

Saves append a trailing newline if the user didn't type one. The
bundled curated lists all end with `\n`, so this keeps `git diff`
quiet for users who keep their config dir under version control.
Parsing on the engine side (`parse_wordlist`) is identical
whether the file ends with `\n` or not — the normalisation is
purely cosmetic.

**Layout picker UX**

A row of layout buttons (one per OS-active layout) rather than a
`pick_list` dropdown. Two reasons: (1) the typical user has 2-3
layouts, so a row of buttons is faster than opening a dropdown to
pick from a 2-element list; (2) the Languages pane already uses
inline checkboxes, so the visual style stays consistent. If the
OS-active list ever grew large (rare even for polyglots) we'd
revisit, but every UI primitive iced ships works on either shape
of input.

## 2026-05-07 (later still) — Smart commands + per-app wordlist profiles

### 1. Smart commands as text triggers, not hotkeys

The first cut wired user commands as additional global hotkeys via
`GlobalHotKeyManager`. We pivoted to text triggers (Espanso /
TextExpander style) for three reasons:

* **OS hotkey limits.** Windows / macOS / X11 all cap the number
  of registered global hotkeys, and any combo a user might pick
  could already be claimed by the system or another app. Text
  triggers have no such limit — users can have hundreds.
* **Visibility.** A hotkey is invisible state ("did I just press
  Ctrl+Alt+S? what did it do?"). A text trigger is right there in
  your buffer — you see what you typed.
* **Architecture fit.** PolterType already runs a word-boundary
  pipeline for layout corrections. Text triggers slot in BEFORE
  the corrector's filters — same `WordBuffer::feed` boundary
  detection, same `KeyEmitter` for backspace + replay. Zero new
  threads, zero new OS surfaces.

The Hotkeys pane stays as it was (the two built-in pause /
switch-last bindings). The new Commands pane is text-trigger only.

### 2. Trigger lookup before auto-switch filters

The smart-command match runs in `decide()` immediately after the
last_word stash, BEFORE the structural-boundary / disabled-app /
identifier filters. Reasoning: those filters exist to suppress
auto-switching when the engine might be wrong (URL context, IDE
context, code-shaped tokens). Text expansion is direct user intent
("I typed `anrl<space>` because I want it expanded") — the engine
is not guessing. So the suppression rules don't apply.

This makes `=>` work as a trigger inside an IDE, where
`looks_like_code_token` would otherwise veto the whole word.

### 3. v1 action surface kept tiny

Three actions (`type_text`, `switch_layout`, `open_path`).
Deliberately small. `run_shell` was tempting but a stolen config
file becomes a remote-execution vector — separate security review.
Multi-token triggers (`best regards` → `…`) would need a sliding
window across word boundaries we don't have today.

Adding new variants is forward-compat through serde: an old binary
encountering a `type = "future_thing"` entry warns and skips that
single command, the rest still load.

### 4. Inline dispatch on the engine thread

The engine's smart-command path runs `send_backspaces` →
`send_text` (or `switch_to` / `opener::open`) inline. Same thread
as the corrector. All three actions complete in well under 50 ms
on the common path; if a future variant becomes slow (network call,
heavy file I/O) the right call is for THAT variant to spawn a
worker — don't pessimise the fast path.

For `TypeText`, the boundary character is re-emitted after the
expansion so the user's typing flow continues — they typed
`anrl<space>`, they expect `<expansion><space>` to land. For
`SwitchLayout` / `OpenPath` the boundary stays consumed (the user
wanted a side-effect, not text continuation).

### 5. Auto-id from display name

The form auto-generates a kebab-case id from the user's display
name (e.g. `"Insert Email Signature"` → `insert-email-signature`).
Empty name falls back to action-typed ids (`type-text`,
`switch-layout`, `open-path`); collisions append `-2`, `-3`, …
deterministically. Users never need to think about ids — they're
exposed in logs and the saved TOML, but the UI surfaces only
display names.

### 6. Per-app wordlist profiles: cache + swap, not rebuild

Profile activation switches the engine's dictionary set in one
`RwLock::write()` — the same `DictionaryDetector::replace_dicts`
primitive the manual "Reload Settings" path already uses. We
build one cached `HashMap<LayoutId, LayoutDictionary>` per
profile up front and stash the global baseline under the empty-
string key, so a focus transition is always a single map lookup +
atomic swap. Building 5 profiles takes 5×N text-file reads (cheap)
because the bundled FSTs are already `Arc`-shared inside
`LayoutDictionary` — only the per-profile `user_overlay`
HashSets are re-derived.

### 7. Focus watcher: 250 ms poll, not OS event

Same cadence as `spawn_layout_poller` already uses. We considered
hooking each platform's "focus changed" event but the gain
(maybe 100 ms faster swap) doesn't justify three platform
implementations + three failure modes. 250 ms is well below the
"I switched apps" perceptual threshold for wordlist purposes,
which only matters at word-boundary time anyway.

### 8. Profile-list management not in v1 UI

The Wordlists pane gets a Profile picker row (Global + each
configured profile), but adding / removing profiles is editable
only in `config.toml` for v1. Reasoning: the profile-management
form needs name + id + apps-list editor + on-disk-cleanup-on-
delete, that's another 200+ LOC of UI on top of an already-
1500-LOC settings_ui.rs. Once users have feedback on which
shapes of profiles they actually want, building the management
UI on top is straightforward.

### What's still on the bench

* **Hotkey capture on Wayland** — iced's keyboard subscription
  works on Windows / X11 / macOS today; Wayland clients don't
  receive grab-style global events while unfocused. The current
  capture works fine when the Settings window is focused (the
  common case for rebinding) but not from a background "rebind via
  hotkey" gesture. Fine for v1.
* **Plug-in marketplace UX** — installation, signing, updates. The
  loader is ready for them; the UI / network plumbing is a separate
  phase whose security model needs its own DECISIONS entry.
* **Profile-list management UI** — see point 7 above. The schema
  + engine wiring + per-profile wordlist editing are all live;
  add/delete/configure-apps in the GUI is queued.
* **Smart command actions** — `run_shell`, multi-token triggers,
  and case-insensitive / case-preserving expansion are deliberately
  out of v1. Each unlocks a different security or UX surface.

## 2026-07-11 — Correction pipeline v2: absorb → delete → replay, echo match-and-consume

Field reports on v0.1.1: (a) "після автоперемикання лишається перший
символ старого слова", (b) "видаляю пару символів, дописую — коректор
переводить пів слова". Root causes and the redesign that fixes them:

**What was wrong.**

* The engine suppressed *everything* for 300-400 ms after a correction
  (blanket lockout) and cleared the word buffer on every event inside
  the window. A fast typist's first keystrokes of the next word landed
  inside that window: on screen but not in the buffer → the next
  correction under-counted its backspaces → the word head stayed behind.
* Keystrokes racing the emission physically interleave with our
  backspace burst at the compositor: each raced key soaks up one
  backspace meant for the word — same visible symptom.
* The buffer had no model of editing across a word boundary: Backspace
  over the space re-entered the previous word on screen, while the
  buffer tracked a brand-new empty word → tail-only corrections.
* `hyprctl` was spawned as a subprocess per keystroke (layout lookup)
  and 4-5× per correction, stretching the race window to 100 ms+.

**What was decided.**

* **Echo handling** — the emitter records every event it puts on the
  wire (`KeyEmitter::take_emitted`); the engine match-and-consumes
  those echoes off the key stream (ordered queue, lookahead 1 for
  remapper-coalesced events, ~800 ms expiry). Real keystrokes are
  processed normally no matter how soon after a correction. The
  blanket lockout is gone.
* **Absorb-before-delete** — a correction first watches the key stream
  until it has been quiet for ~90 ms (3 × 30 ms probes, 600 ms cap).
  Keys typed meanwhile are absorbed into the plan: deleted together
  with the word, re-typed after the boundary in order, and seeded into
  the buffer as the next word. A boundary stops absorption and is
  re-processed through the normal pipeline afterwards, so the next
  word gets its own decision. Enter/Tab or anything murky (Backspace,
  nav, click, shortcut) aborts the correction before any keystroke is
  emitted.
* **Switch first, delete second** — flipping the layout doesn't touch
  text, so a failed switch now aborts with the word intact (previously
  backspaces had already destroyed it), and its propagation overlaps
  the backspace burst.
* **Word-boundary re-open + poisoning** (`WordBuffer`) — the completed
  word and its run of boundary keys stay stashed; backspacing across
  the boundary re-opens the word (scancodes are layout-independent, so
  this survives our own replays). When the buffer *knows* it lost
  track (BS into unseen text, caret moved mid-word via nav keys or
  mouse click, idle gap mid-word, shortcut mid-word), it taints the
  in-progress word; a tainted completion is never auto-corrected and
  drops the manual-switch stash. Mouse clicks are observed by opening
  BTN_LEFT-capable evdev devices and reporting a pseudo-scancode.
* **Hyprland over IPC socket** — `.socket.sock` request/reply with
  `hyprctl` subprocess as fallback, plus a TTL cache (200 ms current /
  2 s list) in front of every Linux backend. Cuts the per-keystroke
  cost from a process spawn to a mutex read and the pre-correction
  latency from ~100 ms to ~2 ms.

**Alternatives considered.**

* `EVIOCGRAB` during corrections (deterministic, no interleaving) —
  rejected: keyd already holds the grab on this class of setup, and
  grabbing keyd's virtual device would swallow our own replay.
* Wayland `zwp_virtual_keyboard_v1` — still the proper long-term path
  for injection; orthogonal to buffer/absorb logic, tracked separately.

## 2026-07-13 — Linux focus tracking + the tray finally admits hook failures

Two of Phase 6's leftovers closed together, because they share a theme:
things the app silently didn't do on Linux.

**Focus tracking (was: the "quietest hole in the product", PLAN §3.9).**
`create_focus_tracker()` returned `NoopFocusTracker` everywhere except
Windows, so `[exceptions].disabled_apps`, per-app wordlist profiles and
`apps = [...]` scoping on smart commands silently did nothing on Linux.
Decisions made:

* **Two backends, probed in order: Hyprland IPC, then X11 EWMH.**
  Hyprland answers `activewindow` over the same UNIX socket the layout
  switcher already uses (`hyprctl` subprocess as fallback); X11 reads
  `_NET_ACTIVE_WINDOW` on the root, then `_NET_WM_PID` on that window.
  Both are polled synchronously — mirroring the Windows tracker's
  model, where the *callers* provide the cadence — no event thread.
* **The reported identity is the executable basename via
  `/proc/<pid>/exe`**, falling back to `/proc/<pid>/comm`, then the
  window class. That makes the answer the exact analogue of the
  Windows tracker (`QueryFullProcessImageNameW` basename), so
  `disabled_apps` entries behave the same across OSes: `"code"`,
  `"alacritty"` — case-insensitive.
* **A 150 ms TTL cache wraps whichever backend wins.** Unlike the
  Win32 query, our backends are real I/O (socket / X round-trip), and
  `focused_exe()` sits on the engine's word-boundary path plus the
  250 ms profile watcher. One real query per 150 ms window is
  invisible to humans and makes the polling free.
* **XWayland's X server is deliberately NOT used as a fallback on
  non-Hyprland Wayland.** It only sees XWayland windows, so its
  `_NET_ACTIVE_WINDOW` goes stale the moment focus moves to a native
  Wayland window — a wrong answer is worse than `None`. GNOME/KDE on
  Wayland keep the noop tracker until per-DE backends (KWin script /
  GNOME extension) exist.
* The IPC socket helpers are a ~40-line copy of
  `poltertype-layout/src/linux/hyprland/ipc.rs` — those are
  `pub(crate)` there, and a shared crate for this much code isn't
  worth the dependency edge yet. Noted in both files.

**Startup-failure surfacing (was: `warn!` into a log file).** When the
input listener failed to start — Wayland without `input`-group access
being the common case — the tray came up looking perfectly healthy and
the app just did nothing. The listener has returned a descriptive
`InputError` since day one *specifically* so the tray could show an
onboarding banner; the plumbing was the missing half. Now a failure:

* prepends a "⚠ Keyboard hooks unavailable — Setup Guide…" tray menu
  entry that opens `docs/PERMISSIONS.md` on GitHub (pinned to `main`,
  so the guide tracks the latest setup script, not the failed binary);
* appends "⚠ no keyboard access, see Setup Guide" to the tooltip
  (part of `TrayState`, so it survives every tray refresh);
* fires one startup notification through the error-notification path,
  which is deliberately NOT gated by `[general].show_notifications` —
  that toggle governs cosmetic switch chatter, and "the app you just
  launched cannot do its job" is not cosmetic.

The original sketch wanted a "Run setup" button that executes
`setup-linux.sh` directly. Rejected for now: that means the tray
spawning a `sudo`/`pkexec` prompt, which is exactly the scary pattern
the docs warn against — a guide the user reads and runs themselves is
more honest. Revisit if onboarding drop-off proves real.

---

## 2026-07-13 — Auto-update from GitHub Releases

**The app now makes a network call.** That sentence is the whole
weight of this decision, so it goes first. Until now `poltertype`'s
default build never opened a socket, and both `CLAUDE.md` and the
landing page said so in as many words. It now polls GitHub for new
releases, downloads them, and installs them on restart. The rest of
this entry is the reasoning and the boundaries.

### Why do it at all

The installers ship **unsigned** (Apple Developer ID and a Windows
OV/EV cert are Phase 9). An unsigned app with no update channel is
not the conservative choice — it is the worst of both worlds: users
sit on old builds forever, and the only way we can reach them with a
security fix is a blog post they will not read. "No auto-update" is
only the safer option for a project that never needs to ship a fix.

### What is actually sent

Nothing. A `GET` of a static JSON asset on `github.com`, with no
query string, no body, no cookie, no identifier. GitHub learns what
any HTTP server learns — the connecting IP, and a User-Agent naming
the running version. That is the irreducible cost of asking "is there
a newer version", and it is why the whole subsystem is one switch:
`[updates].enabled = false` and the app never opens a socket again.
It is not telemetry, it must never grow into telemetry, and the
Settings pane prints the exact URL so the claim can be checked rather
than believed.

### On by default

Yes — and the landing page's "no network calls" copy changes to match,
because the alternative is a promise the binary does not keep. An
opt-in updater buried in a settings pane is one nobody finds, which
means it protects nobody, which means we would have paid the honesty
cost and bought nothing.

### Trust model, and its limit

HTTPS plus a per-artifact SHA-256 from the release manifest. The
download is streamed through the hasher and only renamed into its
final path once the digest matches; a mismatch deletes the bytes and
aborts. There is a test that a mismatched payload leaves *nothing* on
disk, because the failure we cannot afford is a bad artifact reaching
`msiexec`.

Be precise about what that buys: the checksum comes from the *same
release* as the artifact, so it defeats a corrupted transfer and a
tampered asset CDN, but **not** a compromised GitHub account. Whoever
can publish a release can publish a matching checksum. The defence
against that is a detached signature over the manifest, made with a
key that does not live on GitHub — `Manifest.signature` is reserved
for exactly that (ed25519, minisign-style), and the field parses today
so it can be filled in later as a value change rather than a schema
break. Until then the honest statement is: *we trust GitHub as much as
we trust the account that publishes the releases.*

Considered and deferred: signing now. It was the recommendation; the
maintainer chose checksums for this phase to avoid a key-management
burden (a lost private key means users are stranded with no update
path, permanently). The field is there when we want it.

### Never install under the user's hands

The one hard rule. This app owns a global keyboard hook — replacing
its binary mid-sentence would drop keystrokes at best. So the
background worker only ever *stages*: it downloads, verifies, writes
`pending.json`, and stops. The install happens at a moment the user
chose, and only then:

* they click **Quit** — the hook is already down, so we install and
  don't relaunch (they asked for the app to go away);
* they click **⟳ Restart to update** in the tray — install, relaunch.

That is the Chrome/VS Code model, and it is why the tray entry says
"Restart to update" rather than "Update now".

### Why a script on disk, spawned detached

None of the three platforms can replace a running binary: an MSI
cannot overwrite a locked `.exe`, an AppImage cannot be `mv`-ed over
its own live FUSE mount, a `.app` cannot be `ditto`-ed over itself. So
every backend writes a small script into the staging directory, spawns
it **detached** (own process group on Unix, `DETACHED_PROCESS` on
Windows), and the script's first act is to poll until our PID is gone.
The app then exits normally and the installer runs in the gap.

Paths go into a script *file* rather than onto a command line because
the paths involved are home directories — spaces, apostrophes,
non-ASCII — and nested shell quoting is where that turns into a bug.
One layer of quoting instead of three, and a user can read afterwards
exactly what was run on their machine.

Failed installs are counted in `pending.json` *before* the attempt,
not after: an installer that hard-kills us would never reach an
after-the-fact increment, and the broken artifact would be retried on
every quit forever. Three strikes and the staged update is deleted.

### Platform code in a third crate

`CLAUDE.md` said platform code lives in `poltertype-input` and
`poltertype-layout` and nowhere else. Installing an update is
irreducibly per-OS, so the rule now names three crates:
`poltertype-update` is the third. The intent of the rule — platform
`#[cfg]` confined to dedicated platform crates, never sprinkled through
the engine — is unchanged.

### macOS is written, not verified

The `.app`-swap backend follows Apple's documentation and has never
been run on Apple hardware — macOS is a CI-only target for this project.
It also strips `com.apple.quarantine` from the installed bundle, which
is precisely the flag Gatekeeper sets on a download. That is defensible
only because the app is unsigned and the release notes already tell
first-time users to strip it by hand; the moment we ship notarised
builds, that line comes out. Treat the macOS path as unproven until
someone runs it on a Mac.

## 2026-07-13 — Reversed: no default app skip-list

`[exceptions].disabled_apps` used to ship with ~50 entries — VS Code,
Cursor, the JetBrains family, Sublime, Zed, Neovide, kitty, alacritty,
wezterm, konsole, PowerShell, cmd, tmux. The reasoning (above, and it
still reads well) was that corrupting a function name costs far more
than missing a prose correction inside an IDE.

It shipped broken and nobody noticed, because it never ran. Until the
Linux focus tracker landed, `create_focus_tracker()` returned
`NoopFocusTracker` outside Windows, `focused_exe()` returned `None`,
and the match never fired. The list was inert decoration on every
Linux machine.

Then the Hyprland/X11 tracker arrived and armed it. From the user's
side the app simply stopped working: type a wrong-layout word in
Sublime — nothing. In kitty — nothing. No error, no notification, no
log line above `DEBUG`. The correct diagnosis ("your editor is on a
skip-list you never wrote, and a new focus tracker just started
enforcing it") is not one any user is going to reach; "the layout
switching is broken" is.

So the default is now **empty**, and the reasoning above is demoted
from a shipped default to advice. What actually keeps the corrector
out of code is the per-token guard (`suppress_in_identifiers`), the
plausibility-keep, `min_word_length` and the dictionary confidence
threshold — all of which are engine logic that runs in every app on
every OS, needs no focus tracker, and has no way to make the product
look dead. A skip-list is a blunt instrument the user can pick up if
they want it; it is not something we hand them pre-loaded and aimed at
their own editor.

The general lesson is worth more than the specific fix: **a default
whose only saving grace is that it doesn't execute is not a default,
it is a landmine.** When a dormant feature (here: focus tracking) is
implemented for a new platform, every config default gated behind it
changes behaviour on that platform — audit them as part of the same
change, not after a user reports the product as broken.

## 2026-07-24 — Spelling suggestions: surface FSTs, a DFA that survives Cyrillic, and a focus-free tooltip

The engine's promise was "your wrong-layout word gets fixed"; plain
typos (`слоао`, `hwllo`) got nothing. Suggestions close that gap:
when a completed word is kept by the decision pipeline **and** is not
in the current language's dictionary, the engine offers nearby
dictionary words in a small tooltip — click one, or press
`Ctrl+Shift+<digit>`, and the word is replaced in place. On by
default (`[suggestions]`), purely local, and the typed text never
reaches a log line — only counts do.

**A second, surface-form FST per language.** The membership FSTs are
built with the lossy `letters_only_lower` normalisation — `п'ять` is
stored as `пять`, which is fine for "is this a word?" and fatal for
"type this word into the user's text". `build.rs` now also emits
`<stem>-surface.fst` (lowercase, apostrophes folded to `'`, hyphens
kept), and suggestions stream off that one, so `пять` comes back as
`п'ять`. One decoding pass produces both shapes; ≤2-letter surface
entries are dropped (suggestions never target short tokens). Cost:
~2-4 MB per language on disk and resident, only for loaded layouts.

**`levenshtein_automata`, not fst's `levenshtein` feature.** The
candidate walk intersects the surface FST with an edit-distance
automaton (d=1, then d=2 for tokens ≥5 letters when d=1 found
little). fst 0.4's own Levenshtein automaton silently mismatches
multibyte queries — even `слово` within d=1 *of itself* streams zero
results — which rules it out for a Cyrillic-first product. The
tantivy crate handles UTF-8 correctly and counts adjacent
transpositions as one edit, which matches how humans mistype. The
parametric tables build once per Suggester; per-query DFAs are cheap.

**Keyboard-aware ranking.** Raw candidates are re-ranked by a
weighted optimal-string-alignment distance: substituting a key by its
*physical neighbour* costs ~0.4 instead of 1.0 (positions derived
from Set-1 scancodes — layout-independent physics, per-layout char
maps), transpositions 0.6, plus small first/last-letter and
shared-prefix signals, and a penalty for weak-list entries. So
`hwllo` prefers `hello` (w↔e adjacent) over `hallo`, with no
frequency data needed.

**Below-threshold layout verdicts get a second life.** A Switch
verdict whose confidence missed `confidence_threshold` used to be
dropped on the floor. It is now the *leading* tooltip entry, badged
with the target layout; accepting it runs the full switch-and-replay
correction. The engine stays conservative automatically while the
user gets to overrule it with one click.

**The tooltip must never take keyboard focus**, or it would break the
very typing it exists to fix. New `poltertype-popup` crate (a fourth
platform-code island alongside input/layout/update): Wayland uses a
`wlr-layer-shell` overlay with `keyboard_interactivity: None`
(Hyprland/Sway; GNOME/KDE expose no layer-shell to third parties →
noop), X11 an override-redirect window, macOS/Windows are noop seams
for now. Rendering is tiny-skia + cosmic-text — both already in the
tree via iced's software renderer, so no new licences.

**Anchoring: AT-SPI caret first, proxies after.** The tooltip sits
next to the text-insertion point via an anchor chain, best first:

1. **AT-SPI caret extents** — a background watcher (zbus *blocking*,
   no async runtime; the long-lived signal subscription is why this
   is a real bus connection while `poltertype-layout` stays on CLI
   shell-outs) subscribes to `object:text-caret-moved` and resolves
   each event through `GetCharacterExtents`. Extents are requested
   **window-relative** (`ATSPI_COORD_TYPE_WINDOW`) and composed with
   the compositor's live window rect — native-Wayland toolkits
   report *screen* coordinates against the window's initial
   placement, which goes stale on every re-tile (observed live).
   The watcher also raises `org.a11y.Status.IsEnabled` (after the
   a11y-bus handshake — a write during `at-spi-bus-launcher`
   activation gets overwritten by its initial state): toolkits keep
   their a11y bridge dormant until an AT client raises that flag,
   and PolterType now is one. Apps already running before the flag
   went up stay silent until restarted — accepted; the chain
   degrades per-app, not globally.
2. **Pointer inside the focused window** (Hyprland `cursorpos` / X11
   `QueryPointer`) — after a click into the text, the pointer hovers
   near the caret.
3. **Focused window rect** (Hyprland IPC `activewindow` now parsed
   for `at:`/`size:`/`monitor:`; X11 `GetGeometry` +
   `TranslateCoordinates`), bottom-centre — chat inputs and prompts.
4. **Output bottom edge** when nothing is known.

Around the anchor point the popup walks the sides by preference —
above → below → right → left, first side with room wins, clamped to
the output with a margin (`poltertype-popup/src/place.rs`, pure and
unit-tested). "Above" clears the caret line's top, "below" its
bottom, so the line being typed is never covered.

**A click on the tooltip races its own observation.** The engine
watches mouse buttons (BTN_LEFT evdev pseudo-scancode) and treats a
click as "caret moved" — which would kill the very offer the click
is accepting, since the popup's `Accepted` event arrives through
another thread. Resolution: when a pointer press lands while an
offer is live, the engine *freezes* the screen model (word +
separators + in-progress tail) into the offer for a ~500 ms grace
window instead of dropping it. A click ON the overlay never reached
the app below, so the frozen state is exactly what's on screen; an
accept inside the grace applies from it, and the correction's absorb
machinery is granted exactly one benign pointer-press allowance for
whichever ordering the race produced. A click *elsewhere* is voided
by the first following keypress or the grace lapsing. Bare modifier
presses no longer count as "commands" (`is_modifier_scancode`) — on
the Linux listener a modifier's own press already carries its flag,
and `Ctrl↓` was killing the accept chord before its digit arrived;
an idle-timeout carve-out similarly keeps a live offer's word stash
valid while the user pauses to read the tooltip.

**Accept paths and their races.** Tooltip clicks arrive as
`EngineCommand::AcceptSuggestion { generation, index }`; the digit
chord is matched straight off the key stream on *every* platform
(registering nine OS-global hotkeys would steal those combos from
every app even with no tooltip up; stream matching costs one mutex
peek and only while an offer is pending). Every offer carries a
monotonic generation; an accept is honoured only if the generation
matches, the deadline hasn't passed, and the buffer's completed-word
stash still equals the offered word's scancodes — so a stale click
can never replace the wrong text. The replacement itself reuses
`apply_correction` wholesale (absorb window, echo bookkeeping,
compensation loop), with `from == to` skipping the layout-switch
pre-flight and the `Corrected`/`LayoutChanged` events. Suggestion
text is typed as reverse-mapped scancodes (the only injection that
works in terminals); characters the layout can't type (uk apostrophe)
fall back to `send_text`.

**Known trade-offs, accepted deliberately:**

* The accept chord's keypress still reaches the focused app (we
  never block keys) — same accepted risk as the Wayland hotkeys;
  `Ctrl+Shift+digit` is bound by virtually nothing.
* An offer dies when the *next* word completes — the tooltip's
  full 30 s apply only while the word is still the last thing the
  buffer can vouch for. A tooltip that outlives its ability to act
  would be lying about clickability.
* Overlay-only words are suggested in their stripped form (user
  overlays are stored `letters_only_lower`); acceptable for the
  project-jargon lists overlays hold.

**Alternatives considered.** An iced/winit popup window — rejected:
winit can't position toplevels on Wayland at all, and a normal
window steals focus. `zwp_input_method_v2` for real caret rects —
rejected for now: only one IM client may bind it (fcitx users lose),
and it drags a whole input-method identity with it. SymSpell-style
precomputed deletion tables — unnecessary: FST∩DFA already answers
in microseconds at our dictionary sizes without a second index.

**"Add to dictionary" closes the loop.** Field feedback within the
first hour: the tooltip fires on everything the dictionaries don't
know — jargon, names, project vocabulary. The remedy is in the
tooltip itself: a set-apart last row (divider, accent colour) that
appends the word to the user's global overlay file and inserts it
into the running dictionary set **in place** (`add_overlay_word` —
one HashSet insert; a full from-disk reload would re-read and, via
the `&'static` FST plumbing, re-leak every dictionary blob per added
word). The engine only emits `AddToDictionary { layout, word }`; the
app owns the file and the swap. The row rides along only when a
tooltip would show anyway — a popup whose sole content is "add to
dictionary" would itself be the noise it exists to stop.

**Words started mid-text get no tooltip.** Typing right after a
click / arrow keys / Esc means the caret may sit inside a word the
buffer never saw; the typed keys are then a *fragment*, suggestions
computed on a fragment are noise, and accepting one splices the
replacement into the middle of the on-screen word. `WordBuffer` now
records whether each word's first key arrived after an *observed*
separator (`started_clean` on `WordCompleted`), and the engine
offers suggestions only for clean starts. Deliberate asymmetry:
auto layout-correction is NOT gated on this — correcting a
wrong-layout fragment right after clicking into a field is
long-standing, test-pinned behaviour, and the dictionary Keep
protects valid fragments there. Cost accepted: the first word typed
after a click into an empty field misses its tooltip; every word
after it is covered.

## 2026-07-31 — Stray punctuation demotes a token's own-layout credentials

The es–en pair the landing page demos never actually corrected:
typing `mañana` under en-US renders `ma;ana` (ñ sits on the US `;`
key), the dictionary detector looked up the letters-only skeleton
`maana` — which the over-inclusive bulk en list happens to contain —
and vetoed the switch, while the plausibility scorer ignored the `;`
entirely and scored `espa;ol` a perfect 1.0 en-US fit. Decision: a
character that cannot be part of a word in any layout (not alphabetic,
not an apostrophe variant, not a hyphen — `poltertype-detect::text::
non_word_char_count`) now (a) subtracts 0.4 per occurrence from a
rendering's plausibility fit and (b) demotes a current-side dictionary
hit from a Keep veto to a tiebreaker, the same shape the `weak` list
already uses. The skeleton stripping itself stays — it is what lets
the *alt* side recover `mañana` from the scancodes — only its power to
veto on the current side is gone. Clean tokens are untouched: `maana`
typed with no stray characters still keeps.

## 2026-07-31 — Typed words are redacted from every log line and reason

An audit found decision reasons ("current \`maana\` is a dictionary
word") and the correction summary (`original=… corrected=…`, at INFO —
the default level) writing typed words into the on-disk log of a
release build, directly against the README's privacy promise. Decision:
one chokepoint, `poltertype_types::logsafe::redact_word`, through
which every word in a log line or detector reason must pass. It yields
`<N chars>` always — except in a `debug_assertions` build where the
developer exported `POLTERTYPE_UNSAFE_LOG_WORDS=1`, the escape hatch
the self-test recipes need. Release builds redact at compile time; no
configuration reveals typed text there. The alternative — redacting at
the log call sites only — was rejected because reasons travel through
events (`SwitcherEvent`) and every future consumer would inherit the
leak.

## 2026-07-31 — The key gate re-verifies its emitter before every hold

The gate's remapper check ran once, at startup — and raced keyd's own
asynchronous grab of the freshly created uinput device. Winning that
race armed the gate on a stack where it must stand down; the first
correction then grabbed keyd's virtual keyboard, funnelling the user's
keys *and our own corrections* into this process, and the session's
input died until a reboot (observed live, 2026-07-31). Decision: the
probe result is treated as a hint, not a fact — `EvdevGate::service`
re-runs the grab-our-own-emitter check once per hold epoch, before
touching any device, and an `EBUSY` flips the gate off for the rest of
the run (recovery on restart, matching the startup probe's contract).
Defence in depth: the emitter records its `/dev/input/event*` node at
creation (`own_nodes`) so the never-grab-our-own-device exclusion
matches by kernel identity, not by name string. A periodic re-probe
was rejected: grabbing our own emitter outside a hold would swallow
in-flight suggestion emissions; at hold time nothing of ours is on the
wire yet, so the check is free of side effects exactly there.

## 2026-08-05 — The AI feature ships compiled into the release installers

All four installers are now built with
`--features ai,poltertype-ai/remote`, and main CI lints and tests that
feature set so a release configuration can no longer break unseen.
Owner's call, and the reasoning is the product promise itself:
"configure your own model in `config.toml`" is meaningless if the
downloaded build lacks the subsystem — since 0.8.0 the feature existed
and no published release enabled it, which quietly reduced the promise
to "recompile the app yourself". Nothing about the runtime posture
moves: `[ai].enabled` defaults to off, no model / vendor SDK / default
endpoint ships, an entry naming no endpoint and no provider preset is
refused, a non-loopback endpoint additionally needs
`[ai].allow_remote = true`, and keys live in the OS keychain only.
With nothing configured the subsystem builds no detectors and opens no
socket. The honest cost, named because we made the "grep the code"
claim ourselves: the shipped binary now links a second HTTP client
(`reqwest`+`rustls` in `poltertype-ai`) beside the updater's `ureq`,
so every "TLS is only in the updater" sentence in README/docs/site is
retired in the same release. What stays checkable with `cargo tree`:
a stock source build still contains neither the feature nor the
client. Revisit if a supply-chain argument against shipping a dormant
HTTP client ever outweighs the config-only promise, or if a local
in-process backend removes the need for HTTP entirely.

## 2026-08-07 — A layout backend must be probed by what a desktop *does*

Every Linux backend used to be selected by a reachability probe: is the
tool in `PATH`, is the schema installed, does the daemon answer. That
is a check on whether a backend *can run*, and the two are not the same
question. Cinnamon ships `org.gnome.desktop.input-sources` (it comes
with the shared GTK stack), populates it, and never reads it — so the
gsettings backend passed its probe, wrote `current`, and changed
nothing. The failure was silent in the worst available way: the next
`current()` read back our own write, so the engine concluded the
switch had happened and stopped correcting altogether
([#26](https://github.com/Just-Code-NET/PolterType/issues/26), Linux
Mint 22).

The rule from here: where a desktop can be asked something only the
real owner of the layout could answer, ask that instead of inferring
it. Cinnamon's own backend is built on it — calling
`org.Cinnamon.GetInputSources` and seeing whether it answers *is* the
6.6-or-older test, in place of parsing `CinnamonVersion` and encoding
a threshold that would rot. Where no such question exists, the
fallback is a list of desktops known to ignore a schema, kept to what
has actually been verified against upstream source rather than
extended to everything that looks similar.

Two things this decision explicitly rejects. **Not** "an IBus daemon
is running, therefore IBus owns the layout": most desktops run one for
CJK input while switching layouts by a different route entirely, and
Cinnamon activates an `xkb:…` engine on every switch purely so XIM
clients keep working — those engines echo symbols and change no
layout. That inference was the first attempt at fixing #26 and it
would have replaced one silent no-op with another. And **not** a
startup self-test that switches the layout and switches it back to see
whether it took: it is correct, and it flickers the user's keyboard
every launch to answer a question that is nearly always already
settled.

`POLTERTYPE_LAYOUT_BACKEND` exists because all of the above is still
a model of somebody else's input stack. It pins a backend, skips the
probe, and — for `gnome` and `cinnamon` — skips the desktop-name check
those two would otherwise apply, so a user whose setup we have modelled
wrongly is never argued with by a heuristic. An unknown name, or a
backend that will not start, is a startup error rather than a quiet
fall back to probing: the whole point of the variable is to be told
when the choice did not happen.

## 2026-08-07 — A dotted compound is only as plausible as its worst segment

The stray-punctuation term above (2026-07-31) had a blind spot that
made typing a bare domain unusable. `.` is a *letter* in the Cyrillic
layouts — scancode 0x34 is `ю` — so the word buffer correctly keeps
`games.just-code.net` together as one token, and the two renderings of
it are then not comparable at all: the uk-UA one is a clean run of
Cyrillic letters, while the en-US one keeps its literal dots and pays
the stray penalty twice. The correctly-typed domain scored **0.00**
for its own layout against 0.75 for Cyrillic — the most confident
wrong-layout verdict the engine can produce — so the host was rewritten
as `пфьуіюогіе-сщвуютуе`, and the next prose word switched the layout
straight back. A sentence with an address in it switched twice.

Decision: a token made of dots plus word characters is scored one
dot-separated segment at a time, and takes its **worst** segment's
score. The dots themselves stop being counted as noise, because in
that shape they are structure rather than the cross-layout artifact
the term was aimed at.

Why the worst segment and not the mean: the two populations are
separated by their weakest part, not their average. Every segment of
a real hostname reads as a word, whereas a Cyrillic word that merely
contains `ю` leaves at least one segment that reads as nothing —
`союз` → `cj.p`, `купюра` → `reg.hf`, `революція` → `htdjk.wsz`. A
mean lets one strong segment carry a compound that a min correctly
rejects, and those corrections are the ones users actually rely on.

Deliberately narrow in three ways. Only `.` splits — the guard never
looks at `;` or `]`, so the `mañana` case that motivated the stray
term is untouched. A dot sitting next to *other* stray punctuation
(`любов` → `k.,jd`) is a wrong-layout rendering rather than a
compound, and takes the ordinary path. And a leading, trailing or
doubled dot is not compound structure either, so `yjdj.` (`новою`)
still pays the penalty in full.

The residue we accept: a Cyrillic word with `у` on both sides of a
`ю` renders as a compound whose segments both carry a Latin vowel
(`утюгу` → `en.ue`), and its auto-correction is now skipped. That is
a rare word shape against every domain anyone types, and the failure
is the benign direction — the engine stays quiet and the manual
switch-last hotkey still does the job, which is the same trade the
structural-boundary and identifier guards already make.
