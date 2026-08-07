# Distribution packaging

Where PolterType can realistically be installed from, what it takes to
get there, and — for two popular targets — why it cannot. **Nothing in
this directory is live yet:** every file is staged, reviewed and
version-controlled here, and publishing each one is a deliberate human
step against a third-party system.

> **The README install table and the site must not mention a target
> until that target is live.** They may only promise what a user can
> actually do today — see the workspace `CLAUDE.md`. Add the line in the
> same change that publishes the package, not before.

## Read this first: Linux is two problems, not one

Every Linux packaging conversation about this app collapses into the
same confusion unless you split it:

**1. Native packages** — `.deb`, `.rpm`, AUR. No sandbox, so PolterType
simply works: it reads `/dev/input`, writes `/dev/uinput`, and shells
out to `hyprctl` / `gsettings` / `gdbus` / `qdbus` / `ibus` exactly as it does
from an AppImage. Nothing here is a technical question. The only real
questions are **how the packages get built** and **where they are
hosted**, and both have boring, good answers.

**2. Sandboxed formats** — Flatpak, Snap. Here the sandbox is the
product's problem, and it bites twice:

- **`/dev/uinput`.** Typing a correction means creating a virtual
  keyboard. Flatpak cannot grant this short of `--device=all`
  (`device=input` deliberately excludes it). Snap *can* — it has a
  `uinput` interface — so Snap clears this bar and Flatpak does not.
- **Host binaries.** Layout switching is not one API; it is
  `hyprctl`, `gsettings`, `gdbus`, `qdbus`/`qdbus6`, `ibus` and `fcitx5-remote`,
  none of which exist inside a runtime. **This is the wall both formats
  hit**, and it is the one people forget: an app that cannot switch the
  layout can still detect a wrong-layout word, fix the letters, and
  leave the next word just as wrong.

So the honest summary is not "sandboxes are hard" but: *the thing this
app does is talk to the desktop, and sandboxes exist to stop that.*

## Where things stand

