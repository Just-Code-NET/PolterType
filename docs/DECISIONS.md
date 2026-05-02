# Decision log

Short-form record of non-obvious technical choices made while
implementing kb-switcher. Each entry: **what** was decided, **why**,
and any **alternatives** considered.

---

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

For v0.1 the EN/UK mapping TOMLs live in `data/layout-mappings/` and
are baked into the binary at compile time. Runtime overrides from
`$XDG_CONFIG_HOME/kb-switcher/layout-mappings/` are a Phase 8+ task.

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
