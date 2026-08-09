//! `cargo xtask manifest` — sign and inspect the release manifest.
//!
//! The updater verifies `latest.json` against a public key compiled
//! into the binary; this is the other half, run **on the maintainer's
//! machine** with a private key deliberately not available to CI.
//!
//! That asymmetry is the entire security gain. A key stored as an
//! Actions secret would be reachable by anyone who can compromise the
//! GitHub account — exactly the attacker the signature is meant to
//! stop. Signing by hand, between the draft CI produces and the moment
//! a human publishes it, keeps the key on hardware GitHub never
//! touches.
//!
//! The signed bytes come from `poltertype_update::signing_payload`, the
//! same function the app verifies with, so the two ends cannot drift.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use poltertype_update::{Manifest, signing_payload};

/// Where `keygen` puts a new keypair unless told otherwise. Outside the
/// repository and outside the app's own config directory: a signing key
/// that can be committed by accident, or wiped by "reset my settings",
/// is not a signing key.
const DEFAULT_KEY_DIR: &str = ".config/poltertype-signing";

/// Env var read when `--key` is not given, so the key can come from a
/// password manager (`POLTERTYPE_SIGNING_KEY=$(pass …)`) instead of
/// sitting on disk at all.
const KEY_ENV: &str = "POLTERTYPE_SIGNING_KEY";

pub fn run(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("keygen") => keygen(&args[1..]),
        Some("sign") => sign(&args[1..]),
        Some("verify") => verify(&args[1..]),
        Some("payload") => payload(&args[1..]),
        Some(other) => bail!("unknown `manifest` subcommand: {other}"),
        None => {
            print_help();
            Ok(())
        }
    }
}

fn print_help() {
    println!("cargo xtask manifest <subcommand>");
    println!();
    println!("  keygen [--dir DIR]      Create a release signing keypair (refuses to overwrite).");
    println!("  sign <latest.json> [--key FILE]");
    println!("                          Sign in place. Key comes from --key, else ${KEY_ENV}.");
    println!("  verify <latest.json> [--key PUBFILE]");
    println!("                          Check against the key shipped in poltertype-update.");
    println!("  payload <latest.json>   Print the exact bytes a signature covers.");
}

// ─── Subcommands ──────────────────────────────────────────────────────

fn keygen(args: &[String]) -> Result<()> {
    let dir = match flag(args, "--dir") {
        Some(d) => PathBuf::from(d),
        None => home()?.join(DEFAULT_KEY_DIR),
    };
    let secret_path = dir.join("release.key");
    let public_path = dir.join("release.pub");

    // Never clobber: overwriting a signing key silently orphans every
    // release ever signed with it.
    if secret_path.exists() || public_path.exists() {
        bail!(
            "{} already holds a keypair — refusing to overwrite it. \
             Delete it deliberately, or pass --dir.",
            dir.display()
        );
    }

    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed)
        .map_err(|e| anyhow::anyhow!("read 32 bytes from the OS random source: {e}"))?;
    let signing = SigningKey::from_bytes(&seed);
    let verifying = signing.verifying_key();

    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    write_private(&secret_path, &format!("{}\n", BASE64.encode(seed)))?;
    std::fs::write(
        &public_path,
        format!("{}\n", BASE64.encode(verifying.to_bytes())),
    )?;

    println!("private key: {}", secret_path.display());
    println!("public key:  {}", public_path.display());
    println!();
    println!("Put the public key in crates/poltertype-update/release-signing-key.pub");
    println!("and ship a release built with it BEFORE signing anything users will see:");
    println!();
    println!("  {}", BASE64.encode(verifying.to_bytes()));
    Ok(())
}

