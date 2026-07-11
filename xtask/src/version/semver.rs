//! Version-string parsing and bumping.

use super::*;
use anyhow::{Context, Result, bail};

pub(crate) fn parse(s: &str) -> Result<Version> {
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (s, None),
    };
    let mut parts = core.split('.');
    let major: u64 = parts
        .next()
        .context("missing MAJOR")?
        .parse()
        .with_context(|| format!("MAJOR is not a number in `{s}`"))?;
    let minor: u64 = parts
        .next()
        .with_context(|| format!("missing MINOR in `{s}`"))?
        .parse()
        .with_context(|| format!("MINOR is not a number in `{s}`"))?;
    let patch: u64 = parts
        .next()
        .with_context(|| format!("missing PATCH in `{s}`"))?
        .parse()
        .with_context(|| format!("PATCH is not a number in `{s}`"))?;
    if parts.next().is_some() {
        bail!("`{s}` has a fourth dotted segment — only MAJOR.MINOR.PATCH is supported");
    }
    let pre = match pre {
        None => None,
        Some(p) => {
            let (word, counter) = p
                .split_once('.')
                .with_context(|| format!("pre-release `{p}` must be `<word>.<counter>`"))?;
            if word.is_empty() || !word.chars().all(|c| c.is_ascii_alphabetic()) {
                bail!("pre-release word in `{s}` must be ASCII letters (e.g. `alpha`, `beta`)");
            }
            let counter: u64 = counter
                .parse()
                .with_context(|| format!("pre-release counter in `{s}` is not a number"))?;
            Some(PreRelease {
                word: word.to_owned(),
                counter,
            })
        }
    };
    Ok(Version {
        major,
        minor,
        patch,
        pre,
    })
}

pub(crate) fn bump(s: &str) -> Result<String> {
    let mut v = parse(s)?;
    if let Some(p) = &mut v.pre {
        p.counter += 1;
    } else {
        v.patch += 1;
    }
    Ok(v.to_string())
}
