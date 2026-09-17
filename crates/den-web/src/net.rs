//! Which addresses actually reach the shelf, and from where.

use serde::Serialize;
use std::net::{IpAddr, SocketAddr, UdpSocket};

/// Where an address works. A word, like every other status Play reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Only from the machine the library lives on.
    Machine,
    /// From anything on the same network — the phone on the sofa.
    Network,
    /// From anywhere signed in to the same tailnet, including away from home.
    Tailnet,
}

impl Reach {
    pub fn word(self) -> &'static str {
        match self {
            Reach::Machine => "this machine",
            Reach::Network => "this network",
            Reach::Tailnet => "your tailnet",
        }
    }

    /// Nearest first: the network you are probably on, then the one you carry.
    fn order(self) -> u8 {
        match self {
            Reach::Network => 0,
            Reach::Tailnet => 1,
            Reach::Machine => 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Reachable {
    pub url: String,
    pub reach: String,
}

// Route probes, not interface enumeration: ask the kernel which local address
// it would use to reach each of these. No packet leaves the machine for a
// connected UDP socket, so this costs nothing and needs no privileges.
//
// The last two are Tailscale's: `100.100.100.100` is its resolver, inside the
// 100.64.0.0/10 range every tailnet address comes from, and `fd7a:115c:a1e0::/48`
// is its slice of the IPv6 unique-local space. If tailscaled is up, the route
// to either exists and the answer is this machine's tailnet address.
const ROUTE_PROBES: &[&str] = &[
    "1.1.1.1:9",
    "192.168.1.1:9",
    "10.0.0.1:9",
    "172.16.0.1:9",
    "100.100.100.100:9",
    "[fd7a:115c:a1e0::1]:9",
];

/// Tailscale hands out 100.64.0.0/10 and `fd7a:115c:a1e0::/48`. A carrier can
/// also NAT a WAN link inside that v4 range, but a *local interface* holding
/// one on a desktop means tailscaled put it there.
pub fn is_tailnet(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            octets[0] == 100 && (64..128).contains(&octets[1])
        }
        IpAddr::V6(v6) => v6.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

pub fn reach_of(ip: IpAddr) -> Reach {
    if ip.is_loopback() {
        Reach::Machine
    } else if is_tailnet(ip) {
        Reach::Tailnet
    } else {
        Reach::Network
    }
}

pub fn url_for(ip: IpAddr, port: u16) -> String {
    match ip {
        IpAddr::V4(v4) => format!("http://{v4}:{port}"),
        IpAddr::V6(v6) => format!("http://[{v6}]:{port}"),
    }
}

/// Every address this listener can be opened at, each labelled with where it
/// works. Bound to one address, that is the only answer there is.
pub fn reachable(local: SocketAddr) -> Vec<Reachable> {
    let port = local.port();
    if !local.ip().is_unspecified() {
        return vec![Reachable {
            url: url_for(local.ip(), port),
            reach: reach_of(local.ip()).word().to_string(),
        }];
    }

    let mut found: Vec<IpAddr> = Vec::new();
    for target in ROUTE_PROBES {
        let Some(ip) = local_address_toward(target) else {
            continue;
        };
        if ip.is_loopback() || ip.is_unspecified() || found.contains(&ip) {
            continue;
        }
        found.push(ip);
    }

    let mut rows: Vec<(Reach, Reachable)> = found
        .into_iter()
        .map(|ip| {
            let reach = reach_of(ip);
            (
                reach,
                Reachable {
                    url: url_for(ip, port),
                    reach: reach.word().to_string(),
                },
            )
        })
        .collect();
    rows.sort_by_key(|(reach, _)| reach.order());
    rows.into_iter().map(|(_, row)| row).collect()
}

/// The address the kernel would send from to reach `target`, or nothing when
/// there is no route to it at all.
fn local_address_toward(target: &str) -> Option<IpAddr> {
    let bind = if target.starts_with('[') {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    let socket = UdpSocket::bind(bind).ok()?;
    socket.connect(target).ok()?;
    Some(socket.local_addr().ok()?.ip())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    #[test]
    fn the_tailnet_range_is_told_apart_from_the_house_network() {
        assert!(is_tailnet(v4(100, 64, 0, 1)));
        assert!(is_tailnet(v4(100, 101, 20, 33)));
        assert!(is_tailnet(v4(100, 127, 255, 254)));
        // The edges of 100.64.0.0/10, from the outside.
        assert!(!is_tailnet(v4(100, 63, 255, 255)));
        assert!(!is_tailnet(v4(100, 128, 0, 0)));
        assert!(!is_tailnet(v4(192, 168, 1, 40)));
        assert!(!is_tailnet(v4(10, 0, 0, 5)));
    }

    #[test]
    fn tailscales_ipv6_range_counts_too() {
        let ts: IpAddr = "fd7a:115c:a1e0::1a2b:3c4d".parse().unwrap();
        assert!(is_tailnet(ts));
        let other: IpAddr = "fd00::1".parse().unwrap();
        assert!(!is_tailnet(other));
        assert!(!is_tailnet(IpAddr::V6(Ipv6Addr::LOCALHOST)));
    }

    #[test]
    fn every_address_is_labelled_with_where_it_works() {
        assert_eq!(reach_of(v4(127, 0, 0, 1)), Reach::Machine);
        assert_eq!(reach_of(v4(192, 168, 1, 40)), Reach::Network);
        assert_eq!(reach_of(v4(100, 101, 20, 33)), Reach::Tailnet);
        assert_eq!(Reach::Tailnet.word(), "your tailnet");
    }

    #[test]
    fn a_listener_bound_to_one_address_offers_only_that_one() {
        let bound = SocketAddr::from(([100, 101, 20, 33], 5555));
        let rows = reachable(bound);
        assert_eq!(
            rows,
            vec![Reachable {
                url: "http://100.101.20.33:5555".to_string(),
                reach: "your tailnet".to_string(),
            }]
        );

        let loopback = SocketAddr::from(([127, 0, 0, 1], 5555));
        assert_eq!(reachable(loopback)[0].reach, "this machine");
    }

    #[test]
    fn an_ipv6_address_is_bracketed_so_the_url_is_clickable() {
        let ip: IpAddr = "fd7a:115c:a1e0::1".parse().unwrap();
        assert_eq!(url_for(ip, 5555), "http://[fd7a:115c:a1e0::1]:5555");
        assert_eq!(url_for(v4(10, 0, 0, 2), 80), "http://10.0.0.2:80");
    }

    #[test]
    fn the_network_comes_before_the_tailnet_and_the_machine_last() {
        let mut reaches = [Reach::Machine, Reach::Tailnet, Reach::Network];
        reaches.sort_by_key(|r| r.order());
        assert_eq!(reaches, [Reach::Network, Reach::Tailnet, Reach::Machine]);
    }
}
