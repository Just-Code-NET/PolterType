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
`<config-dir>/kb-switcher/wordlists/<stem>.txt` SHOULD be reloadable
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

Storage on disk: bulk wordlists ship as `data/wordlists/<id>.txt.gz`
rather than raw `.txt`. Raw, the six languages total ~165 MB
(uk_ua alone is 84 MB after expansion); gzipped they're ~24 MB.
Both `kb-core/build.rs` (`flate2::read::GzDecoder`) and the xtask
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

Regression test lives at `kb_detect::tests::plausibility_keeps_short_vcv_cyrillic_word`
and replays the exact 6-layout candidate set the engine produces.
The expander itself has eight unit tests under
`xtask::hunspell::tests` covering the SFX / PFX / class / negclass
/ FLAG-mode / continuation / unknown-directive / FLAG-num-rejection
shapes.


## 2026-05-07 — Data files externalised + lazy-loading by OS-active

Two structural problems with the v0.1 baked-in data approach:

1. **Wasteful RAM** — `include_bytes!` baked all six bundled FSTs
   into `kb-switcher.exe`. A user with `en-US / uk-UA / ru-RU`
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

* **`crates/kb-core/build.rs`** writes layout TOMLs, FSTs, and
  stop-word lists to `<workspace>/target/dist/data/` instead of
  embedding them. The workspace target dir is deduced from
  `OUT_DIR` (walks up to a `target` ancestor), which keeps
  `CARGO_TARGET_DIR` overrides working.
* **`crates/kb-core/src/data_dir.rs`** — new module that resolves
  the data directory at runtime. Order: `KB_SWITCHER_DATA_DIR` env
  → `<exe_dir>/data` (Windows MSI, AppImage AppDir) →
  `<exe_dir>/../Resources/data` (macOS .app) →
  `<exe_dir>/../share/kb-switcher/data` (FHS Linux) →
  `<workspace>/target/dist/data` (dev fallback). Unit-tested
  against synthesised exe paths so the per-platform shape is
  pinned.
* **`LayoutDb::load(LoadOptions { active_filter, … })`** — new
  loader that takes the OS-active layout list and skips bundled
  TOMLs whose `id` isn't in it. Pre-parsing the `id` line via the
  small `peek_layout_id` helper means we don't even read the FST
  for filtered-out languages.
* **`crates/kb-app`** queries `LayoutSwitcher::list_active()` at
  startup (right after building the switcher, before loading
  layouts) and feeds the result into `LoadOptions::active_filter`.
  Adding a language in the OS now needs a kb-switcher restart,
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
  `kb-switcher --settings` as a child process. The subprocess form
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

`crates/kb-app/src/main.rs` now reads `[hotkeys]` from settings
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
user-overlay text files in `<config-dir>/kb-switcher/wordlists/`.
Two files per layout, mirroring the loader contract documented in
`crates/kb-core/src/layouts.rs::build_dictionary`:

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
kb-switcher to apply" so the constraint is visible.

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
* **Architecture fit.** kb-switcher already runs a word-boundary
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
