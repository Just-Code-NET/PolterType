# `.claude/` — Claude Code config

This directory tunes [Claude Code](https://claude.com/claude-code) for
contributors working on `kb-switcher`. Anyone is welcome to use Claude
Code (or any other tool) here — these files just save you a few
permission prompts.

## Files

- **`settings.json`** — pre-approved tool list. Read-only Git, Cargo,
  pnpm and file operations are allowed without prompting. Anything
  destructive (push, force, hard reset, rm -rf) still asks first.

## Suggested entry points

- `CLAUDE.md` (repo root) — design rules, layout, hard constraints.
- `docs/PLAN.md` — full architectural plan and roadmap.

## Adding personal overrides

If you want extra permissions just for your machine, put them in
`.claude/settings.local.json` (already gitignored by the harness, not
committed).
