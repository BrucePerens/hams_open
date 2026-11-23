//! Kotlin/Swift view of `hams_net_core`: the SIIT map and the on-demand peer table. The platform supplies the
//! directory lookup, the WireGuard backend and the clock as callback interfaces.

use hams_net_core::crypto::{BadKey, Crypto};
use hams_net_core::deprecation::{
    Change as DepChange, DeprecationList as CoreList, Entry as DepEntry,
};
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

/// The platform's cryptographic primitives (Android's Conscrypt/Tink): the core stays free of native crypto.
#[uniffi::export(with_foreign)]
pub trait PlatformCrypto: Send + Sync {
    /// Ed25519 check of `signature` over `message` under the raw 32-byte `key`.
    fn ed25519_verify(&self, key: Vec<u8>, signature: Vec<u8>, message: Vec<u8>) -> bool;
    /// Bytes of standard base64 text, empty if it does not decode.
    fn base64_decode(&self, text: String) -> Vec<u8>;
}

struct ForeignCrypto(Arc<dyn PlatformCrypto>);

impl Crypto for ForeignCrypto {
    fn sha256(&self, _: &[u8]) -> Option<Vec<u8>> {
        None
    }
    fn sha512(&self, _: &[u8]) -> Option<Vec<u8>> {
        None
    }
    // [@ANCHOR: hams_net_ffi_foreign_crypto_ed25519_verify]
    fn ed25519_verify(&self, key: &[u8], sig: &[u8], msg: &[u8]) -> Result<bool, BadKey> {
        if key.len() != 32 {
            return Err(BadKey);
        }
        Ok(self
            .0
            .ed25519_verify(key.to_vec(), sig.to_vec(), msg.to_vec()))
    }
    fn base64_encode(&self, _: &[u8]) -> String {
        String::new()
    }
    fn base64_decode(&self, text: &str) -> Option<Vec<u8>> {
        Some(self.0.base64_decode(text.into())).filter(|b| !b.is_empty())
    }
    fn random16(&self) -> Option<[u8; 16]> {
        None
    }
}

/// One listed key as the platform sees it.
#[derive(uniffi::Record)]
pub struct ListedKey {
    /// WireGuard public key, or `cert:ISSUER:SERIAL` for a revoked member certificate.
    pub key: String,
    /// Prefixes of that router (empty for a certificate).
    pub prefixes: Vec<String>,
}

/// What a signed document changed: the platform evicts and blocks `added` and lifts `removed`.
#[derive(uniffi::Record)]
pub struct ListChange {
    /// Newly listed.
    pub added: Vec<ListedKey>,
    /// No longer listed.
    pub removed: Vec<ListedKey>,
}

fn listed(e: Vec<DepEntry>) -> Vec<ListedKey> {
    e.into_iter()
        .map(|e| ListedKey {
            key: e.key,
            prefixes: e.prefixes,
        })
        .collect()
}

fn list_change(c: DepChange) -> ListChange {
    ListChange {
        added: listed(c.added),
        removed: listed(c.removed),
    }
}

/// The signed deprecation list and its gossip (see `hams_net_core::deprecation`). The platform fetches documents from hams.com
/// or a neighbour (tcp 5710), passes them in, and enforces the returned changes.
#[derive(uniffi::Object)]
pub struct DeprecationList {
    crypto: ForeignCrypto,
    list: Mutex<CoreList>,
}

#[uniffi::export]
impl DeprecationList {
    // [@ANCHOR: hams_net_ffi_deprecation_new]
    /// An empty list that trusts the usign public key line from enrolment.
    #[uniffi::constructor]
    pub fn new(crypto: Arc<dyn PlatformCrypto>, public_key: String) -> Result<Arc<Self>, NetError> {
        let crypto = ForeignCrypto(crypto);
        let list =
            CoreList::new(&crypto, &public_key).ok_or_else(|| refused("not a usign public key"))?;
        Ok(Arc::new(Self {
            crypto,
            list: Mutex::new(list),
        }))
    }

    // [@ANCHOR: hams_net_ffi_deprecation_seq]
    /// The held sequence number (0 for none).
    pub fn seq(&self) -> Result<u64, NetError> {
        Ok(self
            .list
            .lock()
            .map_err(|_| refused("list lock poisoned"))?
            .seq())
    }

    // [@ANCHOR: hams_net_ffi_deprecation_apply]
    /// Apply a signed envelope (`payload` and `signature`, base64) from hams.com; none when it is refused (the platform then asks hams.com for the full list).
    pub fn apply(
        &self,
        payload: String,
        signature: String,
    ) -> Result<Option<ListChange>, NetError> {
        let mut l = self
            .list
            .lock()
            .map_err(|_| refused("list lock poisoned"))?;
        Ok(l.apply(&self.crypto, &payload, &signature).map(list_change))
    }

    // [@ANCHOR: hams_net_ffi_deprecation_gossip_request]
    /// The line to send a neighbour.
    pub fn gossip_request(&self) -> Result<String, NetError> {
        Ok(self
            .list
            .lock()
            .map_err(|_| refused("list lock poisoned"))?
            .gossip_request())
    }

    // [@ANCHOR: hams_net_ffi_deprecation_gossip_reply]
    /// The answer to a neighbour's request line (empty unless the held list is newer).
    pub fn gossip_reply(&self, request: String) -> Result<String, NetError> {
        Ok(self
            .list
            .lock()
            .map_err(|_| refused("list lock poisoned"))?
            .gossip_reply(&request))
    }

    // [@ANCHOR: hams_net_ffi_deprecation_accept_gossip]
    /// Apply a neighbour's reply; the changes of the documents that verified and lined up.
    pub fn accept_gossip(&self, reply: String) -> Result<Vec<ListChange>, NetError> {
        let mut l = self
            .list
            .lock()
            .map_err(|_| refused("list lock poisoned"))?;
        Ok(l.accept_gossip(&self.crypto, &reply)
            .into_iter()
            .map(list_change)
            .collect())
    }
}
