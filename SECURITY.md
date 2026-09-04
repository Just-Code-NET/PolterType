# Security policy

PolterType reads every keystroke on the machine it runs on, and it can
type. That is what the product does, and it is also why a security
report here deserves a serious answer rather than a form letter.

## Reporting a vulnerability

**Use GitHub's private reporting:**
[**Report a vulnerability**](https://github.com/Just-Code-NET/PolterType/security/advisories/new).
It is enabled on this repository, it is private until we publish it,
and it keeps the whole discussion attached to the fix.

Please **do not open a public issue** for anything that would let
someone read another person's keystrokes, install code on their
machine, or push a malicious update.

If you cannot use GitHub's form, email **support@just-code.net**.
Please keep the details out of any public issue — for this app, a
proof-of-concept posted in the open is a working exploit posted in the
open.

What helps, in rough order of usefulness:

- the version (**Settings → About**, or `poltertype --version`) and the
  operating system + desktop environment;
- what an attacker would gain, even roughly — "reads the word buffer
  from another process" is worth more than a stack trace;
- a way to reproduce it, if you have one;
- whether you want to be credited, and under what name.

**Logs are safe to paste.** Since v0.6.3 released builds cannot print
typed text at all — the redaction gate requires a debug build *and* an
explicit environment variable, and the first condition is false at
compile time. If you ever find a released build printing a word you
typed, that by itself is the vulnerability, and it is exactly the kind
of report we want.

## What we can promise

One maintainer, no service-level agreement, and no pretending
otherwise. Realistically: an acknowledgement within a few days, an
assessment after that, and a fixed release as soon as the fix is
written and tested. If something is being actively exploited, say so
in the report — that changes the order of everything else.

We will credit you in the release notes unless you ask us not to.

## Supported versions

**The latest release only.** PolterType is pre-1.0 on a single release
line: fixes go into the next version, and the in-app updater brings
users to it. There are no maintenance branches to backport to, and
inventing one on paper would be a promise we could not keep.

| Version | Supported |
|---|---|
| latest release | yes |
| anything older | no — update first, then report if it persists |

## In scope

The things that would actually hurt someone:

- **Typed text escaping the machine or reaching disk** — a log line, a
  crash dump, a temporary file, an error message. The privacy claim is
  that this cannot happen; a counter-example is a real finding.
- **The update path.** The updater fetches a manifest and installers
  from GitHub Releases. Anything that lets an attacker substitute what
  gets downloaded, bypass the SHA-256 check, or defeat the ed25519
  signature over `latest.json` is in scope. So is anything that makes
  it install at a moment we promise it will not — the swap happens on
  Quit or on an explicit restart, never while typing.
- **The key gate wedging the keyboard.** On Linux the correction path
  can hold keystrokes back. A way to leave that hold stuck means the
  user's keyboard stops working system-wide, which we treat as a
  security issue and not merely a bug.
- **Privilege problems in `scripts/setup-linux.sh`**, the udev rule, or
  the packaging — anything that grants more than read access to input
  devices and write access to `/dev/uinput`.
- **The macOS quarantine strip.** The updater removes
  `com.apple.quarantine` from the installed bundle. It is documented,
  deliberate while builds are unsigned, and scheduled for removal once
  they are notarised — but if you can turn it into something worse than
  it looks, tell us.
- **Smart commands and the plug-in loader** — anything that turns
  data-only configuration into code execution.
- **A plug-in's translation catalog.** It is confined to
  `plugin.<id>.`: an extension can reword its own pane and its own
  tray entries, and nothing else. A file that reaches a label
  PolterType drew itself — a button, a warning, the update entry — is
  exactly the report we want.

## Out of scope

Not because we do not care, but because they are known and written
down. A report about these is welcome as a normal issue; it will not be
treated as a vulnerability:

- **"It can read keystrokes."** That is the feature. The operating
  system asks for your permission before allowing it, and
  [`docs/PERMISSIONS.md`](docs/PERMISSIONS.md) explains exactly what is
  granted and why.
- **"With `[selection].enabled` on, it reads the clipboard."** That is
  what the setting turns on, which is why it is off by default and why
  the app never takes keyboard focus to get it —
  [`docs/PERMISSIONS.md`](docs/PERMISSIONS.md) § Clipboard has the
  mechanism and its two inherent limits. A way to make it copy when the
  user did **not** press the hotkey would be a real finding.
- **"The installers are unsigned."** Known, stated on the download page
  and in the README, and being worked on — see
  [`docs/CODE_SIGNING.md`](docs/CODE_SIGNING.md).
- **"Linux needs group membership for `/dev/input`."** That is how
  Wayland works; there is no lower-privilege path that also functions.
- **Findings from an automated scanner with no described impact.** A
  report that says which line does what, and what an attacker gets,
  will always get a real answer.

## Where the honest list of weaknesses lives

Every known gap is written down rather than discovered by you:
[`docs/KNOWN-GAPS.md`](docs/KNOWN-GAPS.md) is the ledger — what does
not work despite looking as if it should, and what is reasoned rather
than measured — and [`docs/DECISIONS.md`](docs/DECISIONS.md) records
why each trade-off was made. If something there looks worse than we
judged it, that is a conversation worth having.
