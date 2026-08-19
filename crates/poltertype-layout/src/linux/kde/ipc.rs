//! qdbus invocation.

use super::*;
use crate::linux::shared::{bcp47_to_xkb, cmd_exists, xkb_to_bcp47};
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use std::process::Command;
use tracing::{debug, warn};

pub(crate) const SERVICE: &str = "org.kde.keyboard";
pub(crate) const OBJECT: &str = "/Layouts";

pub(crate) fn run(prog: &str, args: &[&str]) -> Result<String, LayoutError> {
    let out = Command::new(prog)
        .args(args)
        .output()
        .map_err(|e| LayoutError::Os(format!("{prog}: {e}")))?;
    if !out.status.success() {
        return Err(LayoutError::Os(format!(
            "{prog} {args:?} exited {}",
            out.status
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `run` with `--literal`, for calls whose return type qdbus refuses to
/// pretty-print. Without it `getLayoutsList` answers with the sentence
/// "I don't know how to display an argument of type 'a(sss)'" **on
/// stdout, exit code 0** — indistinguishable from a real answer to
/// [`run`]. See [`super::parse::layout_short_names`] for the shapes.
pub(crate) fn run_literal(prog: &str, args: &[&str]) -> Result<String, LayoutError> {
    let mut argv = vec!["--literal"];
    argv.extend_from_slice(args);
    run(prog, &argv)
}
