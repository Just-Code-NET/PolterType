//! Unit tests.
//!
//! Most of these are about decisions — is this newer, is this file name
//! safe, do we understand this schema. The download tests are different:
//! they run a real HTTP server on loopback and pull a real file through
//! `ureq`, because the property that matters most in this crate is that
//! **a file whose checksum doesn't match never survives on disk**, and
//! that is not something to assert about mocked bytes.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use crate::consts::SUPPORTED_SCHEMA;
use crate::download::{fetch_verified, file_name_from_url};
use crate::enums::UpdateError;
use crate::manifest::platform_key;
use crate::types::{Artifact, Manifest};
use crate::version::is_newer;

// ─── Version comparison ───────────────────────────────────────────────

#[test]
fn newer_patch_minor_and_major_are_all_updates() {
    assert!(is_newer("0.3.2", "0.3.1").unwrap());
    assert!(is_newer("0.4.0", "0.3.1").unwrap());
    assert!(is_newer("1.0.0", "0.3.1").unwrap());
}

#[test]
fn same_version_is_not_an_update() {
    assert!(!is_newer("0.3.1", "0.3.1").unwrap());
}

/// A user running a build newer than the published release — a local
/// `cargo build` off `main`, or someone who kept a release candidate —
/// must never be dragged backwards.
#[test]
fn older_published_version_never_downgrades_us() {
    assert!(!is_newer("0.3.0", "0.3.1").unwrap());
    assert!(!is_newer("0.3.1", "0.4.0-rc.1").unwrap());
}

/// Semver's pre-release ordering is what we want and get for free:
/// an rc is older than its final, so an rc user is offered the final
/// and a final user is not offered the rc.
#[test]
fn prerelease_orders_below_its_final_release() {
    assert!(is_newer("0.4.0", "0.4.0-rc.1").unwrap());
    assert!(!is_newer("0.4.0-rc.1", "0.4.0").unwrap());
    assert!(is_newer("0.4.0-rc.2", "0.4.0-rc.1").unwrap());
}

/// The git tag carries a `v`; the manifest is not supposed to, but a
/// hand-written one easily might. Accept both rather than skip an
/// update over a prefix.
#[test]
fn leading_v_is_tolerated() {
    assert!(is_newer("v0.4.0", "0.3.1").unwrap());
}

/// A version we cannot parse is an error, not a shrug. Guessing would
/// mean either installing something whose age we don't know, or
/// silently never updating again.
#[test]
fn unparseable_version_is_an_error() {
    let err = is_newer("latest", "0.3.1").unwrap_err();
    assert!(matches!(err, UpdateError::BadVersion(v, _) if v == "latest"));
}

/// The version we compare everything against is the *running* binary's,
/// so it must be real semver — otherwise every check errors out.
#[test]
fn our_own_version_is_semver() {
    assert!(semver::Version::parse(crate::current_version()).is_ok());
}

// ─── Manifest ─────────────────────────────────────────────────────────

const SAMPLE: &str = r#"{
  "schema": 1,
  "version": "0.4.0",
  "notes_url": "https://github.com/Just-Code-NET/PolterType/releases/tag/v0.4.0",
  "artifacts": {
    "linux-x86_64": {
      "url": "https://github.com/Just-Code-NET/PolterType/releases/download/v0.4.0/poltertype-0.4.0-x86_64.AppImage",
      "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
      "size": 28311552
    }
  }
}"#;

#[test]
fn parses_the_manifest_the_release_workflow_writes() {
    let m: Manifest = serde_json::from_str(SAMPLE).unwrap();
    assert_eq!(m.schema, SUPPORTED_SCHEMA);
    assert_eq!(m.version, "0.4.0");
    assert_eq!(m.artifacts["linux-x86_64"].size, 28_311_552);
    // An unsigned manifest must still *parse*; refusing it is
    // `verify`'s job, not the deserialiser's.
    assert!(m.signature.is_none());
}

