# Adding a language to PolterType

`poltertype` is data-driven: each supported keyboard layout is one
TOML file, and each language's dictionary is a plain wordlist. Neither
is compiled into the binary — both are read from disk at run time.
There are two paths to add a language: **bundled** (committed to
`data/`, shipped with the app, needs a PR and a rebuild) or **user**
(dropped into your config directory, no rebuild).

> **TL;DR** — for a one-off custom layout on your own machine, drop
> a `*.toml` into `<config-dir>/poltertype/layouts/` and a matching
> wordlist into `<config-dir>/poltertype/wordlists/`. To upstream
> a language, edit the bundled files and submit a PR.

---

## 1. Layouts (where each scancode maps to which character)

A layout TOML answers one question per row:
*"when the user presses physical key X under this layout, which
character does the OS produce?"*

The file lives at `data/layout-mappings/<stem>.toml` (bundled) or
`<config-dir>/poltertype/layouts/<stem>.toml` (user). The schema
is identical.

### Schema

```toml
id     = "pl-PL"          # BCP-47-style; unique
name   = "Polski"         # display label (optional)
script = "Latin"          # Latin / Cyrillic / Greek / Armenian / Hebrew / Arabic / Other

[keys]
# Win SC Set-1 scancodes → produced character.
# `plain` = unshifted, `shift` = shifted variant (optional).
0x10 = { plain = "q", shift = "Q" }
0x11 = { plain = "w", shift = "W" }
# ... fill in alphanumeric and punctuation rows
```

### Which scancodes matter

Cover at least the alphanumeric block plus the punctuation that
serves as a word boundary in your language. The fifteen bundled files
in `data/layout-mappings/` are good copy-paste templates — they all
list the same 46 scancodes (number row, three letter rows, and the
key left of `Z`), plus `0x56` where a layout puts a letter on the
extra 105-key board key, as Bulgarian does. The detector ignores
scancodes you don't list, so omitting `0x29` (backtick) for a layout
that doesn't use it is fine.

### Getting the mapping right without guessing

On Linux the authoritative answer is already installed. `xkbcommon`
knows what every layout produces per key, so rather than transcribing
a keyboard picture, compile the keymap and read the levels off it —
`xkb_keymap_key_by_name` for `AD01`…`AB10`, then
`xkb_keymap_key_get_syms_by_level` for group 1 levels 1 and 2, then
`xkb_keysym_to_utf8`. XKB key names map onto Set-1 scancodes in three
straight runs: `AE01`–`AE12` → `0x02`–`0x0D`, `AD01`–`AD12` →
`0x10`–`0x1B`, `AC01`–`AC11` → `0x1E`–`0x28`, `AB01`–`AB10` →
`0x2C`–`0x35`, plus `BKSL` → `0x2B` and `LSGT` → `0x56`.

The nine layouts added in 0.9.0 were generated this way and then
hand-reviewed; the generator is small enough to rewrite in an
afternoon and is not worth vendoring, but knowing the trick is worth
a paragraph.

### Dead keys

PolterType tracks *the character produced per keystroke*, not the
OS-level dead-key state machine. Surface the spacing equivalent
(`´`, `^`, `¨`) — that matches what shows up in a mid-correction
buffer the rare time the user actually presses a dead key alone.

### Test it

Add a TOML, restart PolterType, and watch the log:

```
loaded user layout layout=pl-PL keys=46 dict=false stem=pl_pl
```

If `keys=` is too low, you missed entries. If the layout doesn't
appear, look for a parse error in the log.

**On Windows, expect the OS to disagree with you — and win.** A
bundled or plug-in mapping there is replaced by whatever the installed
keyboard actually produces, because a Windows layout names a language
and a language is not a keyboard. You will see it happen:

```
adopted the OS keymap for this keyboard layout=pl-PL variant=00000415 keys=48 replaced=2
```

That is working as intended, and it means Windows is not the place to
check a hand-written table — the value you typed into the TOML may
never be used. A **user** TOML in your config dir still outranks the
OS, so that is how you test one deliberately. To see exactly which
keys the OS overruled, run with `RUST_LOG=poltertype_core=debug` and
read the `keys the OS disagreed on` line.

---

## 2. Wordlists (so the dictionary detector recognises the language)

Without a wordlist, PolterType falls back to the plausibility
detector — which is decent for distinctive scripts (Cyrillic vs
Latin) but unreliable inside a single script (German vs English).
A real wordlist makes detection trustworthy.

Four filenames per layout, all under either `data/wordlists/`
(bundled) or `<config-dir>/poltertype/wordlists/` (user):

| File | Goes into | Used for |
|---|---|---|
| `<stem>.txt` | `user_overlay` (full-length lookup) | The main dictionary. One lowercase word per line. Bundled languages commit this **gzipped**, as `<stem>.txt.gz`. |
| `<stem>-extras.txt` | same as above | Optional second file for organisation (tech vocab, surnames, …). Merged with `<stem>.txt`. |
| `<stem>-stop.txt` | `short_stop_words` (≤2-letter lookup) | Hand-curated 1- and 2-letter words. Optional — an absent file yields an empty set, which is graceful at runtime. |
| `<stem>-weak.txt` | `weak` | Marks entries that are technically valid but rare (archaic forms, obscure inflections), so that a *strong* dictionary hit in the other layout wins over a weak hit in this one. Optional. |

### Format

