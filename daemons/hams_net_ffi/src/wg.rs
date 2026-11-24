//! One WireGuard session with one peer (userspace, boringtun): the Android app reads IPv6 packets from its VPN interface, hands them
//! to the session of the peer the peer table chose and sends the returned datagrams over a protected UDP socket, and the reverse.
//! Only IPv6 comes out of a session; the caller checks the inner source address against the peer's /60 (cryptokey routing).

use crate::{refused, NetError};
use boringtun::noise::{Tunn, TunnResult};
use boringtun::x25519::{PublicKey, StaticSecret};
use std::sync::{Arc, Mutex};

const BUF: usize = 65_535 + 64;

fn key32(bytes: &[u8], what: &str) -> Result<[u8; 32], NetError> {
    <[u8; 32]>::try_from(bytes).map_err(|_| refused(format!("{what} must be 32 bytes")))
}

// [@ANCHOR: hams_net_ffi_wg_public_key]
/// The WireGuard public key for a 32-byte private key.
#[uniffi::export]
pub fn wg_public_key(private_key: Vec<u8>) -> Result<Vec<u8>, NetError> {
    let secret = StaticSecret::from(key32(&private_key, "private key")?);
    Ok(PublicKey::from(&secret).as_bytes().to_vec())
}

/// What one received datagram produced.
#[derive(uniffi::Record)]
pub struct WgReceived {
    /// Datagrams to send back to the peer (handshake replies, keepalives, queued packets).
    pub replies: Vec<Vec<u8>>,
    /// The decrypted IPv6 packet for the VPN interface, if the datagram carried one.
    pub packet: Option<Vec<u8>>,
}

/// A userspace WireGuard session with a single peer.
#[derive(uniffi::Object)]
pub struct WgSession(Mutex<Tunn>);

#[uniffi::export]
impl WgSession {
    // [@ANCHOR: hams_net_ffi_wg_session_new]
    /// A session for our private key and the peer's public key (both 32 bytes); `index` tells sessions apart.
    #[uniffi::constructor]
    pub fn new(
        private_key: Vec<u8>,
        peer_public: Vec<u8>,
        index: u32,
    ) -> Result<Arc<Self>, NetError> {
        let secret = StaticSecret::from(key32(&private_key, "private key")?);
        let peer = PublicKey::from(key32(&peer_public, "peer public key")?);
        Ok(Arc::new(Self(Mutex::new(Tunn::new(
            secret,
            peer,
            None,
            Some(25),
            index & 0x00ff_ffff,
            None,
        )))))
    }

    // [@ANCHOR: hams_net_ffi_wg_encapsulate]
    /// An IPv6 packet from the VPN interface: the datagrams to send (a handshake first when no session is up; the packet is queued).
    pub fn encapsulate(&self, packet: Vec<u8>) -> Result<Vec<Vec<u8>>, NetError> {
        let mut tunn = self
            .0
            .lock()
            .map_err(|_| refused("session lock poisoned"))?;
        let mut out = vec![0u8; packet.len() + 148];
        Ok(match tunn.encapsulate(&packet, &mut out) {
            TunnResult::WriteToNetwork(d) => vec![d.to_vec()],
            TunnResult::Err(e) => return Err(refused(format!("{e:?}"))),
            _ => vec![],
        })
    }

    // [@ANCHOR: hams_net_ffi_wg_decapsulate]
    /// A datagram from the peer's endpoint.
    pub fn decapsulate(&self, datagram: Vec<u8>) -> Result<WgReceived, NetError> {
        let mut tunn = self
            .0
            .lock()
            .map_err(|_| refused("session lock poisoned"))?;
        let mut out = vec![0u8; BUF];
        let mut replies = Vec::new();
        let mut packet = None;
        let mut input: &[u8] = &datagram;
        loop {
            match tunn.decapsulate(None, input, &mut out) {
                TunnResult::WriteToNetwork(d) => replies.push(d.to_vec()),
                TunnResult::WriteToTunnelV6(d, _) => {
                    packet = Some(d.to_vec());
                    break;
                }
                TunnResult::Err(e) => return Err(refused(format!("{e:?}"))),
                _ => break, // Done, or IPv4: not carried here
            }
            input = &[]; // repeat with an empty datagram until Done
        }
        Ok(WgReceived { replies, packet })
    }

    // [@ANCHOR: hams_net_ffi_wg_tick]
    /// Call about every 250 ms: handshake retries, keepalives, rekeys; returns datagrams to send.
    pub fn tick(&self) -> Result<Vec<Vec<u8>>, NetError> {
        let mut tunn = self
            .0
            .lock()
            .map_err(|_| refused("session lock poisoned"))?;
        let mut out = vec![0u8; 256];
        Ok(match tunn.update_timers(&mut out) {
            TunnResult::WriteToNetwork(d) => vec![d.to_vec()],
            _ => vec![],
        })
    }
}
