# Distribution packaging

Ready-to-publish manifests for the three package managers PolterType's
users actually ask for. **Nothing in here is live yet** — every file is
staged, reviewed and version-controlled here, and publishing each one is
a deliberate human step against a third-party system.

| Directory | Target | Publishing needs |
|---|---|---|
| [`aur/`](aur/) | Arch User Repository — `poltertype` (source) and `poltertype-bin` (repacked AppImage) | an AUR account + its SSH key |
| [`winget/`](winget/) | `winget install JustCode.PolterType` | a PR to `microsoft/winget-pkgs` |
| [`homebrew/`](homebrew/) | `brew install --cask just-code-net/tap/poltertype` | a `Just-Code-NET/homebrew-tap` repository |

> **The README's install table does not mention any of these yet, and
> must not until the corresponding package is live.** The site and the
> README may only promise what a user can actually do today — see the
> workspace `CLAUDE.md`. Add the line in the same change that publishes
> the package, not before.

## Bumping to a new release

```bash
packaging/bump.sh 0.7.0
```

Downloads that release's artifacts, hashes the bytes GitHub is actually
serving, and rewrites the version + checksum in all three targets. It
publishes nothing. Run it only against a **published** release: a
checksum is only worth anything if it came from the file users will
download.

---

## AUR

Two packages, either or both. They conflict with each other by design
(`poltertype-bin` provides `poltertype`), so a user picks one.

* **`poltertype`** builds from the release tarball. `--frozen` and the
  committed `Cargo.lock` mean it builds the dependency versions upstream
  tested, not whatever crates.io resolved this morning.
* **`poltertype-bin`** extracts the release AppImage. `--appimage-extract`
  unpacks without mounting anything, so it works in the clean chroot an
  AUR package is supposed to build in — no FUSE required.

Both install the udev rule and the `modules-load.d` entry. Neither adds
anyone to the `input` group: that is a change to a user's account, and a
package that makes it silently has decided something on their behalf
that they did not ask for. The `.install` notice says the one command to
run and — the part people lose an evening to — that a login session
keeps the group membership it was created with, so it must be restarted.

Publishing (first time):

```bash
git clone ssh://aur@aur.archlinux.org/poltertype.git aur-poltertype
cp packaging/aur/poltertype/* packaging/aur/99-poltertype.rules \
   packaging/aur/uinput.conf aur-poltertype/
cd aur-poltertype
makepkg --printsrcinfo > .SRCINFO      # AUR requires this, and requires it current
makepkg -si                            # build and install it once before pushing
namcap PKGBUILD ./*.pkg.tar.zst        # catches missing/redundant deps
git add PKGBUILD .SRCINFO poltertype.install 99-poltertype.rules uinput.conf
git commit -m "Initial import: poltertype 0.6.3"
git push
```

Same for `poltertype-bin`, whose directory is already self-contained
(the shared files are copies — an AUR repo cannot reach outside itself,
so if you change one, change both).

`.SRCINFO` is not committed here on purpose: it is generated from the
PKGBUILD, it is only meaningful inside an AUR repo, and a stale copy in
two places is a way to publish a package that does not match its own
metadata.

**If someone else wants to maintain these under their own AUR account,
that is the better outcome** — AUR works best when the packager is a
user of the package. Point them at these files and link the result from
the README.

Worth knowing: the built-in updater deliberately refuses to overwrite a
file pacman owns (it requires `$APPIMAGE` to be set — an allowlist, not
a list of package managers to recognise), so packaged users get updates
through the AUR. That is correct, not a limitation to work around.

## winget

The MSI is exactly what winget wants: per-user, no elevation, silent-
install capable. Suggested identifier `JustCode.PolterType`.

```bash
# Regenerate rather than hand-edit — wingetcreate fills in the
# installer hash from the real download and validates the schema.
wingetcreate update JustCode.PolterType \
    --version 0.6.3 \
    --urls https://github.com/Just-Code-NET/PolterType/releases/download/v0.6.3/poltertype-0.6.3-x86_64-pc-windows-msvc.msi \
    --submit
```

The manifests here are the hand-written first version, kept in the repo
so the metadata (description, tags, support URLs) is reviewed like code
rather than typed into a wizard once.

**The open question is the unsigned MSI.** Automated validation may
accept it — MSI plus Mark-of-the-Web is handled more gently than a bare
`.exe` — but if SmartScreen reputation blocks the submission, park the
PR and retry when signed installers ship. Do not work around it.

Once merged: add the `winget install` line to the README install table
and the site's Windows card, and consider wiring `wingetcreate update`
into the release flow.

## Homebrew

**Stage 1 — our own tap, which needs no thresholds.** Create
`Just-Code-NET/homebrew-tap` and drop [`homebrew/poltertype.rb`](homebrew/poltertype.rb)
in as `Casks/poltertype.rb`. Install line becomes
`brew install --cask just-code-net/tap/poltertype`.

The cask deliberately does **not** carry `quarantine: false` and does not
strip the quarantine flag in a postflight. The installers are unsigned;
removing that check silently, on the user's behalf, for an app that reads
every keystroke they type, is not a convenience we get to hand out. The
caveat explains the right-click → Open route and the two macOS
permissions instead. Revisit the day the DMG is notarised — and only
then.

**Stage 2 — the main `homebrew-cask`, later.** It has notability
thresholds for GitHub-hosted apps (≥75 stars / 30 forks / 30 watchers,
with an exception path for apps that have their own website).
poltertype.com may qualify for that exception; a rejected PR costs
nothing and tells us exactly where the bar is. When it lands, promote
the cask and deprecate the tap entry.
