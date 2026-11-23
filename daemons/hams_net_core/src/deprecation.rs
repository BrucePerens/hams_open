//! The signed deprecation list (rogue routers' keys and prefixes, revoked member certificates) and its gossip between neighbouring
//! routers; mirrors `hams-node-status` and `docs/proposals/IPV6_DEPRECATION_LIST.md` in hams_com. Documents are self-contained and
//! signed (Ed25519, usign layout), so one received from a neighbour is verified exactly like one from hams.com and ordered by
//! sequence number only, never by clock. The platform supplies [`Crypto`], the transport (tcp 5710, one line each way) and the
//! enforcement (evict a peer, add an unreachable route) from the [`Change`]s returned.

use crate::crypto::Crypto;
use serde_json::Value;
use std::collections::BTreeMap;

/// Where neighbours ask for the list (tcp).
pub const GOSSIP_PORT: u16 = 5710;
/// A reply is at most this many lines and bytes (the stored chain is short; anything longer is cut).
const MAX_LINES: usize = 60;
const MAX_BYTES: usize = 262_144;

/// One listed key. A router peer has `prefixes`; a revoked member certificate is `cert:ISSUER:SERIAL` with none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// WireGuard public key, or `cert:ISSUER_SHA256:SERIAL`.
    pub key: String,
    /// Prefixes of the router, empty for a certificate.
    pub prefixes: Vec<String>,
}

/// What a document changed, for the platform to enforce: evict and block `added`, lift `removed`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Change {
    /// Entries newly listed.
    pub added: Vec<Entry>,
    /// Entries no longer listed.
    pub removed: Vec<Entry>,
}

struct Line {
    full: bool,
    from: u64,
    body: String,
    sig: String,
}

/// The held list, its sequence number and the signed chain to hand to neighbours.
pub struct DeprecationList {
    key: Vec<u8>,
    keynum: Vec<u8>,
    seq: u64,
    entries: BTreeMap<String, Entry>,
    chain: Vec<Line>,
}

fn entries_of(v: Option<&Value>) -> Vec<Entry> {
    let mut out = Vec::new();
    for e in v.and_then(|x| x.as_array()).into_iter().flatten() {
        let Some(key) = e.get("key").and_then(|k| k.as_str()) else {
            continue;
        };
        if e.get("type").and_then(|t| t.as_str()) == Some("revoked_member_cert") {
            let issuer = e
                .get("issuer_sha256")
                .and_then(|i| i.as_str())
                .unwrap_or("");
            let plain = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric());
            if plain(key) && plain(issuer) {
                out.push(Entry {
                    key: format!("cert:{issuer}:{key}"),
                    prefixes: vec![],
                });
            }
            continue;
        }
        let prefixes = e
            .get("prefixes")
            .and_then(|p| p.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        out.push(Entry {
            key: key.to_string(),
            prefixes,
        });
    }
    out
}

impl DeprecationList {
    // [@ANCHOR: hams_net_core_deprecation_new]
    /// An empty list that trusts the usign public key line (`base64("Ed" + 8-byte key number + 32-byte key)`), or `None` if it is not one.
    pub fn new(crypto: &dyn Crypto, public_key: &str) -> Option<Self> {
        let raw = crypto.base64_decode(public_key.trim())?;
        (raw.len() == 42 && raw.starts_with(b"Ed")).then(|| Self {
            keynum: raw[2..10].to_vec(),
            key: raw[10..].to_vec(),
            seq: 0,
            entries: BTreeMap::new(),
            chain: vec![],
        })
    }

    // [@ANCHOR: hams_net_core_deprecation_seq]
    /// The sequence number of the held list (0 when none).
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// The listed entries.
    pub fn entries(&self) -> Vec<Entry> {
        self.entries.values().cloned().collect()
    }

    fn verified(&self, crypto: &dyn Crypto, body: &str, sig: &str) -> Option<Value> {
        let payload = crypto.base64_decode(body)?;
        let sig = crypto.base64_decode(sig)?;
        (sig.len() == 74 && sig.starts_with(b"Ed") && sig[2..10] == self.keynum[..])
            .then_some(())?;
        crypto
            .ed25519_verify(&self.key, &sig[10..], &payload)
            .ok()?
            .then_some(())?;
        serde_json::from_slice(&payload).ok()
    }

