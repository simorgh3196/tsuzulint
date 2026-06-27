//! URL safety guard for dictionary fetches — SSRF hardening at the boundary.
//!
//! Network acquisition of a morphology dictionary ([`provision_dictionary_from_url`]) hands an
//! operator-configured URL to a host-side HTTP client. Before any request is made, the URL is run
//! through [`validate_dictionary_url`], a pure (no-I/O, `wasm32`-clean) guard that rejects the
//! obvious SSRF vectors so the linter cannot be coaxed into probing internal services:
//!
//! - **scheme** must be `https` (no `http`/`file`/`ftp`/… and no cleartext);
//! - **no credentials** in the authority (`user:pass@host` is refused);
//! - **literal IP hosts** in loopback / private / link-local / unique-local / multicast /
//!   unspecified / reserved ranges (IPv4 and IPv6, including IPv4-mapped IPv6) are refused;
//! - the `localhost` name (and `*.localhost`) is refused.
//!
//! What it deliberately does **not** do: resolve DNS. A hostname that *resolves* to an internal
//! address (DNS-rebinding) is not caught here — that check needs the resolved socket address and
//! therefore belongs to the host-side fetch, alongside connect/read timeouts. The pinned-hash
//! verification in [`crate::dict`] is the backstop: even a successful fetch of the wrong endpoint
//! cannot pass `blake3 == pin`, so the residual risk is "use as a request proxy", which the
//! literal-IP and `localhost` blocks already cover for the common cases. Resolved-address
//! re-validation is a documented follow-up on the native fetch.
//!
//! [`provision_dictionary_from_url`]: crate::dict::provision_dictionary_from_url

use std::net::{Ipv4Addr, Ipv6Addr};

use url::{Host as UrlHost, Url};

/// Why a candidate dictionary URL was rejected by [`validate_dictionary_url`].
#[derive(Debug)]
pub enum UrlPolicyError {
    /// The string is not a valid absolute URL.
    Parse(String),
    /// The scheme is not `https`.
    NotHttps,
    /// The authority embeds a username and/or password (`user:pass@host`).
    HasCredentials,
    /// The URL has no host component.
    MissingHost,
    /// The host is not allowed to be fetched: the `localhost` name or a literal IP address in a
    /// loopback / private / link-local / unique-local / multicast / unspecified / reserved range.
    BlockedHost(String),
}

impl core::fmt::Display for UrlPolicyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            UrlPolicyError::Parse(reason) => write!(f, "dictionary URL is invalid: {reason}"),
            UrlPolicyError::NotHttps => write!(f, "dictionary URL must use https"),
            UrlPolicyError::HasCredentials => {
                write!(f, "dictionary URL must not embed credentials")
            }
            UrlPolicyError::MissingHost => write!(f, "dictionary URL has no host"),
            UrlPolicyError::BlockedHost(host) => {
                write!(
                    f,
                    "dictionary URL host is not allowed to be fetched: {host}"
                )
            }
        }
    }
}

impl std::error::Error for UrlPolicyError {}

/// Validate a candidate dictionary URL, returning the parsed [`Url`] when it is safe to fetch.
///
/// See the [module docs](self) for the policy and its limits.
pub fn validate_dictionary_url(raw: &str) -> Result<Url, UrlPolicyError> {
    let url = Url::parse(raw).map_err(|e| UrlPolicyError::Parse(e.to_string()))?;
    if url.scheme() != "https" {
        return Err(UrlPolicyError::NotHttps);
    }
    // `username()` is empty and `password()` is `None` when no credentials are present.
    if !url.username().is_empty() || url.password().is_some() {
        return Err(UrlPolicyError::HasCredentials);
    }
    match url.host() {
        None => return Err(UrlPolicyError::MissingHost),
        Some(UrlHost::Domain(name)) if is_blocked_domain(name) => {
            return Err(UrlPolicyError::BlockedHost(name.to_string()));
        }
        Some(UrlHost::Ipv4(addr)) if ipv4_is_blocked(addr) => {
            return Err(UrlPolicyError::BlockedHost(addr.to_string()));
        }
        Some(UrlHost::Ipv6(addr)) if ipv6_is_blocked(addr) => {
            return Err(UrlPolicyError::BlockedHost(addr.to_string()));
        }
        Some(_) => {}
    }
    Ok(url)
}

/// Whether a domain name resolves *by name* to the local host (we cannot resolve DNS here, so this
/// only catches the literal `localhost` family — see the [module docs](self) on what is left to the
/// host-side fetch).
///
/// A single trailing dot is stripped first: the WHATWG parser keeps it on a domain (`localhost.` →
/// `Domain("localhost.")`), but the fully-qualified `localhost.` still resolves to the loopback
/// host, so it must match the same rule (otherwise it is a trivial bypass).
fn is_blocked_domain(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let normalized = lower.strip_suffix('.').unwrap_or(&lower);
    normalized == "localhost" || normalized.ends_with(".localhost")
}

