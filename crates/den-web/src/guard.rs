//! Who may talk to the shelf.
//!
//! The remote has no password, the same trust the family's media server
//! extends: anything already on the network may read the shelf and start what
//! is on it. Two things are still worth refusing, and both cost one header
//! each to check.
//!
//! **A name that is not ours.** Anyone can point `games.example.com` at a
//! private address and serve you a page from it; the browser then treats that
//! page as same-origin with Play and every guard below falls away. So Play
//! answers to addresses, to single-label names (nobody can register a name
//! with no dot), and to the suffixes a private network actually uses —
//! `.ts.net` for Tailscale among them. `DEN_WEB_HOSTS` adds your own.
//!
//! **A request from somebody else's page.** A tab open on an unrelated site
//! can POST to a private address without being able to read the answer, which
//! is enough to start a game on your television. Browsers attach `Origin` to
//! those, so a POST whose `Origin` is not Play's own page is refused. A
//! request with no `Origin` at all — `curl`, a script — is let through, since
//! that is somebody at the keyboard, not a page.

use std::net::IpAddr;

/// Suffixes that cannot be handed to a stranger by public DNS.
const PRIVATE_SUFFIXES: &[&str] = &[".ts.net", ".local", ".internal", ".localhost", ".lan"];

const EXTRA_HOSTS: &str = "DEN_WEB_HOSTS";

/// The host part of a `Host` header or an `Origin`, without the port.
pub fn host_name(value: &str) -> &str {
    let value = value.trim();
    // `[::1]:5555` — an address literal, bracketed as the standard requires.
    if let Some(rest) = value.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest);
    }
    match value.rsplit_once(':') {
        // A bare IPv6 nobody bracketed still has colons left over; keep it whole.
        Some((name, port))
            if !name.contains(':')
                && !port.is_empty()
                && port.bytes().all(|b| b.is_ascii_digit()) =>
        {
            name
        }
        _ => value,
    }
}

/// `https://host:port` down to `host`.
fn origin_host(origin: &str) -> Option<&str> {
    let rest = origin.split_once("://").map(|(_, rest)| rest)?;
    if rest.is_empty() {
        return None;
    }
    Some(host_name(rest.split('/').next().unwrap_or(rest)))
}

