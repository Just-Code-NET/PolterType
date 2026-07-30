<!-- Thanks for contributing! A short PR is a fast PR. -->

## What & why

<!-- One or two sentences. If there was a trade-off, name it here —
we keep design reasoning in PR descriptions, not code comments
(see CONTRIBUTING.md). -->

## Platforms touched

<!-- all / Windows / macOS / Linux (Wayland / X11) / none (docs, data) -->

## Checklist

- [ ] `cargo fmt --all` and `cargo clippy --workspace --all-targets --all-features -- -D warnings` are clean
      (or run once: `cargo xtask hooks install` — the pre-commit hook covers this)
- [ ] `cargo test --workspace` passes
- [ ] Docs that describe the changed behaviour are updated in the same PR (README, `docs/…`)
- [ ] Platform-specific code stays inside the per-OS crates — no new `#[cfg(target_os)]` in `poltertype-app` / `poltertype-core` (CONTRIBUTING.md explains where it belongs)
- [ ] When touching OS APIs: the official doc for the API is linked in this description
