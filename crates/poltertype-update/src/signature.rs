//! Ed25519 signature over the release manifest.
//!
//! The per-artifact SHA-256 proves the bytes we downloaded are the
//! bytes the manifest names. It proves nothing about who wrote the
//! manifest — it lives in the same GitHub release as the artifact, so
//! whoever can publish one can publish both. The signature is the part
//! that does not come from GitHub: made on the maintainer's machine
//! with a key CI never sees, verified against a public key compiled
//! into this binary.
//!
//! **Not the JSON is signed.** That would make the check hostage to
//! formatting, and any canonical-JSON scheme is a second specification
//! to get wrong. The payload is a flat, newline-delimited rendering of
//! the fields that carry meaning:
//!
//! ```text
//! poltertype-manifest-v1
//! schema=1
//! version=0.7.0
//! notes_url=https://github.com/…/releases/tag/v0.7.0
//! artifact=linux-x86_64
//! url=https://github.com/…/poltertype-0.7.0-x86_64.AppImage
//! sha256=9f86d081…
//! size=28311552
//! ```
//!
//! Artifacts are ordered by key so a `HashMap` cannot change the
//! payload, and every line ends with `\n`. Because `\n` is the only
//! separator, a value containing one could forge a different manifest
//! with the same payload — so any value carrying `\n` or `\r` is
//! rejected on both the signing and the verifying side. That is the
//! whole ambiguity surface of the format.
//!
//! [`consts::REQUIRE_SIGNATURE`](crate::consts) is the rollout switch:
//! while `false`, an unsigned manifest is accepted with a warning and a
//! *present* signature must still verify. Flipping it can only be done
//! once a signed release is the one users' updaters resolve to.

use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::{Signature, VerifyingKey};
use tracing::{debug, warn};

use crate::consts::{PAYLOAD_HEADER, REQUIRE_SIGNATURE, TRUSTED_PUBLIC_KEY};
use crate::enums::UpdateError;
use crate::types::Manifest;

/// Render the bytes a manifest signature covers.
///
/// Fails rather than emitting an ambiguous payload — see the module
/// docs for why a newline in a value is fatal.
pub fn signing_payload(manifest: &Manifest) -> Result<Vec<u8>, UpdateError> {
    let mut out = String::from(PAYLOAD_HEADER);
    out.push('\n');
    push_field(&mut out, "schema", &manifest.schema.to_string())?;
    push_field(&mut out, "version", &manifest.version)?;
    push_field(&mut out, "notes_url", &manifest.notes_url)?;

    // BTreeMap, not the manifest's HashMap: iteration order is part of
    // what is signed, so it has to be the key order and nothing else.
    let sorted: BTreeMap<_, _> = manifest.artifacts.iter().collect();
    for (key, artifact) in sorted {
        push_field(&mut out, "artifact", key)?;
        push_field(&mut out, "url", &artifact.url)?;
        push_field(&mut out, "sha256", &artifact.sha256)?;
        push_field(&mut out, "size", &artifact.size.to_string())?;
    }
    Ok(out.into_bytes())
}

fn push_field(out: &mut String, name: &str, value: &str) -> Result<(), UpdateError> {
    if value.contains('\n') || value.contains('\r') {
        return Err(UpdateError::UnsignablePayload(name.to_owned()));
    }
    out.push_str(name);
    out.push('=');
    out.push_str(value);
    out.push('\n');
    Ok(())
}

/// The key this build trusts, decoded from the checked-in
/// `release-signing-key.pub`.
pub(crate) fn trusted_key() -> Result<VerifyingKey, UpdateError> {
    let raw = BASE64
        .decode(TRUSTED_PUBLIC_KEY.trim())
        .map_err(|e| UpdateError::TrustedKeyBroken(e.to_string()))?;
    let bytes: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| UpdateError::TrustedKeyBroken(format!("{} bytes, want 32", raw.len())))?;
    VerifyingKey::from_bytes(&bytes).map_err(|e| UpdateError::TrustedKeyBroken(e.to_string()))
}

/// Check the manifest against the baked-in public key.
///
/// Called before anything reads a URL out of the manifest, so that
/// every decision downstream — which version, which artifact, which
/// host to fetch it from — rests on bytes we have authenticated.
pub(crate) fn verify(manifest: &Manifest) -> Result<(), UpdateError> {
    verify_with(manifest, &trusted_key()?)
}

/// [`verify`] against an explicit key.
///
/// Exists so the tests can exercise the real accept/reject path with a
/// key they own — the private half of the shipped one is on the
/// maintainer's machine and, by design, nowhere near this repository.
pub(crate) fn verify_with(manifest: &Manifest, key: &VerifyingKey) -> Result<(), UpdateError> {
    let Some(encoded) = manifest.signature.as_deref() else {
        if REQUIRE_SIGNATURE {
            return Err(UpdateError::UnsignedManifest);
        }
        // Not an error yet, but it is the thing that would become one:
        // say so at a level a bug report will carry.
        warn!(
            "release manifest carries no signature — accepted because this build \
             predates mandatory signing"
        );
        return Ok(());
    };

    let raw = BASE64
        .decode(encoded.trim())
        .map_err(|e| UpdateError::BadSignature(e.to_string()))?;
    let bytes: [u8; 64] = raw
        .as_slice()
        .try_into()
        .map_err(|_| UpdateError::BadSignature(format!("{} bytes, want 64", raw.len())))?;
    let signature = Signature::from_bytes(&bytes);

    let payload = signing_payload(manifest)?;
    key.verify_strict(&payload, &signature)
        .map_err(|e| UpdateError::BadSignature(e.to_string()))?;
    debug!("release manifest signature verified");
    Ok(())
}