/// Parsing must not depend on `REQUIRE_SIGNATURE`, so the rollout
/// switch can be flipped either way without a schema break.
#[test]
fn a_signed_manifest_still_parses_in_an_unsigning_build() {
    let signed = SAMPLE.replace(r#""schema": 1,"#, r#""schema": 1, "signature": "abc123","#);
    let m: Manifest = serde_json::from_str(&signed).unwrap();
    assert_eq!(m.signature.as_deref(), Some("abc123"));
}

// ─── Manifest signature ───────────────────────────────────────────────

/// A keypair the tests own. The shipped public key's private half
/// lives on the maintainer's machine and must never be reachable from
/// here, so everything except "is the shipped key well-formed" is
/// exercised with this one.
fn test_keypair() -> (ed25519_dalek::SigningKey, ed25519_dalek::VerifyingKey) {
    let signing = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
    let verifying = signing.verifying_key();
    (signing, verifying)
}

fn sign(manifest: &mut Manifest, key: &ed25519_dalek::SigningKey) {
    use base64::Engine;
    use ed25519_dalek::Signer;
    let payload = crate::signature::signing_payload(manifest).unwrap();
    let sig = key.sign(&payload);
    manifest.signature = Some(base64::engine::general_purpose::STANDARD.encode(sig.to_bytes()));
}

fn sample() -> Manifest {
    serde_json::from_str(SAMPLE).unwrap()
}

/// The key we ship has to be a key. Nothing at runtime can tell us
/// otherwise in a useful way — a broken one turns every update check
/// into `TrustedKeyBroken` — so it is checked at build time here.
#[test]
fn the_public_key_built_into_this_binary_is_usable() {
    crate::signature::trusted_key().expect("release-signing-key.pub must be a valid ed25519 key");
}

/// The exact bytes that get signed. Pinned deliberately: changing this
/// rendering invalidates every signature ever made, so it must be a
/// decision someone takes on purpose and not a side effect of tidying
/// `signing_payload`. If this test fails, the format changed — bump
/// the header string and plan a rollout, don't update the expectation.
#[test]
fn the_signed_payload_has_exactly_this_shape() {
    let payload = crate::signature::signing_payload(&sample()).unwrap();
    assert_eq!(
        String::from_utf8(payload).unwrap(),
        "poltertype-manifest-v1\n\
         schema=1\n\
         version=0.4.0\n\
         notes_url=https://github.com/Just-Code-NET/PolterType/releases/tag/v0.4.0\n\
         artifact=linux-x86_64\n\
         url=https://github.com/Just-Code-NET/PolterType/releases/download/v0.4.0/poltertype-0.4.0-x86_64.AppImage\n\
         sha256=9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08\n\
         size=28311552\n"
    );
}

/// `artifacts` is a `HashMap`, whose iteration order is randomised per
/// process. If that order reached the payload, a manifest would verify
/// or not depending on the run.
#[test]
fn artifact_order_cannot_change_the_payload() {
    let mut a = sample();
    a.artifacts.insert(
        "windows-x86_64".to_owned(),
        Artifact {
            url: "https://example.com/a.msi".to_owned(),
            sha256: "0".repeat(64),
            size: 1,
        },
    );
    a.artifacts.insert(
        "macos-universal".to_owned(),
        Artifact {
            url: "https://example.com/a.dmg".to_owned(),
            sha256: "1".repeat(64),
            size: 2,
        },
    );

    // Same content, inserted in the opposite order.
    let mut b = sample();
    let win = a.artifacts["windows-x86_64"].clone();
    let mac = a.artifacts["macos-universal"].clone();
    b.artifacts.insert("macos-universal".to_owned(), mac);
    b.artifacts.insert("windows-x86_64".to_owned(), win);

    assert_eq!(
        crate::signature::signing_payload(&a).unwrap(),
        crate::signature::signing_payload(&b).unwrap()
    );
}

#[test]
fn a_manifest_signed_with_the_right_key_verifies() {
    let (signing, verifying) = test_keypair();
    let mut m = sample();
    sign(&mut m, &signing);
    crate::signature::verify_with(&m, &verifying).expect("our own signature must verify");
}

/// The attack the signature exists to stop: someone who can publish a
/// GitHub release swaps the download URL (and its matching checksum,
/// which they can also compute) for one of their own.
#[test]
fn swapping_the_download_url_invalidates_the_signature() {
    let (signing, verifying) = test_keypair();
    let mut m = sample();
    sign(&mut m, &signing);

    let artifact = m.artifacts.get_mut("linux-x86_64").unwrap();
    artifact.url = "https://evil.example/poltertype-0.4.0-x86_64.AppImage".to_owned();
    artifact.sha256 = "f".repeat(64);

    let err = crate::signature::verify_with(&m, &verifying)
        .expect_err("a re-pointed artifact must not verify");
    assert!(matches!(err, UpdateError::BadSignature(_)), "{err:?}");
}

#[test]
fn a_signature_from_another_key_is_refused() {
    let (_, verifying) = test_keypair();
    let attacker = ed25519_dalek::SigningKey::from_bytes(&[9u8; 32]);
    let mut m = sample();
    sign(&mut m, &attacker);

    let err = crate::signature::verify_with(&m, &verifying)
        .expect_err("a stranger's signature must not verify");
    assert!(matches!(err, UpdateError::BadSignature(_)), "{err:?}");
}

#[test]
fn a_signature_that_is_not_even_base64_is_an_error_not_a_panic() {
    let (_, verifying) = test_keypair();
    for junk in ["", "not base64!!", "YWJj"] {
        let mut m = sample();
        m.signature = Some(junk.to_owned());
        let err = crate::signature::verify_with(&m, &verifying).expect_err("must reject `{junk}`");
        assert!(matches!(err, UpdateError::BadSignature(_)), "{err:?}");
    }
}

/// `\n` is the payload's only separator, so a value containing one
/// could describe two different manifests with the same bytes. Both
/// sides refuse rather than sign or accept something ambiguous.
#[test]
fn a_line_break_in_a_field_is_refused_instead_of_signed() {
    let mut m = sample();
    m.version = "0.4.0\nversion=9.9.9".to_owned();
    let err = crate::signature::signing_payload(&m).expect_err("must refuse an ambiguous payload");
    assert!(
        matches!(err, UpdateError::UnsignablePayload(f) if f == "version"),
        "wrong field blamed"
    );

    let mut m = sample();
    m.artifacts.get_mut("linux-x86_64").unwrap().url = "https://ok/\nsize=1".to_owned();
    let err = crate::signature::signing_payload(&m).expect_err("must refuse an ambiguous payload");
    assert!(matches!(err, UpdateError::UnsignablePayload(f) if f == "url"));
}

/// The rollout hinge. While `REQUIRE_SIGNATURE` is false an unsigned
/// manifest still works — every user on an older release depends on
/// that — but a *wrong* signature is refused from day one.
#[test]
fn an_unsigned_manifest_is_accepted_only_while_signing_is_optional() {
    let (_, verifying) = test_keypair();
    let m = sample();
    assert!(m.signature.is_none());
    let result = crate::signature::verify_with(&m, &verifying);
    if crate::consts::REQUIRE_SIGNATURE {
        assert!(matches!(result, Err(UpdateError::UnsignedManifest)));
    } else {
        assert!(result.is_ok(), "{result:?}");
    }
}

#[test]
fn platform_key_matches_a_key_the_release_workflow_publishes() {
    let key = platform_key();
    assert!(
        [
            "windows-x86_64",
            "macos-universal",
            "linux-x86_64",
            "linux-aarch64",
        ]
        .contains(&key.as_str()),
        "platform_key() produced `{key}`, which release.yml does not publish an artifact for"
    );
}

// ─── Download: file name sanitisation ─────────────────────────────────

#[test]
fn artifact_file_name_comes_from_the_url_tail() {
    assert_eq!(
        file_name_from_url("https://example.com/a/b/poltertype-0.4.0-x86_64.AppImage"),
        "poltertype-0.4.0-x86_64.AppImage"
    );
}

#[test]
fn query_and_fragment_are_stripped() {
    assert_eq!(
        file_name_from_url("https://example.com/app.msi?token=abc#frag"),
        "app.msi"
    );
}

/// The URL is manifest-supplied, i.e. untrusted. A tail that tries to
/// climb out of the staging directory must not become a write outside
/// it — the whole point of staging is that we control where the bytes
/// land.
#[test]
fn path_traversal_in_the_url_cannot_escape_the_staging_dir() {
    for hostile in [
        "https://example.com/../../.config/autostart/evil.desktop",
        "https://example.com/..",
        "https://example.com/....//evil",
        "https://example.com/",
        "https://example.com/.bashrc",
    ] {
        let name = file_name_from_url(hostile);
        assert!(
            !name.contains("..") && !name.contains('/') && !name.starts_with('.'),
            "`{hostile}` produced the unsafe file name `{name}`"
        );
    }
}

// ─── Download + checksum, over a real socket ──────────────────────────

/// Serve `body` once at `/<name>` and return the URL to it.
///
/// A hand-rolled server rather than a mocking crate: the whole point is
/// to exercise the real `ureq` read path, and 20 lines of `TcpListener`
/// buy that without adding a dev-dependency to a crate whose dependency
/// surface we deliberately keep small.
fn serve_once(name: &str, body: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local addr").port();

    thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            // Read (and ignore) the request head. We serve exactly one
            // thing, so there is nothing to route on.
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf);

            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\n\r\n",
                body.len()
            );
            let _ = sock.write_all(head.as_bytes());
            let _ = sock.write_all(&body);
            let _ = sock.flush();
        }
    });

    format!("http://127.0.0.1:{port}/{name}")
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn a_download_whose_checksum_matches_lands_on_disk_intact() {
    let body = b"PolterType installer payload".to_vec();
    let url = serve_once("poltertype-0.4.0-x86_64.AppImage", body.clone());
    let dir = std::env::temp_dir().join("poltertype-update-test-ok");
    let _ = std::fs::remove_dir_all(&dir);

    let artifact = Artifact {
        url,
        sha256: sha256_hex(&body),
        size: body.len() as u64,
    };

    let path = fetch_verified(&artifact, &dir).expect("verified download");
    assert_eq!(std::fs::read(&path).expect("read back"), body);
    assert_eq!(
        path.file_name().and_then(|n| n.to_str()),
        Some("poltertype-0.4.0-x86_64.AppImage")
    );
    // The `.part` file is an implementation detail that must not outlive
    // a successful download — a stray one would be mistaken for a
    // resumable transfer by any future change to this code.
    assert!(!dir.join("poltertype-0.4.0-x86_64.AppImage.part").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

/// The load-bearing test of this crate. A payload that does not hash to
/// what the manifest promised is either corrupt or substituted, and in
/// both cases it must be **gone** — not quarantined, not left as a
/// `.part` for someone to find, and above all not returned to a caller
/// who would hand it to `msiexec`.
#[test]
fn a_download_whose_checksum_is_wrong_is_deleted_and_never_returned() {
    let body = b"a tampered installer".to_vec();
    let url = serve_once("poltertype-0.4.0-x86_64.AppImage", body.clone());
    let dir = std::env::temp_dir().join("poltertype-update-test-bad");
    let _ = std::fs::remove_dir_all(&dir);

    let artifact = Artifact {
        url,
        // What the (honest) manifest said the file would hash to.
        sha256: sha256_hex(b"the installer we actually published"),
        size: body.len() as u64,
    };

    let err = fetch_verified(&artifact, &dir).expect_err("must reject a checksum mismatch");
    assert!(
        matches!(err, UpdateError::ChecksumMismatch { .. }),
        "{err:?}"
    );

    // Nothing usable may remain: no final file, no partial.
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    assert!(
        leftovers.is_empty(),
        "a rejected download left files behind: {leftovers:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The manifest is not trusted to be sane. An artifact claiming to be
/// bigger than our ceiling is refused before a single byte is fetched.
#[test]
fn an_oversized_artifact_is_refused_before_downloading() {
    let dir = std::env::temp_dir().join("poltertype-update-test-huge");
    let artifact = Artifact {
        // Deliberately unreachable: rejecting on `size` must happen
        // before anything tries to connect.
        url: "http://127.0.0.1:1/nope".to_owned(),
        sha256: "0".repeat(64),
        size: crate::consts::MAX_ARTIFACT_BYTES + 1,
    };
    let err = fetch_verified(&artifact, &dir).expect_err("must refuse an oversized artifact");
    assert!(matches!(err, UpdateError::TooLarge(_)), "{err:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