    // [@ANCHOR: hams_net_core_deprecation_apply]
    /// Apply the signed envelope (`body` and `sig` as base64), from hams.com or a neighbour. `None` when the signature is wrong or it does
    /// not line up (a full list older than the held one, a delta whose `from` is not the held number: replay, gap, out of order); the
    /// platform then asks hams.com for the full list.
    pub fn apply(&mut self, crypto: &dyn Crypto, body: &str, sig: &str) -> Option<Change> {
        let doc = self.verified(crypto, body, sig)?;
        let seq = doc.get("seq")?.as_u64()?;
        let (full, from, next) = match doc.get("kind")?.as_str()? {
            "full" if seq >= self.seq => (true, 0, entries_of(doc.get("entries"))),
            "delta" => {
                let from = doc.get("from")?.as_u64()?;
                if from != self.seq || seq < from {
                    return None;
                }
                let removes: Vec<&str> = doc
                    .get("remove")
                    .and_then(|r| r.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                    .unwrap_or_default();
                let added = entries_of(doc.get("add"));
                let mut next: Vec<Entry> = self
                    .entries
                    .values()
                    .filter(|e| {
                        let serial = e.key.rsplit(':').next().unwrap_or("");
                        !removes.contains(&serial)
                            && !removes.contains(&e.key.as_str())
                            && !added.iter().any(|a| a.key == e.key)
                    })
                    .cloned()
                    .collect();
                next.extend(added);
                (false, from, next)
            }
            _ => return None,
        };
        let mut change = Change::default();
        for e in &next {
            if !self.entries.contains_key(&e.key) {
                change.added.push(e.clone());
            }
        }
        for (k, e) in &self.entries {
            if !next.iter().any(|n| &n.key == k) {
                change.removed.push(e.clone());
            }
        }
        self.entries = next.into_iter().map(|e| (e.key.clone(), e)).collect();
        self.seq = seq;
        if full {
            self.chain.clear();
        }
        self.chain.push(Line {
            full,
            from,
            body: body.to_string(),
            sig: sig.to_string(),
        });
        Some(change)
    }

    // [@ANCHOR: hams_net_core_deprecation_gossip_request]
    /// The one line a router sends a neighbour: `HAMS-GOSSIP <held seq>`.
    pub fn gossip_request(&self) -> String {
        format!("HAMS-GOSSIP {}\n", self.seq)
    }

    // [@ANCHOR: hams_net_core_deprecation_gossip_reply]
    /// The answer to a neighbour's request line: the stored chain of signed documents (the last full list and the deltas after it,
    /// `full|delta FROM BODY SIG` per line) when the held list is newer than theirs, else nothing. Anything that is not a request is
    /// answered with nothing. Only documents hams.com signed are ever sent.
    pub fn gossip_reply(&self, request: &str) -> String {
        let mut words = request.split_whitespace();
        let theirs = match (
            words.next(),
            words.next().map(str::parse::<u64>),
            words.next(),
        ) {
            (Some("HAMS-GOSSIP"), Some(Ok(n)), None) => n,
            _ => return String::new(),
        };
        if self.seq <= theirs {
            return String::new();
        }
        self.chain
            .iter()
            .map(|l| {
                let kind = if l.full { "full" } else { "delta" };
                format!("{kind} {} {} {}\n", l.from, l.body, l.sig)
            })
            .collect()
    }

    // [@ANCHOR: hams_net_core_deprecation_accept_gossip]
    /// Apply a neighbour's reply line by line with the same rules as a document from hams.com; lines that do not verify or do not line up
    /// are skipped (never an error, never a change). Returns what the accepted ones changed.
    pub fn accept_gossip(&mut self, crypto: &dyn Crypto, reply: &str) -> Vec<Change> {
        let mut changes = Vec::new();
        let reply = reply.get(..MAX_BYTES.min(reply.len())).unwrap_or("");
        for line in reply.lines().take(MAX_LINES) {
            let mut f = line.split(' ');
            if !matches!(f.next(), Some("full" | "delta")) || f.next().is_none() {
                continue;
            }
            let (Some(body), Some(sig), None) = (f.next(), f.next(), f.next()) else {
                continue;
            };
            let b64 = |s: &str| {
                s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"+/=".contains(&c))
            };
            if !b64(body) || !b64(sig) {
                continue;
            }
            let before = self.seq;
            if let Some(c) = self.apply(crypto, body, sig) {
                if self.seq > before {
                    changes.push(c);
                }
            }
        }
        changes
    }
}