/// Whether an IPv4 literal is in a range that must not be fetched (loopback / private / link-local /
/// CGNAT-shared / unspecified / broadcast / documentation / multicast / reserved).
fn ipv4_is_blocked(addr: Ipv4Addr) -> bool {
    let [a, b, _, _] = addr.octets();
    addr.is_loopback()        // 127.0.0.0/8
        || addr.is_private()  // 10/8, 172.16/12, 192.168/16
        || addr.is_link_local() // 169.254.0.0/16
        || addr.is_unspecified() // 0.0.0.0
        || addr.is_broadcast()   // 255.255.255.255
        || addr.is_documentation() // 192.0.2/24, 198.51.100/24, 203.0.113/24
        || addr.is_multicast()     // 224.0.0.0/4
        || a == 0                  // 0.0.0.0/8 ("this network")
        || (a == 100 && (64..=127).contains(&b)) // 100.64.0.0/10 (CGNAT shared)
        || a >= 240 // 240.0.0.0/4 (reserved, includes the 255/8 broadcast block)
}

/// Extracts an embedded IPv4 address from transition-form IPv6 addresses.
///
/// Handles:
/// - IPv4-mapped and IPv4-compatible (`::ffff:a.b.c.d` and `::a.b.c.d` via `to_ipv4`)
/// - NAT64 well-known prefix (`64:ff9b::a.b.c.d` — RFC 6052)
/// - 6to4 (`2002:abcd:efgh::` — RFC 3056)
fn extract_ipv4(addr: Ipv6Addr) -> Option<Ipv4Addr> {
    if let Some(v4) = addr.to_ipv4() {
        return Some(v4);
    }
    let segments = addr.segments();
    // NAT64 (RFC 6052) Well-Known Prefix: 64:ff9b::/96
    if segments[0] == 0x0064
        && segments[1] == 0xff9b
        && segments[2] == 0
        && segments[3] == 0
        && segments[4] == 0
        && segments[5] == 0
    {
        let a = (segments[6] >> 8) as u8;
        let b = (segments[6] & 0xff) as u8;
        let c = (segments[7] >> 8) as u8;
        let d = (segments[7] & 0xff) as u8;
        return Some(Ipv4Addr::new(a, b, c, d));
    }
    // 6to4 (RFC 3056): 2002::/16
    if segments[0] == 0x2002 {
        let a = (segments[1] >> 8) as u8;
        let b = (segments[1] & 0xff) as u8;
        let c = (segments[2] >> 8) as u8;
        let d = (segments[2] & 0xff) as u8;
        return Some(Ipv4Addr::new(a, b, c, d));
    }
    // Teredo (RFC 4380): 2001::/32
    if segments[0] == 0x2001 && segments[1] == 0x0000 {
        let a = (!segments[6] >> 8) as u8;
        let b = (!segments[6] & 0xff) as u8;
        let c = (!segments[7] >> 8) as u8;
        let d = (!segments[7] & 0xff) as u8;
        return Some(Ipv4Addr::new(a, b, c, d));
    }
    None
}

