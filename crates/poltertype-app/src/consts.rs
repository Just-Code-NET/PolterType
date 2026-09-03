//! App-wide constants: identifiers, default hotkeys, README bodies.

pub(crate) const APP_ID: &str = "dev.opensource.poltertype";

pub(crate) const APP_NAME: &str = "PolterType";

/// Cross-platform default for the manual "switch the last word" hotkey.
pub(crate) const DEFAULT_SWITCH_LAST: &str = "Ctrl+Shift+Backspace";

/// Cross-platform default for the pause/resume hotkey. Mirrors
/// `HotkeySettings::default()` in `poltertype-core`, which stays
/// platform-neutral on purpose.
pub(crate) const DEFAULT_PAUSE_TOGGLE: &str = "Ctrl+Shift+Space";

/// macOS substitute for [`DEFAULT_PAUSE_TOGGLE`]. `Ctrl+Space` and
/// `Ctrl+Shift+Space` are macOS's own "select previous/next input
/// source" shortcuts, so registering them globally preempts the very
/// layout switching this app exists to complement. `Ctrl+Shift+P` is
/// free of any system binding. Applied only when the user is still on
/// the default; an explicit choice is always honoured.
pub(crate) const MACOS_SAFE_PAUSE_TOGGLE: &str = "Ctrl+Shift+P";

/// Wayland substitute for [`DEFAULT_SWITCH_LAST`]: a key the focused app
/// won't act on destructively (unlike `Ctrl+Backspace`, which deletes a
/// word). Used only when the user keeps the default on the evdev
/// keystream backend; any explicit custom binding wins.
pub(crate) const WAYLAND_SAFE_SWITCH_LAST: &str = "Ctrl+Shift+F9";

/// Permissions / onboarding guide, linked from the Settings Setup pane.
/// Pinned to `main` — the guide must track the latest setup script, not
/// the version of the binary that failed.
pub(crate) const SETUP_GUIDE_URL: &str =
    "https://github.com/Just-Code-NET/PolterType/blob/main/docs/PERMISSIONS.md";

/// Where to send a user whose install can't update itself in place (a
/// distro package, a dev build, a bare binary) — or whose installer
/// failed.
pub(crate) const RELEASES_URL: &str = "https://github.com/Just-Code-NET/PolterType/releases/latest";

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

/// One-time README seeded into the user wordlists folder. The file
/// conventions it describes are enforced in
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
    - letters only: digits and punctuation are stripped on load,
      so `just-code.net` is stored as `justcodenet`. Words the
      app adds for you are written in that same shape.
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

/// How long to keep probing for a layout-switching backend at startup
/// before giving up, and how often.
///
/// Every backend is probed against something the session brings up
/// asynchronously — a compositor socket, a D-Bus name, a gsettings
/// schema — so at login we can be started before any of it exists.
pub(crate) const SWITCHER_PROBE_WINDOW: std::time::Duration = std::time::Duration::from_secs(15);
pub(crate) const SWITCHER_PROBE_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(500);

/// How often the tray re-stats `config.toml` to notice an edit made
/// anywhere else — the Settings window's Save, a text editor, another
/// machine's synced file. One `stat` a second is below noise, and the
/// file is replaced atomically, so its size and mtime move together.
pub(crate) const CONFIG_WATCH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

/// How often a plug-in that reports state is re-asked while the user is
/// not touching the menu. Slow on purpose: every tick costs one
/// subprocess per reporting plug-in.
pub(crate) const PLUGIN_STATE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);

/// How long a global hotkey may look "still held" before the next
/// `Pressed` is treated as a fresh press anyway. Only the OS-grab path
/// needs it — the keystream matcher latches on real key releases.
pub(crate) const STUCK_HOTKEY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

/// Tray submenu holding words a tooltip offered "Add to dictionary" for
/// and lost to the next keystroke (issue #38). Names the dictionary,
/// because picking a row writes to a user wordlist and the menu was the
/// only place that never said so.
pub(crate) const DEFERRED_MENU_LABEL: &str = "Add a missed word to the dictionary…";

/// The one row that submenu carries while nothing has been missed.
/// Disabled, and there so the entry can be opened at all: an empty
/// submenu answers a click with nothing, which reads as a broken menu.
pub(crate) const DEFERRED_MENU_EMPTY: &str = "No missed words yet";
