//! DANE-EE for `.ham` servers, the part with no I/O: TLSA matching, the DNS query packet and the response parser. The
//! consumer sends the packet to its resolver and does the TLS handshake (`ham_proxy::dane`).

use crate::crypto::Crypto;

// [@ANCHOR: hams_net_core_dane_tlsa_type]
/// One TLSA record: usage, selector, matching type, data.
pub type Tlsa = (u8, u8, u8, Vec<u8>);
const TYPE_TLSA: u16 = 52;
const DANE_EE: u8 = 3;
const FULL_CERT: u8 = 0;

// [@ANCHOR: hams_net_core_dane_tlsa_matches]
/// True if a DANE-EE (3), full-certificate (0) record with matching type 0, 1 or 2 pins `cert_der`.
/// No records, or only other kinds, means refuse.
pub fn tlsa_matches(crypto: &dyn Crypto, cert_der: &[u8], records: &[Tlsa]) -> bool {
    records.iter().any(|(usage, selector, matching, data)| {
        if *usage != DANE_EE || *selector != FULL_CERT {
            return false;
        }
        match *matching {
            0 => cert_der == data.as_slice(),
            1 => crypto.sha256(cert_der).is_some_and(|d| d == *data),
            2 => crypto.sha512(cert_der).is_some_and(|d| d == *data),
            _ => false,
        }
    })
}

// [@ANCHOR: hams_net_core_dane_tlsa_query]
/// A DNS query packet for the TLSA records of `_port._tcp.host`.
pub fn tlsa_query(host: &str, port: u16, id: u16) -> Vec<u8> {
    let mut q = id.to_be_bytes().to_vec();
    q.extend_from_slice(&[1, 0, 0, 1, 0, 0, 0, 0, 0, 0]); // RD, one question
    let name = format!("_{port}._tcp.{}", host.trim_end_matches('.'));
    for label in name.split('.') {
        q.push(label.len().min(63) as u8);
        q.extend_from_slice(&label.as_bytes()[..label.len().min(63)]);
    }
    q.push(0);
    q.extend_from_slice(&TYPE_TLSA.to_be_bytes());
    q.extend_from_slice(&[0, 1]); // class IN
    q
}

// [@ANCHOR: hams_net_core_dane_skip_name]
fn skip_name(p: &[u8], mut i: usize) -> Option<usize> {
    loop {
        let l = *p.get(i)? as usize;
        if l == 0 {
            return Some(i + 1);
        }
        if l & 0xC0 == 0xC0 {
            return Some(i + 2);
        }
        i += 1 + l;
    }
}

// [@ANCHOR: hams_net_core_dane_parse_tlsa]
/// The TLSA records in a DNS response; empty on an error response or one that does not parse.
pub fn parse_tlsa(p: &[u8]) -> Vec<Tlsa> {
    let mut out = Vec::new();
    let u16at = |i: usize| {
        p.get(i..i + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]) as usize)
    };
    let (Some(qd), Some(an)) = (u16at(4), u16at(6)) else {
        return out;
    };
    if p.len() < 12 || p[3] & 0x0F != 0 {
        return out;
    }
    let mut i = 12;
    for _ in 0..qd {
        let Some(j) = skip_name(p, i) else { return out };
        i = j + 4;
    }
    for _ in 0..an {
        let Some(j) = skip_name(p, i) else { return out };
        let (Some(rtype), Some(rdlen)) = (u16at(j), u16at(j + 8)) else {
            return out;
        };
        let Some(rd) = p.get(j + 10..j + 10 + rdlen) else {
            return out;
        };
        if rtype == TYPE_TLSA as usize && rd.len() >= 3 {
            out.push((rd[0], rd[1], rd[2], rd[3..].to_vec()));
        }
        i = j + 10 + rdlen;
    }
    out
}
