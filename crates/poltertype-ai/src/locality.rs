//! Deciding whether an endpoint keeps the request on this machine.
//!
//! `[ai].allow_remote` gates typed text *leaving the computer*. A
//! query to a model listening on loopback does not leave it, so this
//! module is what lets an offline Ollama run without the user
//! enabling network access they are not actually using.
//!
//! The rule is deliberately strict and syntactic: only literal
//! loopback addresses and the name `localhost` count. We do not
//! resolve DNS to find out where a host really points — a resolver
//! answer can change between the check and the request, and a
//! `local.mycompany.net` that resolves to 127.0.0.1 today is exactly
//! the kind of thing that should still require the user to say yes.
//! Anything we are not certain about is [`Locality::Remote`], which
//! is the answer that asks permission.

use crate::enums::Locality;

/// Classify the host of an endpoint URL.
///
/// Unparseable input is `Remote`: if we cannot tell where a request
/// goes, the honest answer is the one that needs consent.
pub fn classify(endpoint: &str) -> Locality {
    match host_of(endpoint) {
        Some(host) if is_loopback_host(&host) => Locality::Loopback,
        _ => Locality::Remote,
    }
}

/// Pull the host out of a URL without taking a URL-parsing
/// dependency: strip the scheme, then userinfo, then everything from
/// the first `/`, `?` or `#`, then the port.
fn host_of(endpoint: &str) -> Option<String> {
    let after_scheme = endpoint.split_once("://")?.1;
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .filter(|s| !s.is_empty())?;
    // `user:pass@host` — the host is what follows the LAST `@`, so a
    // password containing one cannot smuggle a different host past us.
    let hostport = authority.rsplit('@').next()?;

    // Bracketed IPv6 (`[::1]:11434`) — take what is inside the
    // brackets and ignore any port after them.
    if let Some(rest) = hostport.strip_prefix('[') {
        return rest.split(']').next().map(str::to_ascii_lowercase);
    }

    // A bare IPv6 address has several colons and no port; anything
    // with exactly one colon is host:port.
    let host = if hostport.matches(':').count() > 1 {
        hostport
    } else {
        hostport.split(':').next()?
    };
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

fn is_loopback_host(host: &str) -> bool {
    if host == "localhost" {
        return true;
    }
    // Parse as an IP and ask the standard library, so the whole
    // 127.0.0.0/8 block and every spelling of ::1 are covered without
    // us hand-rolling the ranges.
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return ip.is_loopback();
    }
    // `localhost.` and subdomains of it, per RFC 6761.
    let trimmed = host.strip_suffix('.').unwrap_or(host);
    trimmed == "localhost" || trimmed.ends_with(".localhost")
}

#[cfg(test)]
mod tests;
