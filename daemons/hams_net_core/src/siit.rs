//! Stateless SIIT mapping (RFC 6052 /96 format) for the translation /48; mirrors `ipv6_siit/siit_map.py`.
//! Address layout: `<translation /48>:<16-bit domain>:0:0:<IPv4>`. 64:ff9b::/96 is never used.

use std::net::{Ipv4Addr, Ipv6Addr};

/// A translation block: an IPv6 /48 (only the top 48 bits are kept).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block(u128);

const WELL_KNOWN_NAT64: u128 = 0x0064_ff9b << 96;

impl Block {
    /// Parse `"<addr>/48"`; refuses any other length and anything overlapping 64:ff9b::/96.
    // [@ANCHOR: siit_block_parse]
    pub fn parse(text: &str) -> Result<Block, String> {
        let (addr, len) = text
            .split_once('/')
            .ok_or("translation block must be an IPv6 /48")?;
        let addr: Ipv6Addr = addr
            .parse()
            .map_err(|_| "translation block must be an IPv6 /48".to_string())?;
        if len != "48" {
            return Err(format!("translation block must be an IPv6 /48: {text}"));
        }
        let base = u128::from(addr) & (!0u128 << 80);
        if base >> 32 == WELL_KNOWN_NAT64 >> 32 {
            return Err("64:ff9b::/96 is never used".into());
        }
        Ok(Block(base))
    }

    /// IPv6 address of IPv4 host `ipv4` in `domain`.
    // [@ANCHOR: siit_embed]
    pub fn embed(&self, domain: u16, ipv4: Ipv4Addr) -> Ipv6Addr {
        Ipv6Addr::from(self.0 | (u128::from(domain) << 64) | u128::from(u32::from(ipv4)))
    }

    /// Inverse of `embed`; refuses addresses outside the block or with bits 64..95 set.
    // [@ANCHOR: siit_extract]
    pub fn extract(&self, address: Ipv6Addr) -> Result<(u16, Ipv4Addr), String> {
        let value = u128::from(address);
        if value & (!0u128 << 80) != self.0 {
            return Err(format!("{address} is outside the block"));
        }
        if (value >> 32) & 0xFFFF_FFFF != 0 {
            return Err(format!(
                "{address} is not in the /96 format (bits 64..95 must be zero)"
            ));
        }
        Ok((
            ((value >> 64) & 0xFFFF) as u16,
            Ipv4Addr::from(value as u32),
        ))
    }
}
