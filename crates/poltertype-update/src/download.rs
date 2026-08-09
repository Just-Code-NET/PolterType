//! Downloading an artifact and proving it is the one the manifest meant.

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::consts::*;
use crate::enums::UpdateError;
use crate::types::Artifact;

/// Download `artifact` into `dir` and verify its SHA-256.
///
/// Hashing happens *while* streaming, so the installer is never held in
/// memory and no unaccounted file is ever written. The download lands
/// on a `.part` path and is renamed into place only once the hash
/// matches — so the existence of the final file is itself the proof it
/// verified, and a crash mid-download leaves nothing a later run could
/// mistake for a good artifact.
///
/// A mismatch deletes the partial file and names both digests. Loud on
/// purpose: the two ways to get here are a corrupted transfer and a
/// substituted file.
pub(crate) fn fetch_verified(artifact: &Artifact, dir: &Path) -> Result<PathBuf, UpdateError> {
    std::fs::create_dir_all(dir)?;

    if artifact.size > MAX_ARTIFACT_BYTES {
        return Err(UpdateError::TooLarge(MAX_ARTIFACT_BYTES));
    }

    let file_name = file_name_from_url(&artifact.url);
    let final_path = dir.join(&file_name);
    let part_path = dir.join(format!("{file_name}.part"));

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(MANIFEST_TIMEOUT_SECS))
        .timeout_read(Duration::from_secs(DOWNLOAD_TIMEOUT_SECS))
        .user_agent(USER_AGENT)
        .build();

    info!(url = %artifact.url, size = artifact.size, "downloading update artifact");
    let resp = agent.get(&artifact.url).call()?;

    let mut reader = resp.into_reader().take(MAX_ARTIFACT_BYTES + 1);
    let mut hasher = Sha256::new();
    let mut written: u64 = 0;

    {
        let mut out = BufWriter::new(File::create(&part_path)?);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            written += n as u64;
            if written > MAX_ARTIFACT_BYTES {
                drop(out);
                let _ = std::fs::remove_file(&part_path);
                return Err(UpdateError::TooLarge(MAX_ARTIFACT_BYTES));
            }
            hasher.update(&buf[..n]);
            out.write_all(&buf[..n])?;
        }
        out.flush()?;
    }

    let actual = hex(&hasher.finalize());
    let expected = artifact.sha256.trim().to_ascii_lowercase();
    if actual != expected {
        warn!(
            %expected,
            %actual,
            path = ?part_path,
            "downloaded artifact failed checksum verification; discarding"
        );
        let _ = std::fs::remove_file(&part_path);
        return Err(UpdateError::ChecksumMismatch { expected, actual });
    }

    // Rename last: from here on, the file's presence means "verified".
    std::fs::rename(&part_path, &final_path)?;
    info!(path = ?final_path, bytes = written, "update artifact verified");
    Ok(final_path)
}

/// Last path segment of the URL, sanitised down to a plain file name.
///
/// The URL comes from the manifest, so treat it as untrusted input: a
/// crafted `url` ending in `../../autostart/evil.desktop` must not let
/// the download escape the staging directory. Taking only the final
/// component and rejecting anything that still looks like a path
/// keeps the write where we put it.
pub(crate) fn file_name_from_url(url: &str) -> String {
    let tail = url
        .rsplit('/')
        .next()
        .unwrap_or("")
        .split(['?', '#'])
        .next()
        .unwrap_or("");
    let clean: String = tail
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
        .collect();
    // `..` survives the filter above (both chars are allowed), so reject
    // the traversal spelling explicitly rather than trusting the charset.
    if clean.is_empty() || clean.starts_with('.') || clean.contains("..") {
        return "poltertype-update".to_owned();
    }
    clean
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        // Writing to a String is infallible; the Result exists only to
        // satisfy the `Write` trait.
        let _ = write!(s, "{b:02x}");
        s
    })
}