| Target | Reach | Status |
|---|---|---|
| [AUR](#aur) | Arch, Manjaro, EndeavourOS | **staged** — `aur/`, needs an AUR account |
| [`.deb` + `.rpm`](#deb-and-rpm-apt-dnf-zypper) | Debian, Ubuntu, Fedora, RHEL, openSUSE | **not built yet** — the highest-value gap |
| [winget](#winget) | Windows 10/11 | **staged** — `winget/`, needs a PR upstream |
| [Chocolatey](#chocolatey) | Windows, dev-heavy | **not written** — moderation risk, see below |
| [Scoop](#scoop) | Windows, dev-heavy | **not written** — cheapest of all Windows options |
| [Homebrew](#homebrew) | macOS | **staged** — `homebrew/`, needs a tap repo |
| [Snap](#snap) | Ubuntu and derivatives | **blocked on review**, not on code |
| [Flatpak](#flatpak) | Fedora, Steam Deck, GNOME users | **no** — decided with evidence |
| [Nix](#nix) | nixpkgs users | **not written** — contributor-shaped |

## Before adding a tenth target, read this

Every package manager here is a build to keep green, a download to
explain, a checksum to bump and a stranger's bug report to answer. Three
targets are a chore; ten are a part-time job, and a stale package is
worse than a missing one because it looks maintained.

Two rules keep that honest:

- **`bump.sh` must learn every new target in the same change that adds
  it.** A target it does not know goes stale silently — which is exactly
  how winget's `ReleaseDate` sat at the wrong date until it was caught.
- **Prefer targets where someone else does the work.** An AUR package
  maintained by an Arch user, or a nixpkgs entry maintained by a Nix
  user, costs us a review. The same package maintained by us costs us
  every release, forever.

## Signing, per platform — they are not the same thing

Worth stating plainly, because "we should sign our releases" means
three unrelated pieces of work:

- **Windows and macOS have a gatekeeper and a certificate authority.**
  An unsigned installer produces a scary dialog, and the fix is a
  certificate someone issues you — free from
  [SignPath Foundation](https://signpath.org/) for Windows OSS, $99/year
  from Apple for macOS with no open-source exemption.
- **Linux has neither.** There is no CA in this story and nothing to
  buy. No distribution shows a warning for an unsigned AppImage,
  because the trust model is the *repository*, not the binary. What
  Linux does require is a **GPG key on the repository** the moment we
  host one — `apt` and `dnf` refuse or warn otherwise — and that key is
  one we generate, for free, forever. The cost is custody, not money.
  Until repositories exist, the ed25519 signature over `latest.json`
  (live since v0.7.0) is what protects the path that actually matters:
  updates.
- **The stores sign for you.** Flathub and the Snap Store sign what
  they publish with their own infrastructure; nothing for us to hold.

Full custody rules, and what changes on the day macOS builds are
notarised, are in [`docs/CODE_SIGNING.md`](../docs/CODE_SIGNING.md).

## Bumping to a new release

```bash
packaging/bump.sh 0.7.0
```

Downloads that release's artifacts, hashes the bytes GitHub is actually
serving, and rewrites the version, checksum and release date across
every staged target. It publishes nothing. Run it only against a
**published** release: a checksum is only worth anything if it came from
the file users will download.

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
git commit -m "Initial import: poltertype 0.7.0"
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
    --version 0.7.0 \
    --urls https://github.com/Just-Code-NET/PolterType/releases/download/v0.7.0/poltertype-0.7.0-x86_64-pc-windows-msvc.msi \
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

## `.deb` and `.rpm` — apt, dnf, zypper

The biggest gap by reach: Debian, Ubuntu, Fedora, RHEL and openSUSE
between them are most Linux desktops, and none of them has anything to
install today beyond a bare AppImage.

**Building them: one config, not two.** [`nfpm`](https://nfpm.goreleaser.com/)
takes a single YAML and emits `.deb`, `.rpm` and `.apk` from the same
already-built binary — no `debhelper`, no `.spec`, no second toolchain,
and it runs as one step in `release.yml` next to the AppImage job.
The alternatives are `cargo-deb` plus `cargo-generate-rpm`, which means
two tools that must agree about the same file layout; that disagreement
is where packaging bugs live.

The package contents are already decided — the AUR PKGBUILD is the
reference: the binary at `/usr/bin/poltertype`, the data tree at
`/usr/share/poltertype/data` (resolution rule 4 — get this wrong and
the app starts knowing no languages), the desktop entry, the udev rule
at `/usr/lib/udev/rules.d/99-poltertype.rules`, the `modules-load.d`
entry, and a post-install message that does **not** add anyone to the
`input` group.

**Hosting them: three honest options, in increasing order of effort.**

1. **Attach them to the GitHub Release.** Zero infrastructure, works
   the day it ships: `sudo apt install ./poltertype_0.7.0_amd64.deb`.
   What it does not give you is `apt upgrade` — the in-app updater
   already refuses to overwrite a dpkg-owned file, so those users
   update by downloading again. Start here.
2. **[Open Build Service](https://build.opensuse.org/)** — one project
   builds *and hosts signed repositories* for Debian, Ubuntu, Fedora
   and openSUSE from the same source. This is the standard answer for
   "I want `apt install` and `dnf install` without running my own
   infrastructure", it is free for open source, and it is the only
   option here that gives real repository upgrades on every distro at
   once. It is also a second build system to learn and keep green.
3. **[Fedora COPR](https://copr.fedorainfracloud.org/)** (Fedora/EPEL
   only) or an **Ubuntu PPA** (Ubuntu only). Narrower than OBS for
   comparable effort; worth it only if a contributor already lives in
   one of those ecosystems and offers.

**Getting into Debian or Fedora proper is a different sport** — a
distro maintainer sponsors the package, the freeze cycles are theirs,
and the version users get lags ours by months. Not a goal; if someone
volunteers, that is their contribution to make.

## Snap

**Blocked on a review decision, not on the code** — which makes it the
one sandboxed format still worth an attempt.

`uinput` is grantable: snapd has had a
[`uinput` interface](https://snapcraft.io/docs/reference/interfaces/uinput-interface/)
since 2.46. It does not auto-connect, so the user runs
`snap connect poltertype:uinput`, and snapd deliberately installs no
udev rules on a snap's behalf — meaning the host still needs the
`input` group exactly as it does for the AppImage. That is our existing
setup story, not a new one.

The wall is the other half. Layout switching needs host binaries, and
[strict confinement cannot execute them](https://snapcraft.io/docs/explanation/security/snap-confinement/).
The escape hatch is **classic confinement**, which is precisely "for
apps that need access to arbitrary binaries on the host" — and which
[requires manual approval from the Snap Store](https://documentation.ubuntu.com/snapcraft/latest/explanation/classic-confinement/),
because a classic snap has no sandbox left to speak of.

So the question is not technical, it is: *will the reviewers grant
classic confinement to a keyboard utility?* Nobody here can answer that
by reasoning; the way to find out is to ask, and a rejected request
costs a form. If it is granted, Ubuntu users get `snap install` and we
maintain a snapcraft.yaml. If not, the answer is the AppImage and the
`.deb`.

**Do not ship a strict-confinement snap that silently cannot switch
layouts.** It would install, run, detect, correct the letters, and
leave the layout wrong — which reads as the app being broken rather
than confined.

## Flatpak

**No, and this was decided with sources on 2026-07-31** — see
[`docs/DECISIONS.md`](../docs/DECISIONS.md) and issue #12. The README
says so plainly so the question does not get re-asked every few months.

Both blockers apply and neither has an escape hatch:

- `/dev/uinput` is not grantable short of `--device=all` — the entire
  device tree — because
  [`device=input` had `/dev/uinput` removed before it landed](https://discourse.flathub.org/t/support-for-device-input/6645),
  and no portal exists. Flathub requires that
  [static permissions be kept to an absolute minimum](https://docs.flathub.org/docs/for-app-authors/requirements).
- Host binaries for layout switching would need `flatpak-spawn --host`,
  which is "bypassing security mechanisms" in Flathub's own words.

The precedent is
[input-remapper's request](https://discourse.flathub.org/t/input-remapper-flatpak-request/3814),
closed as technically infeasible on exactly this reasoning.

**The one door that is not locked:** a Flatpak distributed *outside*
Flathub — a `.flatpakref` on poltertype.com — has no reviewer, and can
therefore declare `--device=all` and `--talk-name=*` freely. It would
work. It would also be a sandbox with the walls removed, shipped with a
padlock icon, and every install would need a repository the user adds
by hand. That is a worse story than the AppImage, told less honestly.
Not recommended, but it is the accurate answer to "is it *possible*".

**Revisit if** a portal for creating virtual input devices appears, or
`device=input` regains `/dev/uinput`. Both would need to land *and* be
widely deployed.

## Chocolatey

The other half of Windows: winget ships with the OS, Chocolatey is what
developers and admins already have scripted. A `.nuspec` plus an
install script that fetches our MSI and verifies its checksum is a
small package — the MSI is per-user and silent-install capable, which
is exactly what Chocolatey wants.

**The risk is moderation, and it is specific to what this app is.**
Every community package version is
[reviewed by a human and scanned through VirusTotal](https://docs.chocolatey.org/en-us/information/security/),
including any binary the script downloads. PolterType installs a global
low-level keyboard hook — the same primitive a keylogger uses — and
that is a well-known source of heuristic antivirus flags. A handful of
engines flagging the MSI would not make the package wrong, but it can
stall moderation and it will be the first thing a commenter points at.

**Which is the argument for doing signed installers first.** A
code-signed MSI with an OV certificate is the single change that most
improves this, and it improves winget, SmartScreen and the download
page at the same time. See `PROMOTION-PLAN.md` §4 — SignPath Foundation
signs open-source releases for free and we meet every requirement.

## Scoop

The cheapest package in this entire document: a JSON manifest in a
bucket, no moderation, no review, no build. Scoop's audience is
developers on Windows — a small slice, but one that overlaps almost
exactly with people who type in two languages and script their setup.

Two options, same as Homebrew: our own bucket
(`Just-Code-NET/scoop-bucket`, live the moment it is pushed), or the
community `extras` bucket, which has its own criteria. Start with our
own; it costs one repository and one JSON file.

Worth knowing: Scoop prefers portable, non-installer artifacts. The MSI
works, but a plain `.zip` of the binary plus the data tree would fit
Scoop's model better — and we do not build one today.

## Nix

`nixpkgs` is contributor-shaped work: the derivation is not hard, but
it wants someone who runs Nix daily to keep it current and to answer
Nix-specific review comments. The audience overlap with this project is
unusually high.

**The right move is an issue asking for a maintainer, not a derivation
written by someone who does not use Nix** — the same reasoning as the
AUR offer. A package nobody uses and nobody maintains is a liability
with a version number.

## Deliberately not doing

- **Microsoft Store** — the packaging model fights a tray app that
  installs a global hook, and the audience already has winget.
- **MacPorts** — Homebrew is where this audience is.
- **Debian / Fedora official repositories** — maintainer-sponsored,
  freeze-bound, and months behind our releases. Welcome if someone
  volunteers; not something to pursue.
- **A Flatpak on Flathub** — see above. Decided, with sources.

## Ordering

If all of this gets done, this is the order that wastes the least
effort:

1. **Signed installers** (SignPath application — weeks of lead time, so
   start it before anything else). Unblocks or improves winget,
   Chocolatey, the site's warning box and the Homebrew caveat at once.
2. **`.deb` + `.rpm` attached to the release.** Biggest reach per hour,
   no infrastructure, no third-party review.
3. **The three staged targets** — AUR, winget, Homebrew tap — since
   they are already written and reviewed.
4. **Scoop**, because it is nearly free.
5. **OBS**, if and when real `apt`/`dnf` repositories are worth a second
   build system.
6. **Snap**, as a classic-confinement request — an experiment with a
   known cost and an unknown answer.
7. **Nix**, as a call for a maintainer.
