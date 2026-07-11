# Data layout & plug-in foundations

> Where layout mappings, FST wordlists, and (future) plug-ins live on
> disk, how they're discovered at runtime, and the contract a
> third-party language pack will need to satisfy when the plug-in
> marketplace lands.

## Why externalised data

Up to v0.1.0-alpha.0, layout TOMLs and per-language FST wordlists were
baked into `poltertype.exe` via `include_str!` / `include_bytes!`.
That made the binary large (~75 MB resident FST per language × 6
languages) and forced the engine to load every bundled language even
when the user only had a couple installed in the OS.

Externalising the data:

* Lets the runtime load **only** the wordlists for OS-active layouts,
  saving ~5–15 MB of FST RAM per skipped language.
* Removes the "fr-FR was selected as a switch target even though the
  user doesn't have French installed" failure mode at the source —
  unreachable layouts never enter the candidate set.
* Lays the groundwork for a plug-in marketplace: third-party language
  packs drop into a known directory, no rebuild needed.
* Makes installer disk usage transparent — operators / SCCM admins can
  see which language packs are deployed by listing one directory.

## On-disk layout

The data directory has a fixed shape, regardless of where it's
deployed:

```
<data_dir>/
  layout-mappings/        bundled keyboard maps (1 .toml per language)
    en_us.toml
    uk_ua.toml
    ...
  wordlists/              bundled dictionaries
    en_us.fst             FST built by build.rs from <stem>.txt.gz
    en_us-stop.txt        curated 1- / 2-letter stop words
    uk_ua-weak.txt        valid-but-rare entries, demoted in scoring
    ...
  plugins/                data-only language packs; the loader is live
                          (see "Plug-ins" below). Installing them is
                          still manual — the marketplace UX is future.
```

`build.rs` in `crates/poltertype-core` produces this tree at every cargo
build, writing to `<workspace>/target/dist/data/`. Installer scripts
copy that tree into the install location:

| Platform | exe location | data location |
|---|---|---|
| Windows MSI | `%LOCALAPPDATA%\poltertype contributors\poltertype\poltertype.exe` | `…\data\` (sibling) |
| macOS .dmg | `poltertype.app/Contents/MacOS/poltertype` | `poltertype.app/Contents/Resources/data/` |
| Linux AppImage | `<mount>/usr/bin/poltertype` | `<mount>/usr/share/poltertype/data/` |
| dev (`cargo run`) | `target/{debug,release}/poltertype` | `target/dist/data/` |

## Resolution at runtime

`poltertype_core::data_dir::resolve()` returns the active data directory by
trying each of the following in order, returning the first that
exists as a directory:

1. **`POLTERTYPE_DATA_DIR` env override** — escape hatch for tests
   and unusual deployments.
2. **`<exe_dir>/data/`** — Windows MSI install layout, portable mode,
   the layout linuxdeploy produces inside an AppImage AppDir.
3. **`<exe_dir>/../Resources/data/`** — macOS `.app` bundle layout.
4. **`<exe_dir>/../share/poltertype/data/`** — alternate Linux layout
   when an unprefixed binary is dropped in `/usr/bin/`.
5. **`<workspace>/target/dist/data/`** — dev mode, deduced from the
   exe location by walking up to a parent named `target`.

If nothing matches, `resolve()` returns `DataDirError::NotFound` with
every path it tried, so a misdeployed install is debuggable from one
log line.

## Lazy loading by OS-active filter

`poltertype-app` resolves the data dir at startup, queries the OS for the
list of currently active keyboard layouts via
`LayoutSwitcher::list_active()`, and passes that list to
`LayoutDb::load(LoadOptions { active_filter, … })`. Only layouts
whose id is in the filter are read into memory; the rest stay on
disk.

A user with `en-US / uk-UA / ru-RU` enabled in Windows will see:

```
INFO  poltertype_app: OS active layouts: [LayoutId("en-US"), LayoutId("uk-UA"), LayoutId("ru-RU")]
INFO  poltertype_core::layouts: skipping bundled layout — not in active OS list  layout=fr-FR
INFO  poltertype_core::layouts: skipping bundled layout — not in active OS list  layout=de-DE
INFO  poltertype_core::layouts: skipping bundled layout — not in active OS list  layout=es-ES
INFO  poltertype_core::layouts: loaded bundled layout                            layout=en-US
INFO  poltertype_core::layouts: loaded bundled layout                            layout=ru-RU
INFO  poltertype_core::layouts: loaded bundled layout                            layout=uk-UA
INFO  poltertype_app: layout DB ready  loaded=3
```

Adding a language in the OS requires a PolterType restart to pick
it up (`list_active` is queried once at startup). This keeps the
hot path simple and predictable; the cost is one kill-and-relaunch
when you reorganise your input methods.

If the OS query fails for any reason (`LayoutError::Unsupported` /
`Os(...)`), PolterType fails open: it loads every bundled layout,
matching the pre-filter behaviour. The detector and the
`apply_correction` pre-flight together still keep the engine from
typing into an unreachable layout.

## Plug-ins (loader live; marketplace future)

The `plugins/` directory under `<data_dir>/` holds data-only language
packs. **The loader is live** — what's still missing is the
install/update UX, not the mechanism. The contract a third-party pack
has to satisfy:

```
<data_dir>/plugins/<pack-id>/
  manifest.toml             metadata: id, version, supported layouts
  layout-mappings/          drops into the merged mapping pool
    <stem>.toml
  wordlists/                drops into the merged dictionary pool
    <stem>.fst
    <stem>-stop.txt
  README.md                 optional human-readable description
