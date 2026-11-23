//! Directory client without I/O (docs/proposals/IPV6_MEMBER_NETWORK.md, "Directory API"): builds the request
//! bodies and parses the answers of `/api/ipv6/directory/lookup` and `/api/ipv6/directory/hubs`. The consumer
//! owns the HTTPS transport and the credential (member session or device token).

use serde_json::{json, Value};
use std::net::Ipv6Addr;

/// A member /60 (host bits must be zero).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prefix60(u128);

impl Prefix60 {
    // [@ANCHOR: hams_net_core_prefix60_parse]
    /// Parse `"<addr>/60"`; refuses another length or set host bits.
    pub fn parse(text: &str) -> Result<Prefix60, String> {
        let bad = || format!("not an IPv6 /60: {text}");
        let (addr, len) = text.split_once('/').ok_or_else(bad)?;
        let addr: Ipv6Addr = addr.parse().map_err(|_| bad())?;
        let value = u128::from(addr);
        if len != "60" || value & (!0u128 >> 60) != 0 {
            return Err(bad());
        }
        Ok(Prefix60(value))
    }

    // [@ANCHOR: hams_net_core_prefix60_contains]
    /// Whether `address` is inside the prefix.
    pub fn contains(&self, address: Ipv6Addr) -> bool {
        u128::from(address) >> 68 == self.0 >> 68
    }
}

/// One directory record: how to reach a member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The member's callsign.
    pub callsign: String,
    /// The member's /60.
    pub prefix60: Prefix60,
    /// WireGuard public key.
    pub public_key: String,
    /// `host:port` of the WireGuard endpoint.
    pub endpoint: String,
    /// Interest set (callsigns and call-area digits).
    pub interest: Vec<String>,
}

// [@ANCHOR: hams_net_core_directory_lookup_request]
/// `(path, JSON-RPC body)` asking for a callsign or an address.
pub fn lookup_request(query: &str) -> (&'static str, String) {
    (
        "/api/ipv6/directory/lookup",
        json!({"jsonrpc": "2.0", "method": "call", "params": {"query": query}}).to_string(),
    )
}

// [@ANCHOR: hams_net_core_directory_hubs_request]
/// `(path, JSON-RPC body)` asking for the volunteer hubs.
pub fn hubs_request() -> (&'static str, String) {
    (
        "/api/ipv6/directory/hubs",
        json!({"jsonrpc": "2.0", "method": "call", "params": {}}).to_string(),
    )
}

fn text(v: &Value, key: &str) -> Result<String, String> {
    match v.get(key).and_then(Value::as_str) {
        Some(s) if !s.is_empty() => Ok(s.to_string()),
        _ => Err(format!("record lacks {key}")),
    }
}

fn record(v: &Value) -> Result<Record, String> {
    let interest = match v.get("interest") {
        None => vec![],
        Some(a) => a
            .as_array()
            .ok_or("interest must be a list")?
            .iter()
            .filter_map(|s| s.as_str().map(String::from))
            .collect(),
    };
    Ok(Record {
        callsign: text(v, "callsign")?,
        prefix60: Prefix60::parse(&text(v, "prefix60")?)?,
        public_key: text(v, "public_key")?,
        endpoint: text(v, "endpoint")?,
        interest,
    })
}

/// The `result` of a JSON-RPC answer (or the object itself) with `status` checked; `Ok(None)` for an
/// error status ("not found", "refused": the caller learns nothing about which).
fn payload(body: &str) -> Result<Option<Value>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let v = v.get("result").cloned().unwrap_or(v);
    match v.get("status").and_then(Value::as_str) {
        Some("success") => Ok(Some(v)),
        Some(_) => Ok(None),
        None => Err("answer has no status".into()),
    }
}

// [@ANCHOR: hams_net_core_directory_parse_lookup]
/// Parse a lookup answer: `Ok(None)` when the directory has no (visible) record.
pub fn parse_lookup(body: &str) -> Result<Option<Record>, String> {
    payload(body)?.map(|v| record(&v)).transpose()
}

// [@ANCHOR: hams_net_core_directory_parse_hubs]
/// Parse a hubs answer (empty when refused).
pub fn parse_hubs(body: &str) -> Result<Vec<Record>, String> {
    match payload(body)? {
        None => Ok(vec![]),
        Some(v) => v
            .get("hubs")
            .and_then(Value::as_array)
            .ok_or("hubs must be a list")?
            .iter()
            .map(record)
            .collect(),
    }
}
