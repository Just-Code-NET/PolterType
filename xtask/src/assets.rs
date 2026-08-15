//! `assets icon-png` / `assets icon-ico` — render the app icon.
//!
//! The geometry lives in `poltertype-icon`, which the app's own build
//! script also uses; this is the command-line door onto it. Release CI
//! calls both: the PNG becomes macOS's `.icns` and the AppImage's
//! icon, the ICO becomes the MSI's Add/Remove Programs entry.

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

/// Parse `<out-path> [--size N]` and render a PNG.
///
/// Tiny ad-hoc parser instead of a clap dep — we only have one flag,
/// and the xtask crate has been resolutely zero-config so far.
pub fn render_png_command(args: &[String]) -> Result<()> {
    let mut out_path: Option<PathBuf> = None;
    let mut size: u32 = 1024;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--size" => {
                let v = args
                    .get(i + 1)
                    .context("--size needs a value (e.g. --size 256)")?;
                size = v
                    .parse()
                    .with_context(|| format!("--size {v}: not a u32"))?;
                i += 2;
            }
            other if !other.starts_with('-') && out_path.is_none() => {
                out_path = Some(PathBuf::from(other));
                i += 1;
            }
            other => bail!(
                "unexpected argument {other:?} (usage: cargo xtask assets icon-png <out-path> [--size N])"
            ),
        }
    }
    let out = out_path.context("missing output path (cargo xtask assets icon-png <out>)")?;
    poltertype_icon::render_png(size, &out)?;
    println!("rendered {size}×{size} icon to {}", out.display());
    Ok(())
}

/// Parse `<out-path>` and render the multi-size Windows icon.
pub fn render_ico_command(args: &[String]) -> Result<()> {
    let [out] = args else {
        bail!("usage: cargo xtask assets icon-ico <out-path>");
    };
    let out = PathBuf::from(out);
    poltertype_icon::render_ico(&out)?;
    let sizes: Vec<String> = poltertype_icon::ICO_SIZES
        .iter()
        .map(u32::to_string)
        .collect();
    println!(
        "rendered a {}-size icon ({}) to {}",
        sizes.len(),
        sizes.join(", "),
        out.display()
    );
    Ok(())
}
