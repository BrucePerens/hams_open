//! On-demand WireGuard peers; mirrors `ham_ipv6net/client/peers.py`. The first packet toward a member /60 asks
//! the directory and adds a peer with `AllowedIPs = <their /60>`; a peer idle for ten minutes is removed.
//! The directory lookup, the WireGuard backend and the clock are the consumer's (`now` is seconds).
//! Never made on an RF link (47 CFR 97.113(a)(4): WireGuard encrypts).

use crate::directory::{Prefix60, Record};
use std::net::Ipv6Addr;

/// Seconds without traffic before a peer is removed.
pub const IDLE_SECONDS: f64 = 600.0;
/// A destination the directory did not know is not asked about again for this long.
pub const REFUSED_SECONDS: f64 = 60.0;

/// Kind of link the table works on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkType {
    /// An internet link: peers allowed.
    Internet,
    /// An RF link: never.
    Rf,
}

/// The WireGuard backend of the platform.
pub trait Wg {
    /// Add a peer with `allowed_ip` (the member /60) at `endpoint`.
    fn add_peer(
        &mut self,
        interface: &str,
        public_key: &str,
        endpoint: &str,
        prefix: Prefix60,
    ) -> Result<(), String>;
    /// Remove a peer.
    fn remove_peer(&mut self, interface: &str, public_key: &str) -> Result<(), String>;
}

/// Peers made on demand on one interface.
pub struct PeerTable<W: Wg, L: FnMut(Ipv6Addr) -> Option<Record>> {
    lookup: L,
    wg: W,
    interface: String,
    idle: f64,
    peers: Vec<(String, Prefix60, f64)>,
    refused: Vec<(Ipv6Addr, f64)>,
}

impl<W: Wg, L: FnMut(Ipv6Addr) -> Option<Record>> PeerTable<W, L> {
    // [@ANCHOR: hams_net_core_peers_new]
    /// A table for `interface`; refuses an RF link.
    pub fn new(lookup: L, wg: W, interface: &str, link: LinkType) -> Result<Self, String> {
        if link != LinkType::Internet {
            return Err("a WireGuard peer is only made on an internet link, not on rf".into());
        }
        Ok(PeerTable {
            lookup,
            wg,
            interface: interface.into(),
            idle: IDLE_SECONDS,
            peers: vec![],
            refused: vec![],
        })
    }

    // [@ANCHOR: hams_net_core_peers_on_packet]
    /// Traffic toward `destination` at time `now`: refresh its peer or make one from the directory. The
    /// peer's public key, or `None` when the directory has no current record for it.
    pub fn on_packet(&mut self, destination: Ipv6Addr, now: f64) -> Result<Option<String>, String> {
        if let Some(peer) = self.peers.iter_mut().find(|p| p.1.contains(destination)) {
            peer.2 = now;
            return Ok(Some(peer.0.clone()));
        }
        if self
            .refused
            .iter()
            .any(|r| r.0 == destination && now - r.1 < REFUSED_SECONDS)
        {
            return Ok(None);
        }
        let record = match (self.lookup)(destination) {
            Some(r) if r.prefix60.contains(destination) => r,
            _ => {
                self.refused.retain(|r| r.0 != destination);
                self.refused.push((destination, now));
                return Ok(None);
            }
        };
        self.wg.add_peer(
            &self.interface,
            &record.public_key,
            &record.endpoint,
            record.prefix60,
        )?;
        self.peers
            .push((record.public_key.clone(), record.prefix60, now));
        Ok(Some(record.public_key))
    }

    // [@ANCHOR: hams_net_core_peers_expire]
    /// Remove every peer idle for longer than the idle time; returns their public keys.
    pub fn expire(&mut self, now: f64) -> Result<Vec<String>, String> {
        let mut gone = vec![];
        for peer in self.peers.iter().filter(|p| now - p.2 > self.idle) {
            self.wg.remove_peer(&self.interface, &peer.0)?;
            gone.push(peer.0.clone());
        }
        self.peers.retain(|p| !gone.contains(&p.0));
        Ok(gone)
    }
}
