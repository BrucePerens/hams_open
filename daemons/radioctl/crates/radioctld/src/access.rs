// SPDX-License-Identifier: AGPL-3.0-or-later
//! The access allow-list: which peer addresses may talk to a rig. Checked on accept, before a
//! single byte is read. An empty list allows nobody.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// One network: an address and a prefix length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Net {
    addr: IpAddr,
    prefix: u8,
}

/// The set of networks allowed to connect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowList {
    nets: Vec<Net>,
}

// [@ANCHOR: radioctld_access_normalise]
/// An IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) as the IPv4 address it stands for.
fn normalise(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        v4 => v4,
    }
}

// [@ANCHOR: radioctld_access_prefix_matches]
/// True when the first `prefix` bits of `a` and `b` agree.
fn prefix_matches(a: &[u8], b: &[u8], prefix: u8) -> bool {
    let whole = usize::from(prefix / 8);
    let rest = prefix % 8;
    if a.get(..whole) != b.get(..whole) {
        return false;
    }
    if rest == 0 {
        return true;
    }
    let mask = 0xFFu8 << (8 - rest);
    match (a.get(whole), b.get(whole)) {
        (Some(x), Some(y)) => x & mask == y & mask,
        _ => false,
    }
}

// [@ANCHOR: radioctld_access_octets]
/// The address bytes of an IP address.
fn octets(ip: IpAddr) -> Vec<u8> {
    match ip {
        IpAddr::V4(v4) => v4.octets().to_vec(),
        IpAddr::V6(v6) => v6.octets().to_vec(),
    }
}

impl AllowList {
    // [@ANCHOR: radioctld_access_loopback_only]
    /// The default: this machine only (`127.0.0.0/8` and `::1`).
    pub fn loopback_only() -> AllowList {
        AllowList {
            nets: vec![
                Net {
                    addr: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 0)),
                    prefix: 8,
                },
                Net {
                    addr: IpAddr::V6(Ipv6Addr::LOCALHOST),
                    prefix: 128,
                },
            ],
        }
    }

    // [@ANCHOR: radioctld_access_parse]
    /// Builds a list from entries such as `192.168.1.0/24`, `10.0.0.5` or `fd00::/8`.
    pub fn parse(entries: &[String]) -> Result<AllowList, String> {
        let mut nets = Vec::new();
        for entry in entries {
            let (addr_text, prefix_text) = match entry.split_once('/') {
                Some((a, p)) => (a, Some(p)),
                None => (entry.as_str(), None),
            };
            let addr: IpAddr = addr_text
                .trim()
                .parse()
                .map_err(|_| format!("allow-list entry {entry:?} is not an IP address"))?;
            let addr = normalise(addr);
            let longest = if addr.is_ipv4() { 32 } else { 128 };
            let prefix = match prefix_text {
                Some(p) => p
                    .trim()
                    .parse::<u8>()
                    .ok()
                    .filter(|p| *p <= longest)
                    .ok_or_else(|| format!("allow-list entry {entry:?} has a bad prefix length"))?,
                None => longest,
            };
            nets.push(Net { addr, prefix });
        }
        Ok(AllowList { nets })
    }

    // [@ANCHOR: radioctld_access_allows]
    /// True when `peer` is inside an allowed network.
    pub fn allows(&self, peer: IpAddr) -> bool {
        let peer = normalise(peer);
        let peer_bytes = octets(peer);
        self.nets.iter().any(|net| {
            net.addr.is_ipv4() == peer.is_ipv4()
                && prefix_matches(&octets(net.addr), &peer_bytes, net.prefix)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().expect("ip")
    }

    fn list(entries: &[&str]) -> AllowList {
        let owned: Vec<String> = entries.iter().map(|e| (*e).to_string()).collect();
        AllowList::parse(&owned).expect("list")
    }

    // Tests [@ANCHOR: radioctld_access_loopback_only]
    // Tests [@ANCHOR: radioctld_access_allows]
    // Tests [@ANCHOR: radioctld_access_normalise]
    #[test]
    fn the_default_allows_only_this_machine() {
        let allow = AllowList::loopback_only();
        assert!(allow.allows(ip("127.0.0.1")));
        assert!(allow.allows(ip("127.8.9.10")));
        assert!(allow.allows(ip("::1")));
        assert!(allow.allows(ip("::ffff:127.0.0.1")));
        assert!(!allow.allows(ip("192.168.1.5")));
        assert!(!allow.allows(ip("::2")));
    }

    // Tests [@ANCHOR: radioctld_access_parse]
    // Tests [@ANCHOR: radioctld_access_prefix_matches]
    // Tests [@ANCHOR: radioctld_access_octets]
    #[test]
    fn networks_match_by_prefix_and_an_empty_list_allows_nobody() {
        let allow = list(&["192.168.1.0/24", "10.0.0.5", "fd00::/8"]);
        assert!(allow.allows(ip("192.168.1.200")));
        assert!(!allow.allows(ip("192.168.2.1")));
        assert!(allow.allows(ip("10.0.0.5")));
        assert!(!allow.allows(ip("10.0.0.6")));
        assert!(allow.allows(ip("fd12::1")));
        assert!(!allow.allows(ip("fe80::1")));
        assert!(list(&["10.1.0.0/12"]).allows(ip("10.15.255.255")));
        assert!(!list(&["10.1.0.0/12"]).allows(ip("10.16.0.1")));
        assert!(list(&["0.0.0.0/0"]).allows(ip("8.8.8.8")));
        assert!(!list(&["0.0.0.0/0"]).allows(ip("::1")));
        assert!(!list(&[]).allows(ip("127.0.0.1")));
    }

    // Tests [@ANCHOR: radioctld_access_parse]
    #[test]
    fn bad_entries_are_rejected() {
        for bad in ["nonsense", "10.0.0.0/33", "10.0.0.0/x", "::1/129", ""] {
            assert!(AllowList::parse(&[bad.to_string()]).is_err(), "{bad}");
        }
    }
}
