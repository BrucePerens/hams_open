//! Kotlin/Swift view of `hams_net_core`: the SIIT map and the on-demand peer table. The platform supplies the
//! directory lookup, the WireGuard backend and the clock as callback interfaces.

use hams_net_core::directory::{Prefix60, Record};
use hams_net_core::peers::{LinkType, PeerTable as CoreTable, Wg};
use hams_net_core::siit::Block;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, Mutex};

uniffi::setup_scaffolding!();

/// Every failure the core reports, with its message.
#[derive(Debug, uniffi::Error)]
pub enum NetError {
    /// The core refused (bad address, RF link, backend error ...).
    Refused {
        /// Human-readable reason.
        reason: String,
    },
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let NetError::Refused { reason } = self;
        write!(f, "{reason}")
    }
}

fn refused(message: impl Into<String>) -> NetError {
    NetError::Refused {
        reason: message.into(),
    }
}

// [@ANCHOR: hams_net_ffi_siit_embed]
/// IPv6 address of IPv4 host `ipv4` in `domain` inside the translation /48 `block` ("<addr>/48").
#[uniffi::export]
pub fn siit_embed(block: String, domain: u16, ipv4: String) -> Result<String, NetError> {
    let block = Block::parse(&block).map_err(refused)?;
    let v4: Ipv4Addr = ipv4.parse().map_err(|_| refused("not an IPv4 address"))?;
    Ok(block.embed(domain, v4).to_string())
}

/// Result of `siit_extract`.
#[derive(uniffi::Record)]
pub struct SiitHost {
    /// The domain number.
    pub domain: u16,
    /// The IPv4 address.
    pub ipv4: String,
}

// [@ANCHOR: hams_net_ffi_siit_extract]
/// Inverse of `siit_embed`.
#[uniffi::export]
pub fn siit_extract(block: String, address: String) -> Result<SiitHost, NetError> {
    let block = Block::parse(&block).map_err(refused)?;
    let v6: Ipv6Addr = address
        .parse()
        .map_err(|_| refused("not an IPv6 address"))?;
    let (domain, v4) = block.extract(v6).map_err(refused)?;
    Ok(SiitHost {
        domain,
        ipv4: v4.to_string(),
    })
}

/// A directory record as the platform sees it.
#[derive(uniffi::Record)]
pub struct DirectoryRecord {
    /// The member's callsign.
    pub callsign: String,
    /// The member's "<addr>/60".
    pub prefix60: String,
    /// WireGuard public key.
    pub public_key: String,
    /// `host:port`.
    pub endpoint: String,
}

/// The platform's directory lookup (members-only HTTPS call).
#[uniffi::export(with_foreign)]
pub trait DirectoryLookup: Send + Sync {
    /// The record whose /60 contains `destination`, if any.
    fn lookup(&self, destination: String) -> Option<DirectoryRecord>;
}

/// The platform's WireGuard backend.
#[uniffi::export(with_foreign)]
pub trait WireGuard: Send + Sync {
    /// Add a peer allowed for `prefix60`.
    fn add_peer(&self, interface: String, public_key: String, endpoint: String, prefix60: String);
    /// Remove a peer.
    fn remove_peer(&self, interface: String, public_key: String);
}

struct ForeignWg(Arc<dyn WireGuard>);

impl Wg for ForeignWg {
    fn add_peer(&mut self, i: &str, key: &str, endpoint: &str, p: Prefix60) -> Result<(), String> {
        self.0
            .add_peer(i.into(), key.into(), endpoint.into(), p.to_string());
        Ok(())
    }
    fn remove_peer(&mut self, i: &str, key: &str) -> Result<(), String> {
        self.0.remove_peer(i.into(), key.into());
        Ok(())
    }
}

type Lookup = Box<dyn FnMut(Ipv6Addr) -> Option<Record> + Send>;

/// On-demand WireGuard peers (see `hams_net_core::peers`).
#[derive(uniffi::Object)]
pub struct PeerTable(Mutex<CoreTable<ForeignWg, Lookup>>);

#[uniffi::export]
impl PeerTable {
    // [@ANCHOR: hams_net_ffi_peer_table_new]
    /// A table for `interface`; `rf_link` true is refused (no WireGuard on an RF link).
    #[uniffi::constructor]
    pub fn new(
        directory: Arc<dyn DirectoryLookup>,
        wireguard: Arc<dyn WireGuard>,
        interface: String,
        rf_link: bool,
    ) -> Result<Arc<Self>, NetError> {
        let lookup: Lookup = Box::new(move |dest| {
            let r = directory.lookup(dest.to_string())?;
            Some(Record {
                callsign: r.callsign,
                prefix60: Prefix60::parse(&r.prefix60).ok()?,
                public_key: r.public_key,
                endpoint: r.endpoint,
                interest: vec![],
            })
        });
        let link = if rf_link {
            LinkType::Rf
        } else {
            LinkType::Internet
        };
        let table =
            CoreTable::new(lookup, ForeignWg(wireguard), &interface, link).map_err(refused)?;
        Ok(Arc::new(PeerTable(Mutex::new(table))))
    }

    // [@ANCHOR: hams_net_ffi_peer_table_on_packet]
    /// Traffic toward `destination` at `now` seconds; the peer's public key, or none.
    pub fn on_packet(&self, destination: String, now: f64) -> Result<Option<String>, NetError> {
        let dest: Ipv6Addr = destination
            .parse()
            .map_err(|_| refused("not an IPv6 address"))?;
        self.0
            .lock()
            .map_err(|_| refused("peer table lock poisoned"))?
            .on_packet(dest, now)
            .map_err(refused)
    }

    // [@ANCHOR: hams_net_ffi_peer_table_expire]
    /// Remove idle peers; the public keys removed.
    pub fn expire(&self, now: f64) -> Result<Vec<String>, NetError> {
        self.0
            .lock()
            .map_err(|_| refused("peer table lock poisoned"))?
            .expire(now)
            .map_err(refused)
    }
}
