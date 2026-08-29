# Decision log

Short-form record of non-obvious technical choices made while
implementing PolterType. Each entry: **what** was decided, **why**,
and any **alternatives** considered.

---

## 2026-08-29 — A deletion is the one shortcut whose effect we can trust

`Ctrl+Backspace` used to leave PolterType refusing the next word typed,
silently, until a space went by (issue #44). The buffer marks the word
in flight as one it can no longer account for whenever a shortcut fires,
because a shortcut can edit text arbitrarily and a correction deletes a
counted number of characters. That mark was lifted only at a word
boundary, so it also covered the *next* word — which is observed from
its first keystroke and is exactly as trustworthy as any other.

**What the mark actually protects.** Not the word it is set on: that one
is already out of the buffer. It protects the word *after* it, from
being spliced into an unrecorded remainder still sitting left of the
caret. `hel` abandoned mid-word, `lo` typed on top, and a correction
counting two characters turns `hello` into `helло`.

So the only question is whether that remainder is still on screen, and
for one class of shortcut it provably is not: a backwards word- or
line-delete erases it. `Ctrl`/`Alt`/`Cmd`+Backspace is read as a
deletion rather than an arbitrary edit now, and a plain Backspace run
that goes past everything we track — which is what `Ctrl+A` then
Backspace comes to — clears an inherited mark for the same reason.

**Alternative rejected: clear the mark whenever a new word starts.** It
answers every reported shape at once and it is one line, but it also
lifts the protection where it is load-bearing. An idle gap abandons the
word in flight without the caret moving, so the remainder really is
still there, and the automatic pass would then correct the fragment
typed after it. The narrower rule leaves that case exactly as it was.

**Alternative rejected: read `Delete` after a selection as a deletion
too.** From the outside, `Delete` with a leftward selection and `Delete`
with nothing selected are the same event, and the second leaves the text
left of the caret untouched. Guessing wrong there corrupts a word rather
than declining one, so `Shift+Home` then `Delete` still declines and
`docs/KNOWN-GAPS.md` says so. `Ctrl+A` could in principle be read as
"the left context is empty now" under both of its common meanings —
select-all, and beginning-of-line — but that is an assumption about an
application's keymap, and this project takes a runtime signal over an
assumption wherever one exists.

---

## 2026-08-29 — `config.toml` is watched, and the pause state lives in it

Two reports from one user on the same build, and the same sentence
answers both: the running app treats the file as the truth and re-reads
it when it moves.

**The rebind that needed a restart (issue #45).** The chords were
re-applied at exactly one moment — when the Settings window the tray
had spawned exited. That is not when a user finds out whether a rebind
worked. They press Save, the banner says "Saved to …", they try the new
chord with the window still open, and nothing happens; a chord typed
straight into `config.toml` never arrived either, short of the tray's
"Reload Settings". Measured here 2026-08-28 against 0.25.1: rewriting
`pause_toggle` under a running app left the engine on the old scancode
indefinitely.

A watcher thread now stats the file once a second and, when it has
moved, re-reads it and puts what changed in force. Polled rather than
`inotify`-watched: one `stat` a second is below noise, needs no new
dependency, and works the same on all three platforms. The stamp is
size *and* mtime, because the edit this exists for — one chord for
another — routinely leaves the file exactly as long as it was.

Only outside edits get through. `SettingsStore::update` keeps the
store's own snapshot in step with what it writes, so a write the app
made itself re-reads as no change and stops at the watcher.

**The pause state that was forgotten (issue #46).** Turning
auto-switch off is a decision, not a mood: it now rides in
`[general].paused`, written every time the hotkey or the tray item is
used, and read at startup. Because the file is watched, it is also
applied live — which is why the engine gained `SetPaused(bool)` rather
than reusing `TogglePause`. A file names a state; a toggle read off a
file lands the wrong way round exactly when the two have drifted, and
the watcher re-applies it after *any* edit, so editing the sound theme
would have resumed a paused app.

The Settings window never shows the field and would have written back
whatever the file held when it opened, silently resuming an app paused
since. Its Save now re-reads that one value off disk and folds it into
what it writes.

The cost, stated plainly: pausing rewrites `config.toml`, and a
serialised TOML round-trip does not preserve comments. That was already
true of the window's Save; it is now true of the pause hotkey too.

Alternative considered and rejected: a separate state file next to
`config.toml`. It would leave comments alone, but it hides a setting
users ask about, cannot be hand-set, and adds a second source of truth
for one boolean.

---

## 2026-08-29 — A correction under a held key does not happen at all

The two-second wait from the day before had a fallback: wait, then
type anyway. Issue #44 is what that fallback does.

Before typing, a correction releases the modifiers the user is
holding — a replay under a held Ctrl produces shortcuts, not text.
On Wayland that release changes nothing, because it comes from a
virtual keyboard that never pressed the key.

Measured on KDE Plasma Wayland, 2026-08-28, and the capture is
unambiguous: every one of the held key's escape sequences carried
`;6` (Ctrl+Shift) right through the burst, and the burst itself
arrived as seven `^H` and `Ctrl+G`, `Ctrl+H`, `Ctrl+B`, `Ctrl+D`,
`Ctrl+T` — while the app's own log reported a clean correction. What
was measured is that the release we send does not take the modifier
up; *why* is presumably libinput declining a release for a key that
device never pressed, which is consistent but was not read out of its
source. On X11 the active grab swallows the same burst instead, so
the fallback has never worked anywhere.

So the wait is now the whole mechanism, and it happens *before* the
layout switch rather than after: nothing is switched, deleted or
typed while the key is down. Past the bound — five seconds now, since
nothing is riding on it being short — the word is left exactly as the
user typed it and the stash is kept, so the gesture answers the next
press. Doing nothing is a real outcome here; it is the only one that
cannot make the document worse.

"Trigger" is answered two ways, because the two kinds of binding are
observed differently. A chord the engine matches off the key stream
gets an exact latch: the scancode of the key that is down, set by the
press that matched and cleared by that key's release.

A chord an OS grab owns gets no latch at all, and the first attempt at
this was wrong to give it one. On X11 the passive grab becomes
*active* on the press, and from that moment the key's raw events are
delivered to the grabbing client alone: the press arrives, the release
never does. Measured on Cinnamon X11, 2026-08-29 — the latch was set
by every press and cleared by nothing, so every correction on every
X11 session waited its five seconds and gave up. For those the only
readable signal is whether the chord's own modifier set is still down,
which is what the previous entry used for everything. It is a fair
reading there and a poor one elsewhere, so now it is used only there.

**Alternative considered and rejected:** pressing the modifier before
releasing it, so libinput has a press to match. It would work, and it
tells the compositor something false about a key the user is holding
— the next keystroke of theirs would arrive unmodified. It also does
nothing for X11, where the grab is the problem. One rule for both
desktops, and no lying about the keyboard.

---

## 2026-08-28 — The Linux update installs itself, and only the restart is delegated

The AppImage updater wrote a shell script, spawned it in its own
process group, and had it wait for our PID to vanish before swapping
the file. That is the standard shape, and on a systemd desktop it is
wrong: a process group is not a cgroup. systemd stops a `.service`
when its main process exits and, under the default
`KillMode=control-group`, SIGKILLs whatever is still in the cgroup —
so the helper died at the instant it was waiting for, inside its poll
loop, having done nothing. The app's own "run at login" toggle writes
exactly such a unit, so the toggle was arming the bug.

It was invisible from both ends. The script's only output was the
greeting it prints before the wait, the app was already gone, and no
failure marker is written on a path that never runs — "quit and never
came back" was the entire symptom, and the retry counter still ticked
down toward deleting the verified download. It was also intermittent
in a way that hid the cause: an instance started by the *previous*
helper inherits a session **scope**, and a scope has no main process
to stop it, so those runs updated fine. One laptop's logs: eight
clicks, five from the unit and all five lost, three from a scope and
all three fine.

**Decided:** the app renames the staged AppImage over the running one
itself, before it spawns anything. `rename(2)` replaces a directory
entry; the image we are executing from keeps its inode alive through
its own open descriptor, so the running app is untouched and the swap
is atomic. Nothing that must not be lost now depends on a process
outliving us. This does not weaken "never install under a live
keyboard hook" — the swap still happens only on Quit or "Restart to
update", and it cannot disturb a running hook the way an MSI or a
bundle replacement can.

The relaunch is the only delegated step left, and it is now allowed to
fail: an update that installed and could not restart the app is
reported as installed, and the app stays running on the old build
rather than quitting into nothing. Under a service the relaunch goes
to a transient `systemd-run` unit — its own cgroup, out of reach of
our teardown — which waits and then `systemctl --user start`s the unit
again. Starting the AppImage directly would leave the app running
beside a dead unit and give the next login a second copy to refuse.
Everywhere else the detached script is kept: a scope does not kill it,
and it assumes no init system.

**Not chosen: `KillMode=process` in our own unit.** One line, and it
fixes only the installs whose unit we wrote — not GNOME's, not KDE's,
not a hand-written one — while telling systemd to stop tracking
processes it should be tracking.

**Not chosen: `exec`ing the new AppImage in place.** It keeps the PID
and the cgroup and needs no helper at all, but the instance lock is
held across `exec` unless every descriptor is `CLOEXEC`, and the exit
path would have to be restructured around a `run_return` event loop to
get control back at all.

---

## 2026-08-28 — A held hotkey is waited out, not typed over

Two mechanisms broke the force-switch when the key was held rather than
tapped, and only one of them was the one we guessed.

The guessed one is real: evdev reports a held key as repeated presses,
the correction window reads any press carrying Ctrl as a shortcut it
cannot reconstruct, and it therefore abandoned the correction the chord
had just asked for. Swallowing the chord's own repeats fixes that, and
it is what the engine now does.

The other only shows on a real X11 session. A `XGrabKey` passive grab
becomes an *active* grab while the key is down, so every keystroke the
correction emits — ours included, XTEST or uinput — is delivered to the
grabbing client instead of to the application. Nothing is typed and
nothing is deleted, and from inside the app it is indistinguishable
from success. Measured on IceWM in the matrix guest, 2026-08-28.

So the correction waits for the trigger to come back up before emitting,
bounded at two seconds. Rejected: emitting anyway and repairing
afterwards (there is nothing to repair — the keystrokes never landed),
and releasing the grab ourselves (it is not ours to release).

"Trigger" is deliberately narrow — a hotkey's own modifier set, not any
modifier. A word closed by a shifted separator is corrected with Shift
still down, and waiting there would put two seconds into ordinary
typing. Where an OS-level grab owns the chord the engine matches
nothing itself, so the app sends the grabbed chords along for
recognition only.

> **The bound was SUPERSEDED on 2026-08-29** — see the entry below.
> "Waits, then emits anyway" was the half that did not survive
> contact: the emitting-anyway path is not a fallback, it is a way of
> typing control characters into someone's document. The waiting half
> stands, and is now the whole of it.

**Alternative considered and rejected:** treating the grab as a
platform quirk and documenting it. The gesture is the product's second
headline feature; "do not hold the key" is not an answer.

---

## 2026-08-28 — Caps Lock is a key-stream binding on every platform

Asked for by name (#41), and it only works under two conditions that
are worth writing down.

PolterType watches keys and never swallows them, so binding Caps Lock
still latches the lock on every press — and the replay is scancodes,
which the system then applies the lock to, so the corrected word comes
back in capitals. The key has to be neutralised in the layout first
(`caps:none`, or the remapper's equivalent). That is a real
precondition, not a caveat: the Hotkeys pane says it under the row.

And once it *is* neutralised, no OS-level shortcut registry can find
the key: `XGrabKey` resolves a key through its keysym, and it no longer
has one. Measured on IceWM — bound with `caps:none` and registered as a
grab, the binding fired on nothing at all.

So Caps Lock is matched off the key stream everywhere and never
registered as a grab, which is the one shape that works on both sides
of that condition. Rejected: refusing the binding (it is what Punto
Switcher users arrive expecting), and tapping the key a second time to
undo the latch ourselves (racy, and on the grab path our own injected
tap can re-enter the hotkey).

---

## 2026-08-28 — A selection is pasted, because the other two ways lied

Converting a selected passage has three plausible mechanisms and two of
them look right until a real session is put in front of them. Both were
measured failing on KDE Plasma Wayland before the third shipped.

`send_text` is the obvious one and is what the word path falls back to.
On Wayland it goes through a Unicode-compose sequence that most
applications swallow or type literally: converting `ghbdsn cdsn` that
way put `43f` on screen.

Replaying scancodes is what the word path actually uses, and it is
right for a word — the corrected text *is* the same keys under another
layout. A selection is not a word. `Ctrl+A` in an editor takes the
trailing newline with it, and no key produces a newline that is safe to
press: Enter submits forms and sends chat messages. An all-or-nothing
mapping then declines every select-all, which is the common case.

Pasting carries whatever the text actually is — newlines, punctuation
that lives on no layout, anything. The cost is that the converted text
sits on the clipboard for as long as the application takes to read it,
and there is no handshake to wait on, so the restore is a timed guess.
That cost is bounded and visible; the other two were silent and wrong.

**Also measured, and not guessable from the code:** the hotkey fires on
the *press*, so its own `Ctrl+Shift` is still down when the copy chord
goes out — and `Ctrl+Shift+C` is not copy anywhere. Waiting for the
release cannot work, because this runs on the thread that reads key
events and would be blocking the very thing it waits for. The
modifiers are released the same way the replay path releases them.

---

## 2026-08-28 — A capability nobody has by default, and nobody is asked to check

Selection conversion reaches into another application's clipboard. That
is a longer reach than anything else in this app has: the word buffer
never leaves memory, and nothing else touches the clipboard at all. So
it is off until asked for — not out of caution about the code, but
because acquiring that reach by *upgrading* is not consent.

The second half is that "supported desktops" is not a list. Whether a
background process can read the clipboard without taking focus depends
on which Wayland protocols the compositor advertises, and measuring
that across the matrix produced an answer no name would have given:
GNOME and Cinnamon's Wayland sessions advertise no data-control
protocol at all, while KDE Plasma advertises only the newer
`ext_data_control` and sway, labwc, Budgie and Xfce advertise both.

So the toggle probes and disables itself with the reason in place of
the setting. A hint listing desktop names would have been wrong within
a release, and would have made every user check whether they were on
it.

GNOME is the trap worth naming: `wl-clipboard` works there, by falling
back to creating a surface and taking focus. A library with that
fallback would have made this feature *appear* to work while doing the
one thing the app must not. The one we use has no fallback, which is
what turns a missing protocol into an honest "unavailable".

---

## 2026-08-27 — An offer aimed at fast typists must outlive fast typing

The tooltip's "Add to dictionary" row is shown to people who type fast
and dismissed by the next keystroke. Those are the same event. The
reporter of #38 never once managed to use it, which is not a bug in the
tooltip so much as a hole where the rest of the interaction should be.

The word is kept when the offer is *missed*, not when it is made: the
dismissal path is the one an accepted offer never reaches, so "the user
did not take this" needs no extra state to detect.

What it holds is the whole design question, because this is the only
place the app keeps words the user typed beyond the single word the
engine is working on. So: eight, newest first, in memory, never
written, never logged, and no `Debug` derive — a list that cannot reach
a bug report by accident. A repeat moves to the front rather than
taking a second slot; the same spelling under two layouts stays two
entries, because it goes into one wordlist and can be a word in one
language and gibberish in the other.

**Alternative rejected:** persisting the list across restarts, which is
what "review them later" most literally asks for. It turns a menu into
a file of things you typed, and the honest version of that is a feature
with its own consent, not a side effect of a tooltip you missed.

---

## 2026-08-27 — Measure a toolkit upgrade before believing the estimate

`iced_winit` 0.13's boot window (#35) had stood for eleven releases
behind the judgement that fixing it "rewrites every pane of the
Settings UI". That estimate was never compiled. It was 45 errors across
nine files, all of them renames: builders where constructors used to
take arguments, `Status::Focused` growing a field, `application()`
taking boot where it took the title.

The lesson is not that the estimate was too high. It is that a
dependency bump has a number attached and the number takes ten minutes
to get, so an upgrade deferred on a *guess* about its size is deferred
on nothing.

The second half is that a compiling upgrade is not a working one. Three
of the four spellings of the brand-mark canvas compiled; one drew the
mark at the window's origin, one masked it to a sliver, one dropped it
entirely, and only a screenshot could tell them apart. Two of the three
tiny-skia workarounds this repo carried were then dropped on evidence —
the mask bug is fixed, the resize crash is gone — and the third was
kept precisely because its symptom (flicker during a live palette
change) is the one no screenshot can catch.

---

## 2026-08-27 — A gesture you can only make once is not a gesture

The force-switch consumed the stash to reach the switch, so it answered
once per word and a press made in error could not be taken back
(issue #37). That was not an oversight: emptying the stash is what
stopped the hotkey looping. Win32 `RegisterHotKey` reads the Backspaces
we emit together with the user's still-held Ctrl+Shift as a fresh
press, and `wow ` once grew to `wow wow wow…` until the app was killed.

The guard moves from "there is nothing left to switch" to "not within
`FORCE_SWITCH_REARM` of the last one". It is a weaker kind of guard —
a window in time rather than an impossibility — and it is the only kind
available once the gesture has to be repeatable, because the echo and
the second press are the same event arriving at different times. 200 ms
separates them by an order of magnitude at both ends: the echo is
queued while we are still injecting, and a person has to read the
result before pressing again.

What a repeated press *does* is a rotation, not a toggle. Undo is
reserved for the engine's own correction, because that is the press
that also teaches the word — and taking back a press of your own says
nothing about the word, so `LastWord::user_placed` marks whose doing
the current rendering is. Rotation also costs nothing to generalise: it
walks the OS's active layout list, which with two layouts is a toggle
and with three finally makes the third reachable.

**Alternative rejected:** keeping the stash self-consuming and adding a
separate "redo" hotkey. Two gestures for one idea, and the second one
would need a default binding on four platforms.

---

## 2026-08-27 — Two paths read the key stream, so the latch belongs to neither

Chord matchers are edge-triggered: one fire per physical press, latched
from press until release. The latch lived in the run loop, which is
only one of the two places key events are read — a correction reads the
channel directly, to see what raced the replay.

So the release of the key that *closed the word* was swallowed by the
correction that word triggered, and the latch stayed down for good.
`Ctrl+Shift+Space` — the default pause chord, whose key is Space —
died at the first correction of the session. The force-switch had the
milder form and answered every other press, which is what made #37 look
half-fixed after the stash was put back.

The state moves onto the engine and the correction window feeds
swallowed releases into it, matching but never dispatching: we are
inside `apply_correction` and must not re-enter it. A gesture made
during a correction burst is therefore dropped rather than queued —
which is what already happened to it, only now without the latch
staying stuck afterwards.

**Alternative rejected:** not consuming releases of keys that belong to
a chord. The window cannot know which of them the engine is mid-replay
for, and a release it declines to read is one the buffer never learns
about.

---

## 2026-08-27 — A setting that outlives the desktop that wrote it

`org.gnome.desktop.input-sources` is not a property of the running
session. It is a key in `dconf`, a file in the user's home, and once
GNOME has configured two layouts there it stays configured for every
session that user ever starts — i3, fluxbox, Xfce, anything.

The gsettings backend read "populated" as "this desktop drives the
layout through me". On a matrix guest whose dconf an earlier GNOME run
had filled, six X11 sessions took that backend, wrote the key, watched
their own session put the layout back, and declined every correction —
the same six that correct fine with the key empty.

**Decision: invert the test.** The backend claimed every session except
those on a stand-down list (Cinnamon, MATE, wlroots), which could only
ever name the desktops somebody had already been bitten by. It now
claims a session only when the desktop is one whose own daemon acts on
that schema — GNOME and its family — and everything else falls through
to the backend that drives that session for real.
`POLTERTYPE_LAYOUT_BACKEND=gnome` stays as the override for a desktop
we have not heard of.

**Kept, not folded in: the wlroots check.** It scans for a running
compositor rather than trusting a name, because Budgie's Wayland
session *is* labwc and calls itself Budgie — the one case where the
positive list would say yes and be wrong.

---

## 2026-08-27 — Ask the desktop what its font is called

The Settings window and the suggestion tooltip both drew text through
`cosmic-text`, and both asked for a family by a name nobody guarantees:
iced's `Font::DEFAULT` *is* the string "Fira Sans", and cosmic-text
resolves the generic `SansSerif` to the same string. Where the machine
has no Fira Sans — this development laptop included — the request falls
through to whichever installed face the font database answers with. On
Ubuntu 26.04 that was a face with no text glyphs: the window rendered
its headers (bold, matched elsewhere) and its layout ids (monospace)
and nothing else.

**Decision:** resolve the family once, per OS, in `poltertype-shell` —
`fc-match sans-serif` on Linux, Segoe UI on Windows, Helvetica Neue on
macOS — and hand it to iced as the default font and to cosmic-text as
the meaning of `SansSerif`. The window then wears the desktop's own UI
font, which is what a native app should look like anyway.

**Not chosen: bundling a font.** It is the deterministic answer and
iced offers the feature, but its `fira-sans` feature ships the file on
`wasm32` only, so it would mean a `.ttf` in this repo — the one thing
`poltertype-icon` exists to avoid — for a window opened once a month.

**The bug was found late for a bad reason**, worth recording: the
desktop-matrix VM's screenshots come from `VBoxManage screenshotpng`,
which returns a stale framebuffer. Five of them showed the window
"missing text" that was in fact drawn, and two showed text that had
been fixed as still missing. Every conclusion about a window's
contents now comes from a grab taken *inside* the session.

---

## 2026-08-27 — A hotkey with no key in it fires on the way up

Punto Switcher and Caramba users ask for the same gesture every time
(issue #32): two modifiers, left hand, no third key. It cannot be an
OS-level grab — `HotKey::new` takes a key code and there is no key —
so a modifier-only binding is matched off the raw key stream instead,
on **every** platform, while ordinary chords keep the OS grab
everywhere except the Wayland/evdev backend. One hotkey never takes
both paths, so nothing can double-fire.

**Decision: judge the gesture on release, not on press.** The chord
fires when the last modifier comes back up, and only if the set held
was exactly the chord's, no other key was pressed in between, and the
hold was shorter than half a second. That single rule is what lets
`Ctrl+Shift` coexist with `Ctrl+Shift+V`, and `Shift+Shift` with
typing capitals — no allow-list of shortcuts to keep in sync with the
applications people use.

**Decision: refuse a single lone modifier.** A bare `Shift` binding is
the one Caramba offers and the one that cannot be made safe here: on
Linux a mouse button reaches the engine as a key event and poisons the
hold, but on Windows and macOS we do not see clicks at all, so
Shift+click would fire a correction. Two modifiers or two taps are
always deliberate. Caps Lock is refused for a different reason: we
observe it rather than consume it, so a binding would flip the lock as
well as fire.

> **The Caps Lock half was SUPERSEDED on 2026-08-28** — see the entry
> of that date above. The reasoning still holds; the conclusion does
> not. The lock can be taken off the key in the layout, which is
> something the user does once, and the binding is then the only thing
> left on it. Refusing on the user's behalf was the wrong call for a
> gesture people arrive with from Punto Switcher (#41).

**Alternative not chosen: a mouse hook.** It would buy back the
lone-modifier binding at the cost of a second global hook on two
platforms, for a gesture the two-modifier and double-tap forms already
cover.

**Alternative not chosen: converting a selection**, the other half of
#32. It needs clipboard save/restore and a synthesised `Ctrl+C`, and
it matters least where the detector is good. It stays on the list.

---

## 2026-08-26 — A process the OS created is not a process that ran

The Windows self-update never once worked. Not on one machine: the
updater spawned an installer five times across three releases on the
maintainer's own laptop, and Windows Installer has no record of a single
transaction from the staging directory.

The installer was spawned with `DETACHED_PROCESS | CREATE_NO_WINDOW`,
on the reasoning that detachment is what keeps a child alive after its
parent exits. On Windows nothing kills a child when its parent exits, so
that flag bought nothing — and it costs the child its console entirely,
which Windows PowerShell 5.1 cannot start without. The event log shows
it: 40961 "console is starting up", then no IPC thread, no "ready", no
exit event. It died mid-initialisation, every time, before the first
line of a script that had been carefully tested by hand.

**Decision:** `CREATE_NO_WINDOW` alone, which is what
`poltertype-autostart` and `poltertype-shell` were already using for
their own child processes.

**And a second decision, because the flag is not the interesting part.**
`Command::spawn` returning `Ok` proves the OS made a process and nothing
else, and the app was quitting on that. Every installer script now
prints one line before it can fail, and the app reads that line back
before it leaves. No greeting, no hand-off: it stays running, tells the
user, and does not count the attempt against the downloaded artifact.

**Not chosen: trusting the fix.** A bug that survived three releases
because "the installer path was verified" — by running the script from a
console, which is the one context where the missing console does not
matter — does not get to be closed by a code change either.
`docs/KNOWN-GAPS.md` says what is still owed: a click on "Restart to
update" on a real install fed by a published release.

**Also fixed, from the same reading:** the relaunch was inside the
success branch on all three platforms, so an install the OS refused
ended with no PolterType running at all — the old binary being untouched
and perfectly able to start.

## 2026-08-26 — The word a person means is the one under their fingers

Two issues, three reporters, one sentence between them: the force-switch
hotkey does not work. It did work — on a word already closed by a space.
The stash it reads is written by `decide()`, which runs at a word
boundary, so a word still being typed had no stash and the hotkey
returned after a DEBUG line nobody sees.

Everyone who asked came from Punto Switcher or Caramba, where the
gesture is: type, see the wrong layout, press the key. No space is
involved, because the word is not finished — noticing is what finishes
it.

**Decision:** the hotkey falls back to the buffer's in-progress word.
`LastWord.boundary` becomes an `Option`, since an unfinished word has
none, and the correction then erases the word alone rather than the word
plus a separator that is not there.

**Why the buffer is abandoned afterwards:** the keys are still in it, so
the boundary that eventually arrives would run `decide()` over a word
the user has just settled the layout of by hand. The only thing that
second opinion can do is disagree with them.

**Not chosen: re-stamping `word_layout` and letting the word through the
normal decision.** Cheaper, and it makes the engine argue with an
explicit instruction — the one thing this hotkey exists to be immune to.

## 2026-08-25 — Ask the thing itself, never a proxy for it

Eight bugs came out of one desktop-matrix sweep, all the same mistake:
deciding something about the world from a signal that merely correlates
with it.

| It asked | It should have asked | What went wrong |
|---|---|---|
| is fcitx5 running? | is fcitx5 this session's input method? | Ubuntu autostarts it; it took the keyboard on eleven desktops and owned nothing |
| what does `XDG_SESSION_TYPE` say? | which display socket exists? | GDM registers X11 sessions while its greeter is still Wayland, so every X11 session claimed to be Wayland |
| is the `input-sources` schema populated? | does this desktop read it? | GTK ships the schema; Cinnamon, MATE and the wlroots compositors all ignore it |
| did the write succeed? | did the layout move? | GNOME 49 accepts `current` and acts on neither it nor `mru-sources` |
| did the layout move? | did it *stay* moved? | MATE lets the lock land and restores its own group a moment later |
| how many times was Caps Lock pressed? | what does the kernel LED say? | `caps:escape` and `grp:caps_toggle` press the key and latch nothing |

**Decision:** act only where the result can be checked against
something able to disagree. `LayoutSwitcher::verify_switched` returns
`Option<bool>`; `None` means "I can only read my own write" and is not
success. Where an independent reading exists it is sampled repeatedly —
a settings daemon that overrules us does not do it instantly. A desktop
with no such reading gets no backend, and the log names it.

Cost: MATE and three wlroots sessions now correct nothing. Better than
before, where they deleted the word and retyped it unchanged.

**Alternative considered:** keep acting and let the user notice. That
is what shipped, and why all eight stayed invisible — each looked like
"PolterType just didn't fire".

**Alternative considered:** name-list the broken desktops instead of
checking. Two bugs were found *because* the name list was wrong:
Budgie's Wayland session is labwc and calls itself Budgie. Names are a
last resort, and each carries the measurement that put it there.

---

## 2026-08-24 — Caps Lock is a lock the OS owns, and it is not a Shift key

Two things were wrong with one flag. Both listeners on Linux, and the
macOS one, reported `Modifiers.shift` as *shift XOR caps* — the level
the character came out on — and the correction replay pressed that
straight back out as a physical modifier. xkb then applies the lock a
second time: `ALPHABETIC` keys route `Shift` and `Lock` to level 2
separately and leave the *combination* on level 1, so a capital the
lock produced comes back lower-case; `TWO_LEVEL` keys — every digit and
punctuation mark — do not list `Lock` at all, so a `1` typed under the
lock comes back as `!`. The same folded bit also stood in for "Shift is
held" in chord matching and in the pre-replay modifier release.

The second half is worse, because it needs no Caps Lock at all. The
evdev and X11 backends *counted* `KEY_CAPSLOCK` presses. The key is
routinely given another job — `caps:escape`, `caps:ctrl_modifier`,
`grp:caps_toggle` (Caps Lock switches layout, which is how the reporter
of
[#33](https://github.com/Just-Code-NET/PolterType/issues/33) switches
his) — and then it latches nothing whatsoever: `xkbcli compile-keymap
--options caps:escape` gives `key <CAPS> { type="ONE_LEVEL", symbols[1]
= [ Escape ] }`, with no `Caps_Lock` symbol anywhere. Every press still
arrives at evdev, below xkb, and flipped a counter that then stayed
wrong for the rest of the session.

**Decision:** `Modifiers.shift` is the physical key and only that, and
`Modifiers.caps` carries the lock, read from the OS in all four
backends — the kernel `LED_CAPSL` on evdev, `QueryPointer`'s `Lock` bit
on X11, `CGEventFlagAlphaShift` on macOS, `GetKeyState(VK_CAPITAL) & 1`
on Windows. `WordKey` carries both, `LayoutMapping::translate_key`
applies the lock the way xkb does (letters only, Shift cancels it), and
the replay presses the physical state. Turning *characters* back into
keystrokes — the suggestion accept and the `send_text` fallback — goes
through `press_for_char`, which subtracts a live lock.

**Alternative considered:** keep the folded bit and subtract the lock at
the emit sites only. Fewer lines, but it leaves `Modifiers.shift`
lying to chord matching and to `modifiers_held()` — with the lock on,
every correction then waits out the full absorb deadline and emits a
Shift release nobody asked for.

**Alternative considered:** drop the lock from the rendering entirely,
as the Windows listener already did. The replay comes out right for
free, but the buffer then reads `hello` where the screen says `HELLO`,
which is exactly the input the ALL-CAPS abbreviation guard and the
suggestion's capitalisation are reading.

---

## 2026-08-23 — Deleting text we never saw is a statement about the caret, not about the next word

`WordBuffer` poisoned itself when Backspace ran past everything it
tracks, and the poison was read at the *next* word's boundary as
`tainted` — which the engine treats as "do not correct, do not stash,
do not offer". So rubbing out a line and retyping it in the wrong
layout produced nothing at all, for as long as it took to type another
separator.

The flag was in the wrong place. Poison means "a word was in flight and
we lost its head, so a correction would count backspaces against text
we cannot see". At that point in the Backspace branch `keys` is empty
by construction — there is no word in flight, and every keystroke of
the word typed *next* is observed from the first one. The count a
correction derives is therefore exact.

What is genuinely lost is the context: the caret may now sit mid-word
in text we never saw, so the keys we are watching could be the tail of
a longer word on screen. That is exactly what `context_clean` records,
and the suggestion tooltip — which would splice a replacement into the
middle of that word — already stands down on it. Auto-correction does
not, deliberately: it retypes only the keys it saw, and has been
allowed to do so after clicks and idle abandons since the flag existed.

**Decision:** the branch clears `context_clean` and drops the stashes,
and does not poison. A click or an arrow key *mid-word* still poisons —
there `abandon()` sets it because keys really were in flight.

**Alternative considered:** keep the poison and clear it on the first
key of a new word. Same effect, one more state transition to get wrong,
and it would still read as "tainted" to anything that asked in between.

---

## 2026-08-23 — A rendering with punctuation inside it cannot speak for a layout

`тех` typed in Ukrainian came back as `nt[`. The en-US rendering of
those three scancodes is `nt[`; the dictionary detector strips
non-letters before lookup, and the skeleton `nt` is in
`dwyl/english-words`. A 0.95-confidence switch followed, and the
bracket went into the user's text.

The current-layout side of that detector has refused such hits since
0.6.3 — `ma;ana`'s skeleton `maana` is in the same bulk list, and
letting it veto froze the Spanish correction the landing page demos.
The alt side never got the same rule, and it needs it more: a Keep is a
missed correction, while an accepted alt is text the user has to
repair.

**Decision:** a candidate whose raw rendering carries stray punctuation
(apostrophes and hyphens exempt, as everywhere else) is dropped from
the alternates before either sweep. Plausibility already penalises
strays 0.4 apiece, so such a candidate can no longer be chosen by
either detector.

**Why this costs nothing in the common direction:** every Latin letter
key is a letter in the Cyrillic layouts too, so an English word typed
under uk-UA renders as pure Cyrillic and is unaffected. The rule only
bites where the *alt* is the one holding a bracket — which is the case
this fixes.

---

## 2026-08-23 — A token that opens with a hyphen is a flag, not prose

`command --wsl ` came back as `command --цід `. `-` is a word character
in the buffer, and has to be: `well-known` must stay one token, and
`будь-що` is the reason compound scoring exists at all. The consequence
is that a command-line flag arrives at the detectors as a single token
whose leading separators are *inside* it, where the structural-boundary
filters — which read the character that ends a token, and the one
before it — cannot see them. The identifier guard misses it too: it
looks for underscores, digits and camel case.

**Decision:** the pre-decision filters gain one more — a token whose
rendering opens with `-` is never auto-switched. Unconditional, like
the other structural filters, and the manual switch-last hotkey
bypasses it like everything else in that block.

**Alternatives considered:** teaching `looks_like_code_token` about the
hyphen would have been the natural home, but it is fed a *cleaned*
rendering that drops cross-layout punctuation, and on a machine with
de-DE active `-` is one of those (`ß` sits on that key). Making `-` a
boundary at word start instead would split `-` off cleanly, but every
consumer of the buffer would then have to reason about a token that
starts where the user did not start typing.

`wsl` was separately added to the shell vocabulary in
`data/wordlists/en_us-extras.txt`, so the bare word is safe as well;
`tests/shell_vocabulary.rs` checks every entry there against the other
bundled dictionaries.

---

## 2026-08-23 — `injected` means *ours*, not *synthetic*

The engine drops key events flagged `injected`. It has to: on Windows
and macOS our own corrections come back through the same listener, and
an engine that reads its own replay corrects it again.

The two platforms disagreed about what the flag meant. macOS sets it
from `kCGEventSourceUserData` — the marker our emitter stamps, and
nothing else. Windows computed the same `ours` from `dwExtraInfo` and
then OR-ed in `LLKHF_INJECTED`, which the OS sets for anything
synthetic from anyone. The listener's own comment said the opposite of
what the line did: "another automation tool's synthetic keys are
injected too, and the gate holds those back exactly like the user's".

What that cost is a whole class of user, on one platform, silently. A
software KM switch typing from another machine, the on-screen
keyboard, voice typing, a remapper that re-injects — all of it reached
the hook, was tagged, and was thrown away. No log line, no setting, no
entry in `KNOWN-GAPS.md`; PolterType simply did nothing while the same
setup worked on macOS.

**Decision:** Windows sets `injected` from the marker alone, as macOS
does. The key gate is unaffected — it already asked `swallow(ours)`,
which is the question it needs.

**The risk that argues the other way** is a remapper that proxies our
own keystrokes through its virtual keyboard, stripping the marker, so
the engine reads its own correction as user input and spirals. That is
not hypothetical — it is what keyd does on Linux — and the answer
already exists everywhere: the expected-echo queue
(`engine/switcher/echo.rs`), which match-and-consumes the scancodes
the emitter put on the wire. It has never fired on Windows, because
nothing on Windows was stripping anything.

**A second thing this bought, and the reason it went unnoticed for so
long.** Injection is the only way to drive the app without a person at
the keyboard. The Windows build was constructed to ignore exactly
that, so no automated end-to-end test of the Windows runtime was
possible — which is why every Windows bullet in `KNOWN-GAPS.md` rested
on a human at a keyboard, and why several of them had gone three
releases without anyone looking.

---

## 2026-08-23 — The tooltip needs the whole process to agree about pixels

`poltertype.exe` shipped with no application manifest, so Windows ran
it **DPI-unaware**. Two consequences, and they point in opposite
directions: every coordinate the OS hands such a process is virtualised
to 96 DPI, while `GetDpiForMonitor` keeps answering the monitor's real
DPI.

The Windows tooltip uses both. It renders at the anchor monitor's scale
and then places the result in virtual-screen coordinates
(`popup/windows/popup.rs`), so the size and the position were measured
in different units on any panel not at 100% — which is most Windows
laptops. DWM then stretched the layered surface on top of that.

**Decision:** the resource script `poltertype-app/build.rs` already
generates now also carries a manifest declaring `PerMonitorV2`
(`PerMonitor` for pre-1607). Both halves become physical pixels, which
is what the placement arithmetic always assumed.

**Not chosen: `SetProcessDpiAwarenessContext` at startup.** It would
have to be platform code in a crate that is allowed to hold it, run
before any window exists, and race the iced Settings window's own
winit call. A manifest is read before the process starts and cannot
lose that race. It also covers `poltertype --settings`, which is a
separate process out of the same binary.

**Unverified, deliberately stated:** this was reasoned from the Win32
contract and observed only at 100% scale, the only panel the machine
that made the change has.

## 2026-08-22 — A caret sample is worthless without the window it came from

The suggestion tooltip on Linux takes its position from one AT-SPI
caret slot: a background thread folds every `object:text-caret-moved`
signal on the a11y bus into a single "freshest sample", and the anchor
composes those window-relative coordinates with the *focused* window's
rect. Every application on the desktop writes into that slot, and
nothing recorded whose caret it was. The consumer's only sanity check
was whether the composed point landed inside the focused window —
which, for a maximised window, almost anything does.

That is fine exactly as long as the app being typed into is the one
that last moved a caret. It usually is not. Terminals and editors that
draw their own text have no accessibility bridge at all, and Electron
apps emit caret events in the *background*: measured on this desktop, a
chat client fired one every ~30 s while another window was focused, its
caret sitting at (340, 553) of a 1062×606 window. Composed with a
maximised editor's rect that becomes a point roughly a thousand pixels
from where the user is actually typing — inside the window, so the
check passed, and the tooltip appeared there.

**A sample now carries its owner.** The watcher resolves the sending
connection's PID off the a11y bus (`GetConnectionUnixProcessID`, the
same trick the focus watcher uses) and walks the object's `Parent`
chain to the toplevel to read the window size that application reports
for itself. An event whose PID cannot be resolved is dropped rather
than stored. The anchor accepts a caret only when both sides that can
answer agree: same process, and a window size matching the
compositor's rect. Nine applications were measured across GTK, Qt,
Chromium, Gecko, native Wayland and XWayland, and every one of them
reported its window size to the pixel, so that second check is
precise rather than a tolerance, and it separates two windows of one
process, which the PID alone cannot. What was *not* measured is an
application on a fractionally-scaled output, where a toolkit answering
in device pixels would disagree by the scale factor; the mismatch is
logged with both sizes and costs that window the caret rung rather
than misplacing anything, which is the direction to be wrong in.

Cost: the parent walk is deep in web-based apps — a VS Code caret sits
36 levels below its window — so the toplevel is cached per object, and
a burst of caret events during typing pays for it once. The first
attempt capped the walk at 32 hops and silently cost every Electron app
its window identity.

**Two smaller lies were on the same path.** `is_degenerate` treated
only a zero-*area* rect as "no glyph here", but Chromium and Electron
answer `(-1, -1, -1, -1)` for a caret offset of `-1`; that survived as
a point one pixel outside the window and was rejected only by luck.
And VS Code returns a zero rect for *every* offset, so its caret was
never usable at all and the tooltip fell to the window's bottom-centre
— itself far from a caret at the top of a file. The caret event there
comes from the invisible one-character input Monaco parks at the caret
for IME, whose own rectangle is the answer we wanted; it is now the
last resort, guarded by a shape test so a chat composer's 610-px-wide
box can never stand in for a caret and put the tooltip at the start of
the line.

**Placement stopped trusting a second source of truth.** The anchor
used to carry the name and origin of the output containing it, looked
up from Hyprland's `monitors` reply, because layer-shell margins are
output-local. The popup thread is a Wayland client with its own,
authoritative view of the same outputs — so it now finds the output
whose logical rectangle contains the anchor point and measures the
margins from that. A failed monitor lookup used to leave the origin at
(0, 0) while the coordinates stayed global, which put the tooltip on
the wrong monitor; that state cannot be reached any more, one IPC
round-trip per tooltip is gone, and `PopupAnchor` lost three fields.
The surface also declares `exclusive_zone = -1`: the placement is
computed against the whole output, and without it the compositor
measures the margins from whatever a panel's exclusive zone leaves
over, sliding every tooltip by the height of the user's bar.

Verified live on the four-output Hyprland session the earlier placement
work used — including the `transform: 3` output (logical 1440×2560) and
the fractional-scale one (2048×1280 at scale 1.25) — and against the
real a11y bus: VS Code now yields a caret that tracks the typing
position, and a chat client's background caret is refused while the
editor is focused.

---

## 2026-08-16 — Manifest signatures become mandatory, eight releases late

`REQUIRE_SIGNATURE` was written as a two-stage rollout with an explicit
condition for stage two: flip once a signed manifest has been the
published `latest.json` for a full release cycle, so **v0.9.0 at the
earliest**. Checked live through the real redirector on 2026-08-16:
v0.9.0, v0.12.0, v0.14.4, v0.16.0, v0.17.0 and v0.17.1 all carry a
`"signature"`, and so does every release back to v0.7.0. The condition
was met eight releases ago and nobody flipped the constant, because
nothing anywhere fails while it is `false` — which is the same shape of
silence as the entry below, in a different file.

**Decision:** flip it in v0.17.2.

**What we are buying.** An attacker who compromises the GitHub account
can publish a release, and until now could publish an *update*: the
updater accepted a manifest with no signature at all. The checksums in
that manifest are no defence — they live in the same release as the
installer they describe. From v0.17.2 the manifest must be signed by a
key that has never been in CI, and is checked before any URL in it is
read.

**What it costs, stated plainly because it is now load-bearing.**
Signing is a manual step, deliberately: the private key must not be a
GitHub secret, since the person it defends against is exactly the
person who can read those secrets. So publishing a release and
forgetting to sign it is no longer a warning in somebody's log — it is
every updater on v0.17.2 or newer refusing that release until somebody
signs and re-uploads `latest.json`. `RELEASING.md` §7 is the whole
mechanism preventing that, which is why this release also rewrites the
blockquote there from "nothing fails if you skip it" to the opposite.

*Alternative considered:* leave it `false` and rely on "a wrong
signature is refused" as the real protection. That defends against a
tampered CDN and not against the attacker the design names — anyone who
can publish a release can also publish a manifest with the signature
field simply absent.

*Not affected:* every build older than v0.17.2 compiles its own copy of
the constant as `false` and goes on accepting what it always did. The
flip protects new installs forward, and strands nobody.

---

## 2026-08-16 — On Linux the icon comes from a third file, so we write one

The Windows fix below closed one platform and immediately raised the
question for the other two. macOS turned out to be clean: the DMG's
bundle carries `Resources/AppIcon.icns`, `CFBundleIconFile` names it,
and `LSUIElement` keeps the app out of the Dock and the app switcher
on purpose — the surfaces that showed a placeholder on Windows do not
exist there. Linux was not clean, in two separate ways, and the icon
*shipping* was never the problem: the AppImage has carried
`usr/share/icons/hicolor/256x256/apps/poltertype.png` all along.

**The window belonged to no application.** `iced` passes its
`application_id` to winit unconditionally, and we never set one, so
what it passed was `Some("")`. On Wayland that is an empty `app_id`;
on X11 it is an empty `WM_CLASS`, and worse than leaving it out —
winit's fallback to `argv[0]` runs only when nothing is passed at all.
Measured on Hyprland before the fix, `hyprctl clients` reported
`class: ""`.

**And on Wayland an app id is the only route an icon has.** winit's
Wayland backend implements `set_window_icon` as an empty function,
because the protocol has no window icon: the compositor looks the app
up by id in the installed `.desktop` entries instead. So the icon
`window_icon()` builds — the fix from the entry below — reaches
Windows and X11 and is dropped on the floor on Wayland.

**Decision:** `poltertype-shell` gains both halves.
`window_platform_specific()` fills in the one field the binary cannot
name itself (the struct behind `PlatformSpecific` is a different type
per platform, and `poltertype-app` holds no `#[cfg]`), and
`install_desktop_entry()` writes an entry and the mark into
`$XDG_DATA_HOME` at startup — after checking `$XDG_DATA_DIRS`, so a
distribution package that already installed one keeps ownership. The
entry carries an `X-PolterType-Version` stamp, which is what makes an
ordinary launch a single read-and-compare and an upgrade a rewrite.

**Why not a setting, when autostart is one.** Autostart changes what
the machine does at login; this only answers a question the desktop is
already asking about a window that already exists. A toggle for "may
PolterType tell your desktop what PolterType looks like" is a config
key nobody would ever have a reason to turn off.

*Alternative considered:* set the app id and stop there, leaving the
entry to packagers. That fixes X11 fully and Wayland only for people
who installed from the AUR or ran an AppImage integrator — which is
not how the download button on the site behaves.

*Correction to the entry below:* it states that `poltertype-icon` is a
build-dependency with nothing linked into the shipped binary. That
stopped being true in the same commit, which gave the Settings window
a runtime-rendered icon, and is now doubly untrue.

---

## 2026-08-15 — The executable carries its own icon, drawn at build time

A user installed the MSI and found the Start-menu entry wearing the
shell's placeholder. Measured on the installed binary: **zero** icon
resources and **no `VERSIONINFO` block at all** — empty FileVersion,
empty ProductName, blank Details tab.

The cause is that we only ever produced an icon for the *installer*.
`ARPPRODUCTICON` in the WiX template covers Add/Remove Programs and
nothing else; the Start-menu shortcut is authored non-advertised with
an empty `IconLocation`, which means "ask the target file" — and the
target had nothing to give. Every surface that reads the file rather
than the install (Explorer, Alt-Tab, the taskbar, the pinned entry)
was in the same position.

**Decision:** embed the icon in `poltertype.exe`. `poltertype-app`
gains a build script that renders an `.ico` into `OUT_DIR` and hands
it to `rc.exe` via `winresource`, together with a `VERSIONINFO` block
naming the product rather than the crate. The shortcut then inherits
it with no change to the WiX template at all — the fix is upstream of
the installer, which is why the installer does not mention it.

**The mark is rendered, not checked in.** The geometry moved out of
`xtask` into a new `poltertype-icon` crate so the build script and the
installers draw from one source. This keeps the repo's
no-binary-assets property and, more usefully, makes it impossible for
the exe's icon and the installers' icon to drift apart — which is a
thing that happens the first time someone regenerates one of them.
The crate is a **build**-dependency: nothing in it is linked into the
shipped binary.

*Alternative considered:* check an `.ico` into the repo and point the
build script at it. Rejected for the drift, not for the bytes — a
checked-in icon is a third copy of a mark that already exists twice
(here and in the site's `favicon.svg`), and nothing would ever tell us
the three had diverged.

**Two traps, both silent.** `.ico` entries below 256 px are raw DIBs
whose header declares *twice* the real height and whose rows run
bottom-up, and they still need an AND mask even though the alpha
channel makes it redundant. Get any of that wrong and the file parses,
the sizes look right, and the shell quietly declines to draw it —
which is indistinguishable from the bug being fixed. Hence
`ico/tests.rs` reading the bytes back the way Windows does. The build
script's platform test is `CARGO_CFG_TARGET_OS`, not `cfg!`: a build
script runs on the machine doing the building, and the question is
about the machine that will run the binary.

Dropping ImageMagick from release CI came free with it — every size is
now rasterised from the vector mark instead of box-filtered down from
one 1024 px master.

---

## 2026-08-08 — Windows keymaps are read from the OS, not declared in a TOML

Closing #20. A Windows layout is identified by its language, so all
three of Windows' genuinely different Bulgarian keyboards arrive as
`bg-BG`, and `bg_bg.toml` can only describe one of them. Measured
through the shipping code path: the bundled table matches Phonetic
Traditional exactly, differs from Typewriter on 7 keys, and from
Phonetic on **45 of 48**. Nothing errored — the wrong answer was a
perfectly well-formed mapping that belonged to somebody else's
keyboard.

**Decision:** ask the OS. `LayoutSwitcher::describe_keymaps()` is a
new extension point, defaulting to an empty list; the Windows backend
implements it with `MapVirtualKeyExW` + `ToUnicodeEx` over each active
`HKL`, and `LayoutDb` lays the result over the bundled tables.

*Alternative considered — and it is the one the issue itself
proposed:* an optional `windows_klid` in each TOML, matched against
the high word of the `HKL`. Rejected. It needs a hand-authored file
per variant per language, it only ever fixes variants somebody
remembered to describe, and it leaves custom and third-party layouts
exactly as broken as before. Querying the OS fixes all of them at
once and adds no data to maintain. The audit that produced the numbers
above is also the argument that it is safe: asking Windows reproduces
the hand-curated `bg_bg.toml` key for key, so the mechanism was
validated against known-good data before it was trusted with anything.

**Replace, not merge.** `describe_keymaps` promises a *complete*
table for a fixed block of 48 scancodes, so a scancode asked about and
missing from the answer is evidence that the key produces nothing —
not evidence that we failed to ask. Merging would preserve exactly the
stale rows the whole exercise exists to remove. A floor of 30 keys
guards against a query that went wrong rather than a keyboard that is
unusual.

**Precedence: bundled ← plug-ins ← OS ← user.** The OS outranks
anything we shipped because it is describing the machine rather than
guessing at it. A user TOML still outranks the OS: it is an explicit
statement of intent, and it is the escape hatch if this ever reads a
keyboard wrong.

**Two `ToUnicodeEx` traps, both already paid for once.** It answers
`-1` for a dead key *and still writes the character*, so reading that
as "produces nothing" loses every accented key — that mistake was 22
of the 35 apparent differences in the original audit. And it mutates
keyboard state, so a pending dead key would compose itself into
whatever the user types next; we pass the "don't change state" flag
and drain by hand anyway, because older builds ignore the flag.

**What this does not fix.** Two keyboards for one language still
collapse to one `LayoutId` and only one table can be held — we keep
the one currently in effect, else the first the OS lists, which is the
one Windows activates by default for that language. A user who keeps
two keyboards for a language *and* types on the second one gets the
first one's table. Fixing it properly means making `LayoutId` carry
the variant, which reaches config, the UI, dictionary stems and
`switch_to`; the user TOML override is the answer until somebody
actually hits it.

---

## 2026-08-08 — Undoing a correction teaches the dictionary; the tooltip covers a word's other forms

Three separate holes in "teach PolterType this word", found while
looking into a user's report that "Add to dictionary" felt like it
kept asking about the same words.

**The button itself worked.** The evidence said so: every add reached
both the overlay file and the running dictionary, and the file held no
duplicates. What the user was actually hitting was the two gaps below.

**`word_whitelist` did nothing about corrections.** Documented as
"words that should never be auto-corrected", read only by
`maybe_offer_suggestions`. It silenced the tooltip and let the
correction go ahead — a setting that was inert in exactly the case it
names. Now the first pre-decision filter, ahead of the heuristics:
everything else in that block infers intent from shape and context,
this one *is* the intent, so it goes first.

**The correction path had no route into the dictionary.** "Add to
dictionary" is a tooltip row, and the tooltip is only offered for
words the engine keeps (`decide`'s `KeepCurrent` branch). For a word
it corrected, there was no affordance at all — and those are the
expensive ones. The gesture users already reach for is the manual
switch-last hotkey, which after one of our corrections was a no-op
worth nothing: it re-applied the same correction, deleting the word
and retyping it identically. It now undoes instead, and treats the
undo as the teaching signal it plainly is.

*Alternative considered:* a tooltip after every correction with an
"undo + add" row. Rejected — the app corrects constantly, that is the
job, and a tooltip per correction is the noise the tooltip's own
design rules already refuse ("a tooltip whose ONLY content is add to
dictionary would itself be the noise it exists to stop").

*Why it announces itself:* `DictionaryAddOrigin` splits the explicit
route from the implicit one. Clicking a row labelled "Add to
dictionary" is its own confirmation; an undo that also changes what
the engine will do forever is not, so that one gets a notification. A
dictionary growing behind the user's back is how "why did it stop
correcting this?" starts.

**Inflections cost one prompt each.** The overlay is exact-form, so in
Ukrainian one piece of jargon is `деплой`, `деплою`, `деплоїмо`,
`деплоїти` — four trips through the tooltip for one decision the user
already made. Replayed against the reporting user's real wordlist, 11
of 75 entries were forms of a word already in it.

`shares_inflection_stem` is a shape rule — five characters of shared
opening, at most four of ending on either side — not a stemmer. A
stemmer per language is a data set of its own, and this has to work
for whatever languages a user adds. The floor is set by the closest
false pair we could find: `реалм` and `реальний` share exactly four
characters and nothing else.

*Scope, deliberately:* the suggestion path only. A lenient match in
`DictionaryDetector::judge` would suppress corrections for words the
user never taught us — a correction silently not happening, which is
far worse than one extra tooltip. Being wrong in the suggestion
direction costs a suggestion nobody asked for.

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
that keeps the macOS caveats explicit.

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
the known-gaps list) and had been wrong since it was
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
abstracts it. The platform-island list is now five crates, not four.

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
default build never opened a socket, and both the project rules and the
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

The project rules said platform code lives in `poltertype-input` and
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

## 2026-08-08 — Input state is reconciled against the device, not inferred from edges

The X11 backend tracked modifiers by watching press and release edges,
because XInput2 raw events carry no modifier state of their own. That
is correct only while every edge arrives, and it turns out edges go
missing routinely: any client holding an active keyboard grab stops
raw events reaching everyone else for as long as it holds one.
Measured on X.org against a client selecting exactly what this backend
selects — three key taps produced nine raw events with no grab and
zero during one, with no error, no disconnect and nothing in any log.

The consequence was disproportionate to the cause. One swallowed Alt
release left the modifier latched forever; `Modifiers::is_command()`
was then true for every keystroke, the engine read each one as a
shortcut, and the word buffer was abandoned every time. The app stayed
alive, kept receiving keys, and discarded all of them until it was
restarted — the same silent-wrong-state failure shape as
[#26](https://github.com/Just-Code-NET/PolterType/issues/26) itself,
which is how it was found.

The rule from here: **where the OS can be asked for the current state
of an input device, ask it, and treat our edge-derived view as a cache
to be reconciled — not as the truth.** `XQueryKeymap` answers from the
server's own device state rather than from event delivery, and keeps
working through a foreign grab (measured the same way). The listener
reconciles against it on idle rounds, gated on believing some modifier
is held, so an idle keyboard issues no round-trips at all.

Two things this rejects. **Not** a periodic self-grab to detect the
condition: `XGrabKeyboard` returning `AlreadyGrabbed` would name the
culprit precisely, but a grab attempt on a cadence steals whatever
keystroke lands in its window, which is a worse bug than the one being
diagnosed. And **not** treating this as somebody else's problem
because the missing release comes from another client's grab: a
desktop taking a grab to service its own keybinding is normal, ours is
the code that assumed it would see every edge.

The same assumption lives in the evdev backend
(`wayland::update_modifiers`), where `EVIOCGKEY` is the equivalent
question. Nothing has reported it — an `EVIOCGRAB` by another process
takes the whole device rather than swallowing one edge — so it is
noted here rather than fixed on spec.

## 2026-08-10 — An ad-hoc signature is worth shipping before a real one

macOS release builds carried a signature on their arm64 slice and none
on their x86_64 slice, for the whole life of the universal DMG. Nobody
stripped anything: arm64 macOS refuses to execute unsigned code, so the
linker ad-hoc-signs that slice as a matter of course; the
cross-compiled x86_64 slice is under no such rule and gets nothing;
`lipo` merges the two verbatim. The release never ran `codesign` at
all, and the missing half was invisible in every artifact anyone
looked at — the DMG mounted, the app launched, `--version` answered.

On Intel, where macOS runs the x86_64 slice, the cost was the whole
product. Accessibility is granted to a *code identity*, and unsigned
code has none, so the event tap attached, reported success and
received nothing: permission toggled on, listener healthy in the log,
not one correction ever made
([#28](https://github.com/Just-Code-NET/PolterType/issues/28)).

The decision is to ad-hoc sign now rather than wait for the Developer
ID that `docs/CODE_SIGNING.md` has been deferring since v0.10.0. An
ad-hoc signature identifies nobody and buys no Gatekeeper trust — the
right-click → Open dance is unchanged, and it would be easy to file
this as security theatre. It is not: the signature is what gives the
app an identity for TCC to attach a permission to, and without one the
app does not work on that hardware at all. Correctness first,
provenance when someone buys the certificate.

Its price is real and belongs in the docs rather than in a footnote:
an ad-hoc designated requirement pins the code hash, so every update
is different software to macOS and the user must grant Accessibility
again. A Developer ID pins the certificate instead and would carry the
grant across updates. That is now the strongest argument for buying
one — stronger than the Gatekeeper warning, which users only meet
once.

What the fix mostly is, though, is the check rather than the signature.
`codesign --verify` passes happily on a bundle whose fat binary has an
unsigned slice, which is precisely how this shipped unnoticed through
several releases, so the build now asserts **each architecture by
name** and fails the release if either comes back unsigned. A packaging
step that can silently produce a half-broken artifact needs an
assertion that names the half.

---

## 2026-08-11 — The macOS caret answer is validated before it is trusted

The macOS suggestion tooltip landed with a focus tracker
([#29](https://github.com/Just-Code-NET/PolterType/pull/29), outside
contributor, Intel hardware), and the interesting part is not the
`NSPanel` — it is that the Accessibility API answers the caret
question wrongly, confidently, in the applications people type in
most.

`AXBoundsForRange` and the marker-range pair return real, thin caret
rectangles in TextEdit and native fields. Chrome — omnibox and web
inputs alike — and Terminal return a zero-size rectangle at the web
area's origin, or a point past the bottom edge of the window. Taken at
face value that anchors the tooltip to a place the text is not, which
reads to a user as the tooltip following the *previous* field: a bug
that looks like flakiness rather than like a wrong answer.

So the tracker judges what it gets. A caret is believable when its
numbers are finite, it is between half a point and 120 points tall, it
is no wider than a sliver, and it intersects the focused element's own
frame with 24 points of slack — real carets do stick out of their
field by a few points, junk ones miss by hundreds. Anything else falls
back to the focused element's frame when that element is a text
widget, which is exact for the omnibox and field-accurate for web
inputs. Two related choices: the app element is built from
`NSWorkspace`'s frontmost pid rather than from
`AXUIElementCreateSystemWide`, whose `kAXFocusedApplication` answered
`cannotComplete` essentially always on the test machine, and every
query carries a 0.3 s messaging timeout because this runs on the event
loop and a hung target app must not take the tray with it.

`AXManualAccessibility` / `AXEnhancedUserInterface` would make Chrome
compute real carets. They were considered and refused: a typing
utility does not get to mutate another process's global accessibility
state to make its own tooltip prettier.

The general shape is worth keeping. Where a platform API is *allowed*
to answer badly and no error accompanies the bad answer, validate the
answer against what a correct one would look like, and degrade to a
coarser source. The alternative — trusting it and shipping — produces
exactly the class of bug nobody can reproduce on their own machine.

Two smaller findings came out of the same work and are recorded here
because both look like nothing in a diff. macOS `CGEvent` posts made
back-to-back coalesce inside the receiving app's run-loop turn, which
dropped or duplicated a keystroke at the delete/replay seam of a
correction; the emitter now paces a burst at 2 ms per event, matching
the X11 emitter. And `cosmic-text`/`fontdb` log the text they are
shaping at debug level — the tooltip shapes the user's words, so those
two targets are now capped at `warn` regardless of `RUST_LOG`. That
one was a live leak on every platform, not a macOS detail: the rule is
that typed text never reaches a log, and a dependency's logger is
still a log.

## 2026-08-11 — `view` is rebuilt per state change, not per frame

A report that the Settings window "lags badly when scrolling" was
diagnosed twice, and the first diagnosis was wrong in a way worth
recording, because the wrong answer was written down in our own source
comments and read back as fact.

Several comments asserted that `view` runs on every frame. On that
belief, `PluginPane::in_array` looked catastrophic: it read the
plug-in's whole config file and ran a format-preserving `toml_edit`
parse **per list row**, so a chat plug-in showing two room lists of 34
conversations would do 68 of those per frame. Measured, one such call
costs 78 µs against a 17 KB config.

The belief was false. iced rebuilds the widget tree on a *state
change* — a message from a click, a keystroke, a completed command —
and a `Scrollable` with no `on_scroll` handler produces no message, so
scrolling rebuilds nothing. A counter in `view` logged **one** call
across a session containing a nav click, a room-list load and 150
wheel events, and `/proc/<pid>/io` confirmed the scroll path reads
zero bytes.

Two separate things came out of that, and they are separate on
purpose:

* **The per-row re-read was still a real defect**, just on a different
  axis. It cost 1.2 MB read and 68 TOML parses on *every click*,
  against a file the click itself had just written. The membership and
  the parsed rows are now held on the pane and refreshed where the
  file can actually have changed — on load, on every write this pane
  makes, and on reaching a section. Another program owns that file, so
  the refresh still reads from disk; what is gone is doing it per row.
  Measured: 1216 KB per interaction before, 34 KB after.
* **The scrolling itself was the CPU renderer, and the dev profile
  owned it.** The window has no GPU path — every frame is rasterised
  by `tiny-skia` and shaped by `cosmic-text` — and unoptimised those
  two are the entire frame budget. The same scroll costs 97% of a core
  in a stock debug build and 20% in release, so `[profile.dev.package
  ."*"] opt-level = 3` is now set: dependencies are optimised, our own
  crates are not, and stepping still works where the bugs are.

The general lesson is the one about the comments. A performance claim
in a comment is a measurement someone did not repeat, and it decays
like any other doc — except that nobody re-checks it, because it reads
as an explanation rather than as a fact. State the *shape* of the cost
in a comment and put the number next to the measurement that produced
it.

## 2026-08-13 — A plug-in supplies rows; the menu and the icon stay ours

The tray could show a plug-in's settings and its state. It could not
show its *work* — a queue, an inbox, the things that arrived while
nobody was looking — because a manifest written months earlier cannot
name them.

So a manifest may now declare a list whose rows the plug-in prints
while the menu is being opened. The obvious alternative was to let a
plug-in hand over menu markup, and that is exactly what the whole
plug-in design refuses on the pane side: a third party that can draw
can imitate PolterType's own dialogs, another plug-in, or a system
prompt. The split is the same one storey down — the plug-in supplies
*text*, PolterType supplies the menu, the separators and the layout.

Two boundaries fall out of it, and both are the point rather than
implementation detail:

* **Actions come from the manifest, never from the row.** What a row
  can do was decided before that row existed. `{id}` is substituted as
  a whole argument, not into one, so a row's own text cannot become a
  second flag — the same reasoning that keeps smart commands from
  substituting typed text into `run_shell`.
* **The icon is shared, and it stays ours.** A plug-in names the state
  key that counts and a value above zero raises a mark; it never
  replaces the icon, draws on it or picks what it looks like. The mark
  went to the top-right because the bottom-right is the pause
  indicator, and a paused PolterType with work waiting has to be able
  to say both at once. Rendered at 16 px the mark's ring sat on the
  second glyph and a layout code stopped being readable, so the glyphs
  moved down a row — the icon's first job is to name the layout, and
  the badge is a guest on it.

## 2026-08-14 — A word means something only against the layout it was typed in

Two bugs arrived in one report: type a word under en-US, switch the
layout by hand, close the word with `,` — and PolterType retyped the
whole word and put `?` where the comma had been.

The comma half is narrow and was simply wrong. A correction replays
the word as scancodes so it re-reads under the new layout, and the key
that *closed* the word went out the same way and picked up the new
layout's glyph too: `Shift`+`0x35` is `,` in uk-UA and `?` in en-US.
Re-reading the word is the correction; re-reading its separator is a
second edit nobody asked for. The boundary is now resolved by
character against the target layout and replayed on whichever key
produces it there. Where the target cannot produce that character at
all the key goes out as typed — a wrong separator is a worse outcome
than an uncorrected word only if you have never seen the alternative,
which is refusing to fix the word.

The other half is the one worth defending. The word buffer holds
**scancodes**, deliberately: that is what makes a correction work in
terminals and Wayland-native apps without a compose dance. But it also
means a buffer is meaningless on its own — it renders into text only
against a layout, and `decide` was reading it against whatever was
active when the word *ended*. Switch by hand in between and the engine
reads perfectly good English as Cyrillic gibberish, "fixes" it, and
drags the layout back off the one the user had just chosen. The user's
own switch is the strongest possible statement of intent, and the
engine was overruling it with a guess.

Every word is therefore stamped with the layout in effect at its first
key, and a word that ends under a different one skips the automatic
path entirely — no correction, no suggestion offer. The stash the
manual switch-last hotkey reads is still written, and now rendered
under the layout the word was typed in rather than the one that
happens to be current, so the deliberate gesture still works on
exactly the words the automatic one now leaves alone.

The stamp costs one `current()` query per word, which on Linux is
served by the 200 ms TTL cache in front of every backend. That cache
is also the limit of the fix: a switch made inside 200 ms of the
previous word's boundary can be sampled stale, and the engine then
either misses a real change or invents one. Physically that means
pressing the layout chord and the next letter almost together, which
is rare — and the honest alternative, an uncached query per word,
spawns a `hyprctl` per word for a case nobody has hit.

---

## 2026-08-20 — A compound is judged segment by segment, and comparatively

`cqrs-client`, typed correctly under en-US with ru-RU loaded, was
replaced with `сйкы-сдшуте`. Read as one string the token is
indefensible: six consonants in a row, a 0.20 vowel ratio, 0.00 en-US
fit against 0.75 for the Cyrillic reading. Read as
`cqrs` + `client` it is obvious — the second half is a plain English
word scoring a perfect 1.00, and only the acronym welded to its front
made the whole thing look like noise.

The 2026-08-07 entry below already established this shape for dots
(a hostname is only as plausible as its worst segment). Hyphens are the
same structure and were not covered, and neither treatment helps here
anyway: taking the *worst* segment makes `cqrs-client` look worse, not
better. What was missing is that a compound is a token the wrong-layout
hypothesis has to explain **piece by piece**.

**Decision:** both detectors split on `-` and `.` and compare position
against position. The dictionary keeps the token when a segment is a
real word here that no alternate explains; plausibility keeps it when a
segment reads *better* here than any alternate would make it.

**The comparison is the whole decision, and the first version did not
have it.** "Some segment reads well under the current layout" passed
every unit test in `poltertype-detect` and then vetoed **a fifth of a
real Russian corpus**: `по-нашему` renders as `gj-yfitve`, and `yfitve`
scores a flawless 1.00 under en-US — exactly as `нашему` does under
ru-RU. A segment that reads equally well either way is evidence of
nothing. Two further rules fell out of the same corpus:

* **Better than *every* alternate, not just the winner.** Scoring only
  against the best-fitting candidate picks whichever layout happens to
  read that segment worst; with all fifteen bundled layouts loaded,
  `куда-то` lost to a bg-BG reading while ru-RU explained it perfectly.
* **A layout that renders the segment identically is not a rival.**
  Switching to it would leave the text exactly as typed. es-ES and
  de-DE reproduce most Latin tokens character for character, and
  without this every one of them counted as an alternative reading of
  `client`.

Segments below three letters never speak for their token, and one
carrying stray punctuation never does either — `будь-що` renders as
`,elm-oj`, whose `oj` is a perfect English fit and whose `elm` is a
real English word, both by pure coincidence.

**Alternative considered and rejected:** treat any hyphen as code
punctuation in `looks_like_code_token`. One line, and it would have
fixed the report — at the cost of every hyphenated word in every
language the app exists to correct. `по-перше`, `будь-ласка`,
`все-таки`, `интернет-магазин` are not identifiers.

**What guards this.** `poltertype-detect`'s own tests run on toy
profiles and hand-built FSTs, which is precisely why they missed both
failure modes. The corpus in
`crates/poltertype-core/tests/compound_corpus.rs` runs identifiers and
wrong-layout compounds through the real 370k/1.4M-entry dictionaries
and the real vowel profiles, in both directions. Any change to the
guard answers to it.

---

## 2026-08-20 — KDE addresses layouts by index, and has since Plasma 5.23

[#31](https://github.com/Just-Code-NET/PolterType/issues/31) arrived as
an AppImage crash on CachyOS and carried a second bug in its log:

```
INFO OS active layouts active=[LayoutId("qdbus: I don't know how to
display an argument of type 'a(sss)', run with --literal.")] count=1
```

Every bundled layout was then skipped as "not in the active OS list"
and the engine came up with `layouts=0` — running, and structurally
incapable of correcting anything.

Two failures compounded. `getLayoutsList` returns `a(sss)`, which plain
`qdbus` cannot render: it prints that sentence **to stdout and exits
0**, so an exit-status check reads it as a successful answer and the
sentence becomes a layout id. And the backend was written against an
interface that has not existed since Plasma 5.23 (2021): KWin's
`KeyboardLayoutDBusInterface` (`src/keyboard_layout.h`) declares
`getLayout() -> uint` and `setLayout(uint) -> bool`, addressing layouts
by **position in the configured list**, not by xkb short name. We were
passing `"us"` to a method that takes an integer.

**Decision:** call `getLayoutsList` with `--literal` and parse Qt's
`argumentToString` output (`qtbase/src/dbus/qdbusutil.cpp` — the format
is `[Argument: a(sss) {[Argument: (sss) "us", "", "English (US)"], …}]`,
strings unescaped, short name first); resolve `getLayout`'s index
against that list, and map a target back to its index for `setLayout`.
Which API is in play is probed once at init from the shape of
`getLayout`'s answer, since xkb short names are never numeric — guessing
wrong does not fail loudly, it switches to the wrong layout.

**The probe now demands an answer, not an exit code.** `try_init`
required only that `list_active()` returned `Ok`, which the error
sentence satisfied. It now requires a non-empty *parsed* list, so a
backend that cannot be understood falls through to the next one instead
of poisoning the engine with a layout that does not exist. That is the
same lesson as the 2026-08-07 entry on probing by what a desktop
*does* — this time the reachability check was not merely weak, it was
being answered by an error message.

**Not fixed by moving to zbus,** which is already in the tree under
`poltertype-input`. The CLI shell-out is a deliberate choice for these
backends (see the crate's module doc) and the bug was never in the
transport — it was in reading an interface we had not checked against
its source. Verified against KWin master and Qt's own printer;
**unverified on a running Plasma session**, because nobody here has
one. `docs/KNOWN-GAPS.md` says so.

---

## 2026-08-20 — Ask for the tray library before it aborts the process

The same report's crash: `libappindicator-sys` `dlopen`s the tray
library on first use and **panics** when no soname resolves. Release
builds are `panic = "abort"`, so a KDE box without
`libayatana-appindicator` installed — the default on Arch — met a
SIGABRT and a four-line dlopen dump in the system language, naming four
`.so` files and no package. `catch_unwind` cannot help across an abort,
and there is no feature to flip: `tray-icon` 0.24, five versions ahead
of ours, loads the same object the same way.

**Decision:** `poltertype-tray` opens the same sonames itself before
the `TrayIcon` is built, and on failure the app exits with the package
name for each of the four common distro families. A resolved handle is
deliberately leaked — the tray is about to load the same object anyway,
and `dlclose`ing a GTK-linked library only to reopen it is the riskier
half of the trade.

**And the AppImage stops needing it.** Because the load is a `dlopen`
by soname, the library is not in the binary's `DT_NEEDED`, so
linuxdeploy's dependency walk never saw it and every AppImage we have
ever shipped went out without it. `--library` names it explicitly and
deploys its own dependencies too; the build script then asserts the
file is in the AppDir, because a silently un-deployed library is a
failure that only appears on a user's machine.

**Not chosen: run without a tray.** The engine would still correct
text, but PolterType puts its entire UI in the tray — no Settings, no
pause, no quit. A daemon the user cannot see or stop is a worse answer
than a sentence telling them what to install.

---

## 2026-08-20 — A permission error must name its own cause

The same reporter, one release later: PolterType told them
`no readable keyboard devices in /dev/input/* — run
scripts/setup-linux.sh to grant access`. They had run it, twice, with a
re-login in between. The sentence was every Wayland read failure's only
output, and the script it names is a no-op for four of the five things
that produce it — a session that predates its own group membership, a
udev rule that never reached the existing nodes, a container with no
input devices, and a device set with no keyboard in it.

Worse, the app knew better and did not say so. `setup::probe_setup`
already distinguished "not set up" from "set up, but this session
predates it" for the Settings pane; the listener that actually fails
never consulted it.

**Decision:** the scan reports facts — nodes present, nodes opened,
keyboards among them, and the errno plus `uid`/`gid`/`mode` of the
first refusal — and one pure function turns those into the sentence.
Both the listener and the Setup pane read the group state from the same
place, so they cannot tell different stories. The facts are printed
too: a user's log paste is now the diagnosis rather than the start of
one.

**`evdev::enumerate()` had to go** for this. It swallows every open
error, which makes a total permission failure and a machine with no
keyboard the same empty vector — and telling those apart is the whole
point.

**A mouse alone is now a failure.** Opening only pointer devices used
to count as a successful start: clicks tell the engine to forget its
buffer and nothing else, so the app ran and could never correct
anything, silently.

**The script verifies itself.** `setup-linux.sh` re-reads the group
database and `stat`s every node afterwards, and exits non-zero instead
of printing "Done". It also catches `sudo -i`, where `root` is what
gets added to the `input` group. A setup script that cannot fail is
indistinguishable from one that did not run.

**Not chosen: a `--doctor` flag.** The Setup pane already exists for
this and the error string already reaches the tray alert; a third
surface would be a third thing to keep in step.

**Not chosen: acquiring the permission ourselves.** Unchanged from
0.7.0 — the pane copies a command rather than running `sudo`, and an
app that quietly acquires root has spent trust it will not get back.
What did change is *which* command: the AppImage now carries the script
at `usr/share/poltertype/scripts/`, and the command names the copy the
running binary has. Naming a repository path to someone who downloaded
one self-contained file was the same defect as the sentence above — a
correct instruction the reader cannot follow.

---

## 2026-08-20 — A substituted hotkey has to be visible where the user looks

The third bug in issue #31, and the same shape as the second: the app
knew something and told the log instead of the user. On the
Wayland/evdev backend PolterType substitutes `Ctrl+Shift+F9` for the
default `Ctrl+Shift+Backspace` — the chord is observed, not consumed,
so the Backspace also reaches the focused app and `Ctrl+Backspace`
deletes the word being corrected. The Settings window went on showing
`Ctrl+Shift+Backspace`. The reporter pressed it, nothing happened, and
he filed "I cannot force switch last word" with a screenshot of the
pane next to a log line reading `rebound_to="Ctrl+Shift+F9"`.

The macOS pause substitution had the identical hole, unreported only
because nobody here runs macOS.

**Decision:** one resolver, `hotkeys::effective_*`, called by both the
tray and the Settings window. The tray builds its `HotkeyEnvironment`
from the backends it actually started; the Settings window is a
separate process with neither, so `poltertype_input::hotkey_environment`
answers the same two questions from the session itself. The pane prints
the substitution under the row: which chord was replaced, and why.

**Phrased as facts, not backend names.** `observed_not_consumed` and
`system_owns_ctrl_shift_space` say why a chord is unusable here; a
caller comparing against `"linux-wayland-evdev"` would have to know
what that implies, and the two places that knew it disagreed.

**Not chosen: writing the substitute into `config.toml`.** It would
make the two agree trivially and break the rule that earns the
substitution in the first place — one config file means the same thing
on every machine. The substitution stays a runtime decision, applied
only while the user is on the default.

**Not chosen: passing the effective chords to the child process.**
Exact for the tray's own Settings button and wrong for
`poltertype --settings` started by hand, which is how a user checks
what a key is bound to.

**Also fixed: the pane's own header.** It said hotkeys "are registered
with the OS at startup" directly above the two rows the OS never sees
on this backend.

## 2026-08-21 — A word the user can still see outlives the buffer that typed it

The manual switch-last hotkey did nothing. Not on one machine, not in
one app: for anybody who took longer than two seconds to press it,
which is everybody.

`[engine].idle_timeout_ms` abandons the word being typed after a pause,
because the caret can be anywhere by then. It was clearing the *stash*
of the last completed word with it — and the first key event after the
pause is the chord's own `Ctrl`. So the sequence the hotkey exists for
(type, notice the layout was wrong, reach for the chord) cleared the
stash on its way to reading it.

**Decision:** the stash gets its own window, `LAST_WORD_TTL`, 60 s,
independent of the buffer's. Not a relaxation of the caret rules — a
click, a nav key, deleting text we never saw and the next completed
word each still drop it through their own path. Time alone is the only
thing that behaves differently, and it is the only one of them that
does not mean the word moved.

**Why 60 s and not "until something invalidates it":** what the idle
clear was really buying is that a machine left alone stops holding a
word in RAM. That is worth keeping; two seconds is simply not the
number for it.

**Not chosen: making the hotkey work on the word still being typed.**
It is the obvious next question — press the chord mid-word and nothing
happens — and the answer is not a flag. `force_switch_last` replays a
boundary key after the corrected word; a word with no boundary needs
that path to grow a second shape, inside the one function where a
mistake corrupts the user's text rather than failing to fix it. Its own
change, with its own testing.

## 2026-08-21 — Autostart is a session mechanism, so use the session's

"Run at login" wrote `~/.config/autostart/<id>.desktop` and reported
success. On GNOME, KDE and Xfce something reads that directory. On a
bare Hyprland, Sway or river session nothing does, so the toggle was a
setting that lied — and those are exactly the sessions this app is
most used on.

Where systemd's `xdg-desktop-autostart.target` bridges the gap it is
the wrong shape as well: it fires as early as the user manager can
reach it, which on a compositor that publishes its environment from its
own config is *before* it has. That is not theory — it is how this
project's own machine failed. The unit ran, the app probed seven layout
backends, found none, and exited 1.

**Decision:** a systemd **user service** wanted by
`graphical-session.target`. That target means "the session is up and
its environment is published", which is precisely the precondition
PolterType has. `PartOf` the same target, so logging out stops it.

**The XDG entry stays as the fallback** for a machine with no user
manager, and is removed when the unit goes in: two mechanisms start two
copies, and the second loses to the instance lock with a log line that
reads like a fault.

**What this does not solve.** A bare compositor still has to reach
`graphical-session.target` once — a session target of its own, started
from the compositor config after the environment is published. We
cannot write that safely into somebody's `hyprland.conf`, so we say it:
the app logs a warning when it installs the unit into a session that
never reaches the target, and `docs/PERMISSIONS.md` carries the recipe.
A toggle that quietly does nothing was the whole complaint; being told
is the minimum improvement.

**Not chosen: `WantedBy=default.target`.** It would run everywhere,
including a TTY or SSH login with no session at all, and — worse — with
an environment that has no `WAYLAND_DISPLAY` in it and never will, since
a process cannot inherit variables published after it started. Being
early is survivable; being started into nothing is not.

## 2026-08-21 — A session that is not ready yet is not an unsupported one

Two ways one login killed the app outright, both found chasing the
autostart bug above.

`global-hotkey`'s X11 backend opens a display on a thread of its own
and uses the handle without checking it: with none, its first act is
`XDefaultRootWindow(NULL)`. We built that manager unconditionally,
including on the evdev backend, which reads its chords off the key
stream and never registers anything with it. A Wayland session with no
Xwayland therefore died three log lines into startup, in a stack frame
with none of our code in it.

And a layout backend that could not be probed was an exit — so a
compositor that had not finished coming up cost the user the entire
session.

**Decision:** build the hotkey manager only on the path that registers
something, and only after waiting up to 15 s for an X display; run
without OS-level hotkeys past that. Keep running without a layout
backend too, behind `UnavailableSwitcher` and a **⚠ Layout switching
unavailable — Setup…** entry in the tray, which is the shape a missing
keyboard hook has had since 0.17.3.

**Why a wait and not a probe:** both failures are timing, not
capability. The same 15 s window covers a compositor socket that
appears a second late and an Xwayland that starts after us; a probe
answered at the wrong instant is just a faster way to be wrong.

**Not chosen: refusing to start without a tray, for consistency with
the missing-tray-library check.** That check is about a library that
will never appear; this one is about a session that has not finished
starting. Same symptom, opposite prognosis.

## 2026-08-21 — An undo is not always a claim, and an overlay is not always right

`Привіт`, typed correctly under uk-UA, came out as `Ghbdsn`. Two
separate things had to be true for that, and both were.

The user's own en-US overlay held `ghbdsn` — `привіт`'s en-US rendering.
It got there from the manual switch-last hotkey, which doubles as "you
were wrong, learn this word" and learned unconditionally. Undoing a
correction the engine got *right* — trying the gesture out, wanting the
other rendering once — therefore taught the engine the exact gibberish
it had just fixed. Four such entries had accumulated across two
languages; `tasks` → `ефілі` is the same thing pointing the other way.

`DictionaryDetector::judge` then made each one permanent. Its
overlay-priority sweep ran over the *alternate* layouts before the
current layout's own dictionary was consulted at all, so an overlay
entry on one side outranked a bundled-dictionary hit on the other — at
0.95 confidence against a 0.55 threshold, every single time.

**Decision:** an undo teaches only when it carries evidence — the target
layout already knows the word, so an overlay entry merely promotes it
past a `weak` flag, or the layout being undone *from* does not know what
was on screen, meaning the switch rested on word shape rather than on a
dictionary hit. And a clean dictionary word of the current layout is
checked before either alt sweep, the overlay one included.

**Why both halves.** Either alone leaves the failure reachable. The
guard cannot retire entries that older versions already wrote, and the
ordering cannot stop anyone hand-editing nonsense into their own
overlay. Together, a bad entry is inert and no new ones are minted.

**Not chosen: a plausibility floor on what may be learned.** It was the
first idea and it only works in one direction. `ghbdsn` scores near zero
as English — it has no vowels at all — but `ефілі` reads as an entirely
ordinary Ukrainian word by script, vowel ratio and consonant runs alike.
A rule that catches half the cases would have left the other half to be
rediscovered by a user.

**Not chosen: refusing to learn whenever the other side is a real
word.** That is the same test inverted, and it retires the case the
gesture exists for: uk-UA `туче` is valid but `weak`, loses to en-US
`next`, and teaching it is exactly how a user says "no, I meant туче".

## 2026-08-22 — A separator says what a token is from either side

`/tmp ` came back as `/еьз `. Nothing about the token was ambiguous to
a human: it is a path, and the slash says so.

The engine had the rule already — `is_structural_boundary` vetoes
auto-switching after `:` `/` `\` `@` `=` `#` `&`, which is what keeps
`http:` and `just-code.net/` intact. It only ever looked at the key
that *closed* the word, and a path segment closes with an ordinary
space. `tmp` therefore reached the detectors bare, where it is exactly
the shape the word-plausibility scorer is worst at: no vowels at all
under en-US (0.25), a textbook vowel ratio under uk-UA (`еьз`, 1.00).
The dictionary detector had no opinion — `tmp` is in no general-purpose
English word list — so shape decided, and shape was confidently wrong.

**Decision:** the word buffer remembers the separator a word *opened*
after, and the same structural test runs on it. One rule, both sides:
`/tmp`, `@nickname`, `C:\Users`, `--flag=value`.

The buffer is the only place that can answer this. By the time
`decide()` runs, its `boundary_run` has already been reset to the key
that closed the word, and the engine keeps no other model of what is
left of the caret. So `lead` travels with the word — cleared when the
caret moves, restored when a backspace re-opens the previous word,
because a separator we cannot still see is not evidence of anything.

**Not chosen: reading structural characters out of the rendered token
instead.** That is a different rule wearing the same clothes, and it
misfires in both directions. `ґанок` typed under en-US renders `\fyjr`
— `ґ` sits on the backslash key — so a token-wide test would refuse to
correct every Ukrainian word containing `ґ`. And whether the slash even
lands *inside* the token depends on which layouts are loaded: bg-BG
puts `б` on `0x35`, so with Bulgarian active `/` is a letter and `/tmp`
is one four-key token, while on an en-US + uk-UA machine it is a
separator and a word. A rule that changes shape with the user's
language list is not a rule.

**The other half was data.** A veto only helps where a separator
exists; `cd tmp` has none. Shell vocabulary is missing wholesale from
`dwyl/english-words` — `tmp`, `mkdir`, `stderr`, `rustc`, `systemctl`
— and it is mostly vowel-less, which is precisely what the scorer reads
as noise. Those entries now ship in `en_us-extras.txt`, each one
checked against every other bundled dictionary first: an entry whose
cross-layout twin *is* a real word is a permanent veto on correcting
that word, which is the same failure as a poisoned overlay arriving by
a respectable route. `src` (uk-UA `ікс`), `cfg` (`сап`) and `ptr`
(`зек`) failed that check and were left out.
