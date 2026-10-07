//! One leaked copy of each FST file for the life of the process.
//!
//! The FST reader wants `&'static [u8]`, so the bytes are leaked. Every
//! dictionary rebuild — a profile cache, a Settings window closing,
//! *Reload Settings* — used to read and leak the same files again, and
//! memory grew by the whole wordlist set each time (issue #76). Keyed by
//! size and mtime as well as path, so a pack replaced on disk is read
//! anew rather than served stale.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::SystemTime;

use parking_lot::Mutex;

struct Entry {
    len: u64,
    modified: Option<SystemTime>,
    bytes: &'static [u8],
}

static CACHE: LazyLock<Mutex<HashMap<PathBuf, Entry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The file's bytes, read and leaked on first use and shared after.
pub(crate) fn leaked_bytes(path: &Path) -> std::io::Result<&'static [u8]> {
    let meta = std::fs::metadata(path)?;
    let (len, modified) = (meta.len(), meta.modified().ok());
    let mut cache = CACHE.lock();
    if let Some(e) = cache.get(path) {
        if e.len == len && e.modified == modified {
            return Ok(e.bytes);
        }
    }
    let bytes: &'static [u8] = Box::leak(std::fs::read(path)?.into_boxed_slice());
    cache.insert(
        path.to_path_buf(),
        Entry {
            len,
            modified,
            bytes,
        },
    );
    Ok(bytes)
}