/// Whether an IPv6 literal is in a range that must not be fetched. An IPv4-mapped address
/// (`::ffff:a.b.c.d`) is re-checked through the IPv4 rules so a mapped private address cannot slip
/// through as "just an IPv6 host".
fn ipv6_is_blocked(addr: Ipv6Addr) -> bool {
    if extract_ipv4(addr).is_some_and(ipv4_is_blocked) {
        return true;
    }
    let segments = addr.segments();
    addr.is_loopback()        // ::1
        || addr.is_unspecified() // ::
        || addr.is_multicast()   // ff00::/8
        || (segments[0] & 0xfe00) == 0xfc00 // fc00::/7  (unique local)
        || (segments[0] & 0xffc0) == 0xfe80 // fe80::/10 (link-local)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_plain_https_url_with_a_public_host() {
        let url = validate_dictionary_url("https://dict.example.com/ja/ipadic.dict.zst").unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("dict.example.com"));
    }

    #[test]
    fn accepts_a_public_ipv4_literal() {
        // A routable public address (one of Google's DNS) is allowed.
        validate_dictionary_url("https://8.8.8.8/d.zst").unwrap();
    }

    #[test]
    fn rejects_a_non_https_scheme() {
        assert!(matches!(
            validate_dictionary_url("http://dict.example.com/d.zst"),
            Err(UrlPolicyError::NotHttps)
        ));
        assert!(matches!(
            validate_dictionary_url("file:///etc/passwd"),
            Err(UrlPolicyError::NotHttps)
        ));
    }

    #[test]
    fn rejects_embedded_credentials() {
        assert!(matches!(
            validate_dictionary_url("https://user:pass@dict.example.com/d.zst"),
            Err(UrlPolicyError::HasCredentials)
        ));
        assert!(matches!(
            validate_dictionary_url("https://user@dict.example.com/d.zst"),
            Err(UrlPolicyError::HasCredentials)
        ));
    }

    #[test]
    fn rejects_the_localhost_name() {
        assert!(matches!(
            validate_dictionary_url("https://localhost/d.zst"),
            Err(UrlPolicyError::BlockedHost(_))
        ));
        assert!(matches!(
            validate_dictionary_url("https://LocalHost/d.zst"),
            Err(UrlPolicyError::BlockedHost(_))
        ));
        assert!(matches!(
            validate_dictionary_url("https://api.localhost/d.zst"),
            Err(UrlPolicyError::BlockedHost(_))
        ));
    }

    #[test]
    fn rejects_the_localhost_family_with_a_trailing_dot() {
        // The WHATWG parser keeps a trailing dot on a *domain* (`localhost.` → `Domain("localhost.")`),
        // and `localhost.` still resolves to the loopback host — so the fully-qualified form must be
        // blocked too, or it is a trivial bypass of the `localhost` rule.
        for raw in [
            "https://localhost./d.zst",
            "https://LOCALHOST./d.zst",
            "https://api.localhost./d.zst",
        ] {
            assert!(
                matches!(
                    validate_dictionary_url(raw),
                    Err(UrlPolicyError::BlockedHost(_))
                ),
                "expected {raw} to be blocked"
            );
        }
        // A public domain with a trailing dot is still allowed (the dot-strip must not over-block).
        validate_dictionary_url("https://dict.example.com./d.zst").unwrap();
    }

    #[test]
    fn rejects_blocked_ipv4_literals() {
        for host in [
            "127.0.0.1",       // loopback
            "10.0.0.5",        // private
            "172.16.0.1",      // private
            "192.168.1.1",     // private
            "169.254.0.1",     // link-local
            "0.0.0.0",         // unspecified
            "100.64.0.1",      // CGNAT shared
            "240.0.0.1",       // reserved
            "255.255.255.255", // broadcast
            "224.0.0.1",       // multicast
            "127.0.0.1.",      // trailing dot — the parser normalizes it to the Ipv4 literal
            "2130706433",      // 127.0.0.1 in integer form — normalized to the Ipv4 literal
            "0x7f.0.0.1",      // 127.0.0.1 with a hex octet — normalized to the Ipv4 literal
        ] {
            let url = format!("https://{host}/d.zst");
            assert!(
                matches!(
                    validate_dictionary_url(&url),
                    Err(UrlPolicyError::BlockedHost(_))
                ),
                "expected {host} to be blocked"
            );
        }
    }

    #[test]
    fn rejects_blocked_ipv6_literals() {
        for host in [
            "[::1]",                  // loopback
            "[::]",                   // unspecified
            "[fc00::1]",              // unique local
            "[fe80::1]",              // link-local
            "[ff02::1]",              // multicast
            "[::ffff:127.0.0.1]",     // IPv4-mapped loopback
            "[::ffff:10.0.0.1]",      // IPv4-mapped private
            "[::127.0.0.1]",          // IPv4-compatible loopback
            "[64:ff9b::127.0.0.1]",   // NAT64 loopback
            "[64:ff9b::10.0.0.1]",    // NAT64 private
            "[2002:7f00:0001::]",     // 6to4 loopback (127.0.0.1)
            "[2002:0a00:0001::]",     // 6to4 private (10.0.0.1)
            "[2001:0000::80ff:fffe]", // Teredo loopback (127.0.0.1)
            "[2001:0000::f5ff:fffe]", // Teredo private (10.0.0.1)
        ] {
            let url = format!("https://{host}/d.zst");
            assert!(
                matches!(
                    validate_dictionary_url(&url),
                    Err(UrlPolicyError::BlockedHost(_))
                ),
                "expected {host} to be blocked"
            );
        }
    }

    #[test]
    fn accepts_a_public_ipv6_literal() {
        validate_dictionary_url("https://[2001:4860:4860::8888]/d.zst").unwrap();
    }

    #[test]
    fn rejects_an_unparseable_url() {
        assert!(matches!(
            validate_dictionary_url("not a url"),
            Err(UrlPolicyError::Parse(_))
        ));
        // A relative reference has no scheme/host → parse error (it is not an absolute URL).
        assert!(matches!(
            validate_dictionary_url("/just/a/path"),
            Err(UrlPolicyError::Parse(_))
        ));
    }

    #[test]
    fn error_display_is_human_readable() {
        assert_eq!(
            UrlPolicyError::NotHttps.to_string(),
            "dictionary URL must use https"
        );
        assert_eq!(
            UrlPolicyError::HasCredentials.to_string(),
            "dictionary URL must not embed credentials"
        );
        assert_eq!(
            UrlPolicyError::MissingHost.to_string(),
            "dictionary URL has no host"
        );
        assert!(
            UrlPolicyError::BlockedHost("127.0.0.1".into())
                .to_string()
                .contains("127.0.0.1")
        );
        assert!(
            UrlPolicyError::Parse("bad".into())
                .to_string()
                .contains("bad")
        );
    }
}
