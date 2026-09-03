//! `cargo xtask style` — the file-organization and platform-split
//! rules of `CONTRIBUTING.md`, checked instead of remembered.
//!
//! Two things the compiler has no opinion about and review kept
//! catching by hand: which kind of declaration belongs in which file,
//! and where a platform `cfg` may appear. Both are stated in
//! `CONTRIBUTING.md`; this makes them fail the build.

mod consts;
mod enums;
mod modtree;
mod rules;
mod run;
mod scan;
mod types;

#[cfg(test)]
mod tests;

pub(crate) use run::run;