fn sign(args: &[String]) -> Result<()> {
    let path = positional(args, "manifest path")?;
    let mut manifest = read_manifest(&path)?;

    let signing = load_signing_key(flag(args, "--key"))?;
    // Signing a manifest that already carries a signature is fine — the
    // field is not part of the payload — but say so, because it usually
    // means someone is re-signing after an edit and wants to be sure the
    // old one is gone.
    if manifest.signature.is_some() {
        println!(
            "note: replacing the signature already in {}",
            path.display()
        );
    }
    manifest.signature = None;

    let payload = signing_payload(&manifest).context("render the signed payload")?;
    let signature = signing.sign(&payload);
    manifest.signature = Some(BASE64.encode(signature.to_bytes()));

    let json = serde_json::to_string_pretty(&manifest).context("serialise the signed manifest")?;
    std::fs::write(&path, format!("{json}\n"))
        .with_context(|| format!("write {}", path.display()))?;

    // Verify what we just wrote, from disk, against the key the app
    // ships. Signing something the released binaries cannot check is
    // the one failure mode worth an extra read.
    verify_file(&path, None)?;
    println!("signed and verified: {}", path.display());
    Ok(())
}

fn verify(args: &[String]) -> Result<()> {
    let path = positional(args, "manifest path")?;
    verify_file(&path, flag(args, "--key"))?;
    println!("signature OK: {}", path.display());
    Ok(())
}

fn payload(args: &[String]) -> Result<()> {
    let path = positional(args, "manifest path")?;
    let mut manifest = read_manifest(&path)?;
    manifest.signature = None;
    let bytes = signing_payload(&manifest).context("render the signed payload")?;
    print!(
        "{}",
        String::from_utf8(bytes).context("payload is not UTF-8")?
    );
    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────────────────

fn verify_file(path: &Path, key_path: Option<&str>) -> Result<()> {
    let manifest = read_manifest(path)?;
    let Some(encoded) = manifest.signature.as_deref() else {
        bail!("{} carries no signature", path.display());
    };
    let raw = BASE64
        .decode(encoded.trim())
        .context("signature is not base64")?;
    let bytes: [u8; 64] = raw
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("signature is {} bytes, want 64", raw.len()))?;

    let verifying = load_verifying_key(key_path)?;
    let mut unsigned = manifest.clone();
    unsigned.signature = None;
    let payload = signing_payload(&unsigned).context("render the signed payload")?;
    verifying
        .verify_strict(&payload, &Signature::from_bytes(&bytes))
        .context("signature does not match this manifest")?;
    Ok(())
}

/// The 32-byte seed, base64, from `--key` or the environment.
fn load_signing_key(path: Option<&str>) -> Result<SigningKey> {
    let encoded = match path {
        Some(p) => std::fs::read_to_string(p).with_context(|| format!("read key file {p}"))?,
        None => std::env::var(KEY_ENV)
            .map_err(|_| anyhow::anyhow!("no signing key: pass --key FILE or set {KEY_ENV}"))?,
    };
    let raw = BASE64
        .decode(encoded.trim())
        .context("signing key is not base64")?;
    let seed: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("signing key is {} bytes, want 32", raw.len()))?;
    Ok(SigningKey::from_bytes(&seed))
}

/// The public key to check against — by default the one the app ships,
/// because "does this verify for users" is the only question that
/// matters.
fn load_verifying_key(path: Option<&str>) -> Result<VerifyingKey> {
    let encoded = match path {
        Some(p) => std::fs::read_to_string(p).with_context(|| format!("read key file {p}"))?,
        None => include_str!("../../crates/poltertype-update/release-signing-key.pub").to_owned(),
    };
    let raw = BASE64
        .decode(encoded.trim())
        .context("public key is not base64")?;
    let bytes: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("public key is {} bytes, want 32", raw.len()))?;
    VerifyingKey::from_bytes(&bytes).context("not a valid ed25519 public key")
}

fn read_manifest(path: &Path) -> Result<Manifest> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("{} is not a release manifest", path.display()))
}

fn positional(args: &[String], what: &str) -> Result<PathBuf> {
    args.iter()
        .find(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("missing {what}"))
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a == name)?;
    args.get(i + 1).map(String::as_str)
}

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("$HOME is not set; pass --dir"))
}

/// Create the file with owner-only permissions *before* the secret goes
/// into it — writing first and chmod-ing after leaves a window where the
/// key is world-readable.
#[cfg(unix)]
fn write_private(path: &Path, contents: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    f.write_all(contents.as_bytes())?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private(path: &Path, contents: &str) -> Result<()> {
    // No mode bits to set; the file inherits the directory's ACL, and
    // the recommended directory is under the user profile.
    std::fs::write(path, contents).with_context(|| format!("create {}", path.display()))
}
