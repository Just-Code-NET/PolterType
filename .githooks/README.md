# kb-switcher git hooks

Versioned hooks that mirror the CI gates locally.

| Hook | What it runs | Why |
|---|---|---|
| `pre-commit` | `cargo fmt --all -- --check` | Refuses commits with formatter drift — same gate CI enforces. |
| `pre-push` | `cargo build --workspace --all-targets` | Refuses pushes if the workspace doesn't build. Catches typos before CI does. |

## Install

```bash
cargo xtask hooks install
```

This sets `core.hooksPath` to `.githooks/` for the local clone.
That's the one-line install — Git itself does the rest, no per-hook
symlinking. Re-run after `git clone` (and optionally drop the call
into your shell rc / `~/.config/git/init.templateDir` if you want
new clones to pick it up automatically).

## Uninstall

```bash
cargo xtask hooks uninstall
```

Unsets `core.hooksPath` — Git falls back to `.git/hooks/` (default,
empty in fresh clones).

## Bypass

For the rare case you really need to skip a hook (in-progress commit
on a feature branch, fixing a CI script in the same push that broke
it, etc.):

```bash
git commit --no-verify
git push --no-verify
```

Use sparingly. The hooks are here because forgetting `cargo fmt` and
discovering it via a red CI build wastes everyone's time.

## Why versioned `.githooks/`, not `.git/hooks/`

`.git/` is per-clone and not tracked by Git. Versioning the hooks in
the repo (and pointing `core.hooksPath` at them) means every
contributor gets the same gates, hook updates ride along with the
code that requires them, and there's no out-of-band setup beyond the
one `cargo xtask hooks install` invocation.