```
# blank lines and `#` comments ignored
# one lowercase word per line
hello
world
function
```

### Sourcing

Bundled languages source their wordlists from public projects with
non-restrictive licences — see `data/wordlists/CREDITS.md`. The
`cargo xtask wordlists fetch` command re-downloads them.

For a user-side language, anything works: a Hunspell stems file
stripped of affix flags, an `aspell dump master` output, a frequency
list — as long as it's one lowercase word per line.

### Wiring a new bundled language

To bundle a new language so users get it without touching their
config directory:

1. Drop the layout TOML into `data/layout-mappings/<stem>.toml`.
2. Add the dictionary source to
   `xtask/src/consts.rs::HUNSPELL_SOURCES` and run `cargo xtask
   wordlists fetch`, which downloads the `.dic` + `.aff`, expands the
   affix rules and writes `data/wordlists/<stem>.txt.gz` for you.
   (Hand-made lists work too — drop `<stem>.txt.gz` in yourself and
   skip the entry.) The optional companions all live in the same
   directory:
   * `<stem>-extras.txt` — your tech-vocab extras
   * `<stem>-stop.txt` — short stop words
   * `<stem>-weak.txt` — valid-but-rare entries

   Watch the reported form count. Anything in the tens of millions
   means the `.aff` is combinatorial rather than inflectional and
   wants `ExpandMode::StemsOnly` — Hebrew is the worked example, see
   `data/wordlists/CREDITS.md`.
3. Add the stem to `crates/poltertype-core/build.rs::LAYOUTS`, which
   is what copies the data into the dist tree:

   ```rust
   const LAYOUTS: &[(&str, &str)] = &[
       ("en_us", "en-US"),
       // ...
       ("pl_pl", "pl-PL"),
   ];
   ```

4. Add the same stem to
   `crates/poltertype-core/src/layouts/consts.rs::BUNDLED_LAYOUT_STEMS`,
   which is the list the runtime actually scans for:

   ```rust
   pub const BUNDLED_LAYOUT_STEMS: &[&str] =
       &["en_us", "uk_ua", "ru_ru", /* … */ "pt_br"];
   ```

   Nothing is baked into the binary — the TOML is read from
   `<data_dir>/layout-mappings/` at run time. The stem is all the
   runtime needs.

5. **Add the four files to `installers/wix/main.wxs`.** The MSI is
   the one installer that enumerates every data file by hand
   (`<File Id="MapPlPl" …>` and friends); the AppImage and the macOS
   bundle `cp -R` the whole tree and need nothing. Four entries per
   language — the layout TOML, `<stem>.fst`, `<stem>-stop.txt`,
   `<stem>-surface.fst` — and WiX **errors on a `Source=` that
   doesn't exist**, so only list files the build actually produces
   (that is why only `uk_ua` has a `-weak.txt` entry). Skip this step
   and the language works everywhere except Windows, where it
   silently isn't installed.

6. Register the layout id with the OS switchers in
   `poltertype-layout`, or the app can name your language but never
   switch to it:
   * Linux — `linux/shared.rs`, both directions of the XKB table.
   * macOS — `macos.rs`, `tis_id_to_bcp47` (list every variant you
     know) and `bcp47_to_tis_id` (base id only; a wrong guess here
     targets a keyboard the user doesn't have).
   * Windows needs nothing — it resolves BCP-47 from the HKL.

7. Optional but recommended: extend `derive_vowels` with the
   language's vowel set (especially if it uses accented vowels —
   the plausibility detector counts vowel ratio, and unrecognised
   vowels score as consonants).
8. `cargo build` and `cargo test --workspace`.

Keep (3) and (4) in lock-step. If they drift, the runtime logs a
"missing TOML" warning at startup for the stem it can't find — noisy
rather than silent, but it is not a build failure, so read the first
few log lines after wiring a new language.
`every_bundled_stem_resolves_to_a_layout` in
`crates/poltertype-core/src/layouts/tests.rs` fails on that drift, so
in practice `cargo test` catches it first.

---

## 3. User-side, no rebuild

If you want to add a language without touching the source tree
(e.g. you're not a Rust dev, or you just want a one-off custom
mapping for your odd keyboard):

1. Open the tray menu → **Open User Layouts Folder…** to ensure
   `<config-dir>/poltertype/layouts/` exists.
2. Drop `<stem>.toml` in there (schema as above).
3. Open the tray menu → **Open User Wordlists Folder…** for
   `<config-dir>/poltertype/wordlists/`.
4. Drop the wordlists there.
5. Restart PolterType (the engine snapshots the layout database at
   start; new layouts need a fresh boot to populate the scancode-
   translation tables). Adding *words* to an already-loaded language
   only needs **Reload Settings** from the tray menu.
6. Optionally edit `config.toml` and add your new id to
   `[languages].active`.

Same TOML, same wordlist format. If your `id` matches a bundled
layout, your file wins — handy if you disagree with the bundled
mapping for your physical keyboard.

---

## 4. Plausibility-only languages

Adding *just* a layout TOML without any wordlist is a perfectly
valid lightweight mode: the dictionary detector silently abstains,
and the plausibility detector picks up via vowel-ratio /
consonant-cluster / script-fit signals. This works well for layouts
whose script differs from any other active layout (a Greek user
with Greek + English, say) — script alone is a strong signal. For
two layouts in the same script, you'll want a real wordlist.