```

The loader treats `plugins/<pack-id>/{layout-mappings,wordlists}`
exactly the same way it treats the bundled
`<data_dir>/{layout-mappings,wordlists}` — so plug-in authors don't
need to learn a separate API, and conflict resolution (a pack
overriding a bundled language) is the same `id` collision rule
already in place for user-side TOMLs in
`<config-dir>/poltertype/layouts/`.

The loader is **live as of 0.1.0-alpha.0**. `<data_dir>/plugins/`
is enumerated at every `LayoutDb` load; each pack's
`layout-mappings/*.toml` (with optional `wordlists/<stem>.fst` next
to it) is merged into the runtime layout DB the same way bundled
layouts are. Until the marketplace UI ships, the only way to install
a pack is to drop the directory tree there yourself — useful for
internal teams shipping a custom layout set, or for testing your own
pack before publishing.

### What plug-ins won't be (in v1)

* **Native code.** No `.dll` / `.dylib` / `.so` loading. The plug-in
  surface is data-only: TOMLs and FSTs. This caps the security
  blast-radius of a malicious pack and rules out platform-portability
  headaches.
* **Network calls.** PolterType's no-network rule (see CLAUDE.md)
  applies to plug-ins. A pack that wants to fetch updates does it
  through a separate user-driven download / installer step, not at
  PolterType runtime.
* **Settings injection.** Plug-ins can ship default short-stop words
  and dictionary entries; they cannot toggle global engine flags
  (autostart, hotkeys, …). The user owns those.

These constraints are deliberate — they let v1 plug-in support stay
small and reviewable, with a clear extension path for native /
network / settings hooks in later versions if the use cases turn
out to need them.

## How user overlays differ from plug-ins

`<config-dir>/poltertype/layouts/` and
`<config-dir>/poltertype/wordlists/` already let an individual user
add layouts and dictionary entries without rebuilding the app. The
two layers solve different problems:

| Layer | Lives in | Audience | Versioning |
|---|---|---|---|
| Bundled `<data_dir>/` | install dir (read-only after install) | every user of this install | shipped with the app |
| Plug-in pack `<data_dir>/plugins/<id>/` | install dir | every user of this install | per-pack manifest |
| User overlay `<config-dir>/…` | per-user profile | one user | not versioned (live edit) |
| Per-app overlay `<config-dir>/poltertype/wordlists/profiles/<id>/<stem>.txt` | per-user profile | one user, one set of apps | not versioned (live edit) |

The per-app overlays are declared as `[[wordlists.profiles]]` entries
in `config.toml`; the engine swaps the active overlay set when the
focused app changes. **Caveat:** focus tracking is implemented on
Windows only, so on macOS and Linux the profile set never switches.

Order of precedence at load time (last writer wins on `id` collision):

```
bundled  ←  plug-ins  ←  user-overlay
```

So a user can override a bundled mapping, a plug-in can override a
bundled one, and a user can override a plug-in. The override is
explicit — you put a TOML with the same `id` in your config dir and
restart.