fn extra_hosts() -> Vec<String> {
    std::env::var(EXTRA_HOSTS)
        .unwrap_or_default()
        .split(',')
        .map(|name| host_name(name).to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect()
}

/// Whether Play should answer to this name at all.
pub fn name_is_ours(name: &str) -> bool {
    let name = host_name(name);
    if name.is_empty() {
        return false;
    }
    if name.parse::<IpAddr>().is_ok() {
        return true;
    }
    let lower = name.trim_end_matches('.').to_ascii_lowercase();
    if lower == "localhost" {
        return true;
    }
    // Nobody can register a name with no dot in it, so nobody can point one here.
    if !lower.contains('.') {
        return true;
    }
    if PRIVATE_SUFFIXES.iter().any(|s| lower.ends_with(s)) {
        return true;
    }
    extra_hosts().contains(&lower)
}

fn changes_something(method: &str) -> bool {
    !matches!(
        method.to_ascii_uppercase().as_str(),
        "GET" | "HEAD" | "OPTIONS" | "TRACE"
    )
}

/// The reason to refuse this request, in the words Play would say, or nothing
/// if it is welcome.
pub fn refusal(method: &str, host: Option<&str>, origin: Option<&str>) -> Option<String> {
    let host = host.map(str::trim).filter(|h| !h.is_empty());

    if let Some(host) = host {
        if !name_is_ours(host) {
            let name = host_name(host);
            return Some(format!(
                "Play does not answer to the name `{name}`. Open it by address instead, \
                 or set {EXTRA_HOSTS}={name} on the machine the library lives on."
            ));
        }
    }

    if !changes_something(method) {
        return None;
    }
    let Some(origin) = origin.map(str::trim).filter(|o| !o.is_empty()) else {
        // No page asked; somebody at a keyboard did.
        return None;
    };
    let Some(from) = origin_host(origin) else {
        return Some(format!("`{origin}` is not a page Play serves."));
    };
    let here = host.map(host_name).unwrap_or_default();
    if from.eq_ignore_ascii_case(here) {
        return None;
    }
    Some(format!(
        "That asked from `{origin}`, which is not Play's own page. \
         Open the shelf itself to start or stop a game."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_header_gives_up_its_name_without_its_port() {
        assert_eq!(host_name("192.168.1.5:5555"), "192.168.1.5");
        assert_eq!(host_name("desktop"), "desktop");
        assert_eq!(host_name("[fd7a:115c:a1e0::1]:5555"), "fd7a:115c:a1e0::1");
        assert_eq!(host_name("[::1]"), "::1");
        assert_eq!(host_name("box.tail1234.ts.net"), "box.tail1234.ts.net");
        // Not bracketed, so not a port — keep every colon.
        assert_eq!(host_name("fd7a:115c:a1e0::1"), "fd7a:115c:a1e0::1");
    }

    #[test]
    fn play_answers_to_addresses_and_to_private_names() {
        assert!(name_is_ours("192.168.1.5:5555"));
        assert!(name_is_ours("100.101.20.33:5555"));
        assert!(name_is_ours("[fd7a:115c:a1e0::1]:5555"));
        assert!(name_is_ours("localhost:5555"));
        // MagicDNS short names, and mDNS.
        assert!(name_is_ours("desktop"));
        assert!(name_is_ours("desktop.local"));
        assert!(name_is_ours("box.tail1234.ts.net"));
    }

    #[test]
    fn a_public_name_pointed_at_a_private_address_is_refused() {
        let said = refusal("GET", Some("games.example.com"), None).expect("refused");
        assert!(said.contains("games.example.com"), "{said}");
        assert!(said.contains("DEN_WEB_HOSTS"), "{said}");
        assert!(!name_is_ours("evil.example.com"));
    }

    #[test]
    fn a_name_you_told_play_about_is_allowed() {
        // Env vars are process-wide; this test owns the key.
        std::env::set_var(EXTRA_HOSTS, "games.example.com, shelf.home.arpa");
        assert!(name_is_ours("games.example.com"));
        assert!(name_is_ours("GAMES.example.com:5555"));
        assert!(name_is_ours("shelf.home.arpa"));
        assert!(!name_is_ours("somewhere.example.com"));
        std::env::remove_var(EXTRA_HOSTS);
    }

    #[test]
    fn reading_the_shelf_never_needs_an_origin() {
        assert!(refusal("GET", Some("192.168.1.5:5555"), None).is_none());
        // A page elsewhere may look; it may not press anything.
        assert!(refusal(
            "GET",
            Some("192.168.1.5:5555"),
            Some("https://elsewhere.example")
        )
        .is_none());
    }

    #[test]
    fn a_post_from_somebody_elses_page_is_refused() {
        let said = refusal(
            "POST",
            Some("192.168.1.5:5555"),
            Some("https://elsewhere.example"),
        )
        .expect("refused");
        assert!(said.contains("elsewhere.example"), "{said}");
        assert!(refusal("POST", Some("192.168.1.5:5555"), Some("null")).is_some());
    }

    #[test]
    fn a_post_from_the_shelf_itself_goes_through() {
        assert!(refusal(
            "POST",
            Some("192.168.1.5:5555"),
            Some("http://192.168.1.5:5555")
        )
        .is_none());
        // Behind `tailscale serve` the scheme and port change; the name does not.
        assert!(refusal(
            "POST",
            Some("box.tail1234.ts.net"),
            Some("https://box.tail1234.ts.net")
        )
        .is_none());
    }

    #[test]
    fn a_request_with_no_page_behind_it_is_somebody_at_a_keyboard() {
        assert!(refusal("POST", Some("192.168.1.5:5555"), None).is_none());
        assert!(refusal("POST", None, None).is_none());
    }
}
