//! App-wide constants: identifiers, default hotkeys, README bodies.

pub(crate) const APP_ID: &str = "dev.opensource.poltertype";

pub(crate) const APP_NAME: &str = "PolterType";

/// Parse a hotkey string from `[hotkeys]` (e.g. `"Ctrl+Shift+Space"`)
/// using `global-hotkey`'s native `FromStr`. On parse failure we log
/// a warning and fall back to `default_str` so the app boots with a
/// usable hotkey rather than nothing — matches the Settings UI's
/// "loud-but-graceful" approach to bad config values.
/// Cross-platform default for the manual "switch the last word" hotkey.
pub(crate) const DEFAULT_SWITCH_LAST: &str = "Ctrl+Shift+Backspace";

/// Wayland substitute for [`DEFAULT_SWITCH_LAST`]: a key the focused app
/// won't act on destructively (unlike `Ctrl+Backspace`, which deletes a
/// word). Used only when the user keeps the default on the evdev
/// keystream backend; any explicit custom binding wins.
pub(crate) const WAYLAND_SAFE_SWITCH_LAST: &str = "Ctrl+Shift+F9";

/// Permissions / onboarding guide the tray's "Setup Guide…" alert
/// entry opens when keyboard hooks fail to start. Pinned to `main` —
/// the guide must track the latest setup script, not the version of
/// the binary that failed.
pub(crate) const SETUP_GUIDE_URL: &str =
    "https://github.com/Just-Code-NET/PolterType/blob/main/docs/PERMISSIONS.md";

/// One-time README seeded into the user layouts folder. Mirrors the
/// wordlists README's plain-text, no-markdown style.
pub(crate) const USER_LAYOUTS_README: &str = "\
PolterType — user layouts
=========================

Drop layout-mapping TOML files here to add support for keyboards /
languages the app doesn't ship out of the box. New layouts are
picked up on the next app start.

File naming:
    Use a clear file stem matching the language code, lowercase, with
    underscore between language and country: `pl_pl.toml`, `tr_tr.toml`,
    `cs_cz.toml`, `nl_nl.toml`, …

TOML schema (same as the bundled `data/layout-mappings/*.toml`):

    id     = \"pl-PL\"          # BCP-47 ish; what config.toml refers to
    name   = \"Polski\"         # display name in the tray (optional)
    script = \"Latin\"          # Latin / Cyrillic / Greek / Armenian / Hebrew / Arabic / Other

    [keys]
    # Win SC Set-1 scancode → produced character.
    # `plain` is unshifted, `shift` is the shifted variant (optional).
    0x10 = { plain = \"q\", shift = \"Q\" }
    0x11 = { plain = \"w\", shift = \"W\" }
    # … and so on for the alphanumeric / punctuation rows that
    #   matter for word-boundary detection.

The bundled `en_us.toml` and `uk_ua.toml` files are excellent
copy-paste starting points — see the PolterType source repo,
`data/layout-mappings/`.

Picking up dictionary support:
    To get full word-detection (not just plausibility scoring),
    drop matching wordlists alongside in
    `<config-dir>/poltertype/wordlists/`:

        <stem>.txt          # main wordlist, one lowercase word per line
        <stem>-extras.txt   # same effect, separate file for organisation
        <stem>-stop.txt     # 1- and 2-letter stop words

    where `<stem>` is your TOML file's stem (`pl_pl` for `pl_pl.toml`).
    See the user wordlists README in `<config-dir>/poltertype/wordlists/`
    for the format.

Override the bundled mapping:
    If your TOML's `id` matches an embedded layout (e.g. `de-DE`),
    your file wins. Use this if your physical keyboard differs from
    the bundled mapping.
";

/// One-time README seeded into the user wordlists folder. Plain
/// text (no markdown), short, and readable in any editor / preview
/// pane. Matches the file conventions documented in
/// `poltertype_core::layouts::build_dictionary`.
pub(crate) const USER_WORDLISTS_README: &str = "\
PolterType — user wordlists
===========================

Drop text files here to extend the built-in dictionaries without
rebuilding the app. Changes are picked up on the next \"Reload
Settings\" tray click (Ctrl+Shift+R if you've bound it) — no restart
needed.

Per layout, three filenames are recognised. Replace `<stem>` with the
layout id you want to extend (`en_us`, `uk_ua`, …):

    <stem>.txt          One word per line; treated as a real word
                        in this layout, regardless of length.
                        Use this for tech vocab, surnames, slang,
                        product names — anything that should NOT
                        get auto-corrected away.

    <stem>-extras.txt   Same effect as <stem>.txt; separate file
                        so you can organise (e.g. one for tech
                        vocab, one for personal names). Both are
                        merged into the same overlay at load time.

    <stem>-stop.txt     Curated 1- and 2-letter additions. Needed
                        when you want a SHORT (≤2 letter) token
                        treated as a real word — at that length
                        the embedded full dictionary is bypassed
                        on purpose, so this is the only path that
                        works for short tokens.

Format for all three:
    - one lowercase word per line
    - blank lines and `# comment` lines ignored
    - UTF-8

Example (`uk_ua.txt`):
    кубернетес
    докерфайл
    редіс

Example (`uk_ua-stop.txt`):
    хм
    тю

Tip: the embedded dictionaries already cover ~370k EN and ~333k UK
entries plus a curated tech-vocab list. You only need files here for
words you actually see auto-corrected wrongly.
";
