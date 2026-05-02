//! `cargo xtask` — internal helper commands.
//!
//! Phase 1 placeholder; future home for `xtask dist`, `xtask icon-gen`,
//! `xtask bundle-linux-setup`, etc.

use anyhow::{Result, bail};

fn main() -> Result<()> {
    let cmd = std::env::args().nth(1);
    match cmd.as_deref() {
        Some("help") | None => {
            println!("xtask: no commands implemented yet (Phase 1 stub)");
            Ok(())
        }
        Some(other) => bail!("unknown xtask command: {other}"),
    }
}
