use super::*;

#[test]
fn loopback_endpoints_need_no_network_permission() {
    for url in [
        "http://127.0.0.1:11434/api/generate",
        "http://localhost:1234/v1/chat/completions",
        "http://LocalHost:8080/v1/chat/completions",
        "http://[::1]:11434/api/generate",
        "http://127.1.2.3:8080/v1",
        "https://127.0.0.1/v1/chat/completions",
        "http://localhost./v1",
        "http://ollama.localhost/v1",
        // No port, no path.
        "http://localhost",
    ] {
        assert_eq!(classify(url), Locality::Loopback, "{url} should be local");
    }
}

#[test]
fn everything_else_is_remote() {
    for url in [
        "https://api.openai.com/v1/chat/completions",
        "https://api.anthropic.com/v1/messages",
        "http://192.168.1.10:11434/api/generate",
        "http://10.0.0.5/v1",
        "http://ollama.internal:11434/api/generate",
    ] {
        assert_eq!(classify(url), Locality::Remote, "{url} should be remote");
    }
}

/// A host we cannot parse must come out `Remote` — the answer that
/// asks the user rather than the one that assumes.
#[test]
fn unparseable_endpoints_fail_closed() {
    for url in [
        "",
        "not a url",
        "127.0.0.1:11434", // no scheme — we do not guess one
        "http://",         // no authority
        "http:///v1/chat", // empty authority
        "file:///etc/passwd",
    ] {
        assert_eq!(classify(url), Locality::Remote, "{url:?} must fail closed");
    }
}

/// Userinfo must not be able to disguise the real host. `@` in a
/// password is legal, so the host is what follows the *last* one.
#[test]
fn userinfo_cannot_disguise_the_host() {
    assert_eq!(
        classify("http://localhost@evil.example.com/v1"),
        Locality::Remote,
        "the host here is evil.example.com, not localhost"
    );
    assert_eq!(
        classify("http://user:p@ss@127.0.0.1:11434/api/generate"),
        Locality::Loopback,
        "an @ inside the password must not hide a genuinely local host"
    );
}

/// A remote host that merely *contains* a loopback spelling is remote.
#[test]
fn lookalike_hosts_are_not_loopback() {
    for url in [
        "http://localhost.evil.example.com/v1",
        "http://127.0.0.1.evil.example.com/v1",
        "http://notlocalhost/v1",
        "http://localhosts/v1",
    ] {
        assert_eq!(classify(url), Locality::Remote, "{url} is not loopback");
    }
}
