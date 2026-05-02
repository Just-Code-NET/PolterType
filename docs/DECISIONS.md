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
in `kb-core::engine::buffer::tests`.

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
build time `kb-core/build.rs` reads `data/wordlists/<id>.txt` and
emits `<OUT_DIR>/<stem>.fst`. At runtime we `include_bytes!` the
blob and wrap in `fst::Set::new(&'static [u8])` — O(len) lookup,
no per-word allocation, lives in `.rodata`.

Concrete cost: release binary 5 → 6.85 MB (+1.85 MB for both FSTs);
~3 MB additional resident memory at runtime. 700k+ words for
~5 bytes per word storage cost — FST is the right tool.

User overlay path: drop a one-word-per-line text file at
`<config-dir>/kb-switcher/wordlists/<stem>.txt` to extend a
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

* **Per-app**: `[exceptions].disabled_apps` ships with a sensible
  default list — VS Code / Cursor, every JetBrains IDE, Sublime, Zed,
  Neovide, Windows Terminal, alacritty / kitty / wezterm,
  PowerShell / cmd, etc. The focus tracker (`kb-input::focus`) reads
  the foreground process executable and the engine matches case-
  insensitively. Match → skip auto-decision.
* **Per-token**: even outside the IDE list, the engine checks
  `looks_like_code_token(buffer)` from `kb-detect`. If the just-
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
