// SPDX-License-Identifier: AGPL-3.0-or-later
//! A small, sans-IO WebSocket (RFC 6455) client and server codec, for the Home Assistant driver
//! and its fake server. Text frames, ping and pong, close, and fragmented messages; no extensions,
//! no compression, no TLS. The handshake helpers include the SHA-1 and base64 the accept key
//! needs, so the crate adds no dependency for them.
//!
//! The decoder accepts masked frames (a server reading a client) and unmasked frames (a client
//! reading a server) and bounds every message, so a hostile or broken peer cannot make it
//! allocate without limit.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

/// The GUID RFC 6455 appends to the client key.
const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

// [@ANCHOR: radioctl_station_ws_sha1]
/// SHA-1 of `data`. Used only for the WebSocket accept key, which is not a security boundary.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in message.as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for (slot, word) in w.iter_mut().zip(block.as_chunks::<4>().0) {
            *slot = u32::from_be_bytes(*word);
        }
        for i in 16..80 {
            let v = w.get(i - 3).copied().unwrap_or(0)
                ^ w.get(i - 8).copied().unwrap_or(0)
                ^ w.get(i - 14).copied().unwrap_or(0)
                ^ w.get(i - 16).copied().unwrap_or(0);
            if let Some(slot) = w.get_mut(i) {
                *slot = v.rotate_left(1);
            }
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (slot, add) in h.iter_mut().zip([a, b, c, d, e]) {
            *slot = slot.wrapping_add(add);
        }
    }
    let mut out = [0u8; 20];
    for (chunk, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(h) {
        *chunk = word.to_be_bytes();
    }
    out
}

// [@ANCHOR: radioctl_station_ws_base64]
/// Standard base64 with padding.
pub fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for shift in [18u32, 12, 6, 0] {
            out.push(char::from(
                TABLE
                    .get(((n >> shift) & 63) as usize)
                    .copied()
                    .unwrap_or(b'A'),
            ));
        }
        if chunk.len() < 3 {
            out.pop();
            if chunk.len() == 1 {
                out.pop();
                out.push_str("==");
            } else {
                out.push('=');
            }
        }
    }
    out
}

// [@ANCHOR: radioctl_station_ws_accept_key]
/// The `Sec-WebSocket-Accept` value for a client key.
pub fn accept_key(key: &str) -> String {
    base64(&sha1(format!("{key}{GUID}").as_bytes()))
}

// [@ANCHOR: radioctl_station_ws_random]
/// Bytes that differ from run to run, from the standard library's hasher seeds. The WebSocket
/// mask and key only have to be unpredictable to caching proxies; they are not secrets.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    for chunk in out.chunks_mut(8) {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_usize(N);
        let bytes = hasher.finish().to_le_bytes();
        for (slot, byte) in chunk.iter_mut().zip(bytes) {
            *slot = byte;
        }
    }
    out
}

// [@ANCHOR: radioctl_station_ws_client_key]
/// A fresh `Sec-WebSocket-Key`.
pub fn client_key() -> String {
    base64(&random_bytes::<16>())
}

// [@ANCHOR: radioctl_station_ws_handshake_request]
/// The HTTP upgrade request of a client. The token is not part of it: Home Assistant
/// authenticates inside the WebSocket.
pub fn handshake_request(host: &str, path: &str, key: &str) -> String {
    format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )
}

// [@ANCHOR: radioctl_station_ws_check_response]
/// Checks the server's answer to the upgrade: status 101 and the right accept key.
pub fn check_handshake_response(head: &str, key: &str) -> Result<(), String> {
    let status = head.lines().next().unwrap_or("");
    if !status.contains(" 101") {
        return Err(format!("the server answered \"{}\"", status.trim()));
    }
    let want = accept_key(key);
    let got = head.lines().find_map(|l| {
        let (name, value) = l.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("sec-websocket-accept")
            .then(|| value.trim().to_string())
    });
    match got {
        Some(g) if g == want => Ok(()),
        Some(_) => Err("the server's accept key is wrong".into()),
        None => Err("the server sent no accept key".into()),
    }
}

/// A WebSocket frame type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opcode {
    /// A continuation of a fragmented message.
    Continuation,
    /// UTF-8 text.
    Text,
    /// Binary data.
    Binary,
    /// Close.
    Close,
    /// Ping.
    Ping,
    /// Pong.
    Pong,
}

impl Opcode {
    // [@ANCHOR: radioctl_station_ws_opcode_code]
    /// The wire value.
    fn code(self) -> u8 {
        match self {
            Opcode::Continuation => 0,
            Opcode::Text => 1,
            Opcode::Binary => 2,
            Opcode::Close => 8,
            Opcode::Ping => 9,
            Opcode::Pong => 10,
        }
    }

    // [@ANCHOR: radioctl_station_ws_opcode_from_code]
    /// The type for a wire value, if it is one this codec knows.
    fn from_code(code: u8) -> Option<Opcode> {
        Some(match code {
            0 => Opcode::Continuation,
            1 => Opcode::Text,
            2 => Opcode::Binary,
            8 => Opcode::Close,
            9 => Opcode::Ping,
            10 => Opcode::Pong,
            _ => return None,
        })
    }
}

/// One frame as read: final-fragment flag, type, unmasked payload.
type Frame = (bool, Opcode, Vec<u8>);

/// A whole message (fragments joined) or control frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Its type; never `Continuation`.
    pub opcode: Opcode,
    /// Its payload.
    pub payload: Vec<u8>,
}

// [@ANCHOR: radioctl_station_ws_encode]
/// One final frame. A client masks (`Some`), a server does not (`None`).
pub fn encode_frame(opcode: Opcode, payload: &[u8], mask: Option<[u8; 4]>) -> Vec<u8> {
    let mut out = vec![0x80 | opcode.code()];
    let flag = if mask.is_some() { 0x80 } else { 0 };
    let len = payload.len();
    if len < 126 {
        out.push(flag | len as u8);
    } else if let Ok(short) = u16::try_from(len) {
        out.push(flag | 126);
        out.extend_from_slice(&short.to_be_bytes());
    } else {
        out.push(flag | 127);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }
    match mask {
        Some(m) => {
            out.extend_from_slice(&m);
            out.extend(payload.iter().enumerate().map(|(i, b)| b ^ m[i % 4]));
        }
        None => out.extend_from_slice(payload),
    }
    out
}

/// Reassembles frames from a byte stream.
#[derive(Debug)]
pub struct Decoder {
    buffer: Vec<u8>,
    partial: Option<(Opcode, Vec<u8>)>,
    max_message: usize,
}

impl Decoder {
    // [@ANCHOR: radioctl_station_ws_decoder_new]
    /// A decoder that refuses any message over `max_message` bytes.
    pub fn new(max_message: usize) -> Decoder {
        Decoder {
            buffer: Vec::new(),
            partial: None,
            max_message,
        }
    }

    // [@ANCHOR: radioctl_station_ws_decoder_push]
    /// Adds bytes and returns every message completed. An error means the stream is broken or
    /// hostile and the connection must be dropped.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Message>, String> {
        self.buffer.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some((frame, used)) = self.frame()? {
            self.buffer.drain(..used);
            let (fin, opcode, payload) = frame;
            match opcode {
                Opcode::Ping | Opcode::Pong | Opcode::Close => {
                    if !fin || payload.len() > 125 {
                        return Err("a control frame is fragmented or too long".into());
                    }
                    out.push(Message { opcode, payload });
                }
                Opcode::Text | Opcode::Binary => {
                    if self.partial.is_some() {
                        return Err("a new message began inside a fragmented one".into());
                    }
                    if fin {
                        out.push(Message { opcode, payload });
                    } else {
                        self.partial = Some((opcode, payload));
                    }
                }
                Opcode::Continuation => {
                    let Some((first, mut joined)) = self.partial.take() else {
                        return Err("a continuation with nothing to continue".into());
                    };
                    joined.extend_from_slice(&payload);
                    if joined.len() > self.max_message {
                        return Err("message too large".into());
                    }
                    if fin {
                        out.push(Message {
                            opcode: first,
                            payload: joined,
                        });
                    } else {
                        self.partial = Some((first, joined));
                    }
                }
            }
        }
        Ok(out)
    }

    // [@ANCHOR: radioctl_station_ws_decoder_frame]
    /// The first complete frame in the buffer and how many bytes it used, if there is one.
    fn frame(&self) -> Result<Option<(Frame, usize)>, String> {
        let b = &self.buffer;
        let (Some(&b0), Some(&b1)) = (b.first(), b.get(1)) else {
            return Ok(None);
        };
        if b0 & 0x70 != 0 {
            return Err("a reserved bit is set (extensions are not negotiated)".into());
        }
        let opcode = Opcode::from_code(b0 & 0x0f).ok_or("unknown opcode")?;
        let masked = b1 & 0x80 != 0;
        let (len, mut at) = match b1 & 0x7f {
            126 => match b.get(2..4) {
                Some(s) => (usize::from(u16::from_be_bytes([s[0], s[1]])), 4),
                None => return Ok(None),
            },
            127 => match b.get(2..10) {
                Some(s) => {
                    let mut eight = [0u8; 8];
                    eight.copy_from_slice(s);
                    (
                        usize::try_from(u64::from_be_bytes(eight))
                            .map_err(|_| "frame length is absurd")?,
                        10,
                    )
                }
                None => return Ok(None),
            },
            n => (usize::from(n), 2),
        };
        if len > self.max_message {
            return Err("frame too large".into());
        }
        let mask = if masked {
            let Some(m) = b.get(at..at + 4) else {
                return Ok(None);
            };
            at += 4;
            Some([m[0], m[1], m[2], m[3]])
        } else {
            None
        };
        let Some(raw) = b.get(at..at + len) else {
            return Ok(None);
        };
        let payload: Vec<u8> = match mask {
            Some(m) => raw.iter().enumerate().map(|(i, x)| x ^ m[i % 4]).collect(),
            None => raw.to_vec(),
        };
        Ok(Some(((b0 & 0x80 != 0, opcode, payload), at + len)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: radioctl_station_ws_sha1]
    // Tests [@ANCHOR: radioctl_station_ws_base64]
    // Tests [@ANCHOR: radioctl_station_ws_accept_key]
    #[test]
    fn the_accept_key_matches_the_rfc_example() {
        // RFC 6455 section 1.3.
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
        // SHA-1 test vectors (FIPS 180).
        let hex = |d: [u8; 20]| d.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(
            hex(sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(hex(sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            hex(sha1(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b""), "");
    }

    // Tests [@ANCHOR: radioctl_station_ws_random]
    // Tests [@ANCHOR: radioctl_station_ws_client_key]
    // Tests [@ANCHOR: radioctl_station_ws_handshake_request]
    // Tests [@ANCHOR: radioctl_station_ws_check_response]
    #[test]
    fn the_handshake_is_checked() {
        let key = client_key();
        assert_ne!(key, client_key(), "keys differ from call to call");
        let req = handshake_request("10.0.0.5:8123", "/api/websocket", &key);
        assert!(req.starts_with("GET /api/websocket HTTP/1.1\r\n"));
        assert!(req.contains(&format!("Sec-WebSocket-Key: {key}")));
        let good = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nsec-websocket-accept: {}\r\n\r\n",
            accept_key(&key)
        );
        assert!(check_handshake_response(&good, &key).is_ok());
        assert!(check_handshake_response("HTTP/1.1 403 Forbidden\r\n\r\n", &key).is_err());
        assert!(check_handshake_response(
            "HTTP/1.1 101 Switching\r\nSec-WebSocket-Accept: nope\r\n\r\n",
            &key
        )
        .is_err());
        assert!(check_handshake_response("HTTP/1.1 101 Switching\r\n\r\n", &key).is_err());
    }

    // Tests [@ANCHOR: radioctl_station_ws_encode]
    // Tests [@ANCHOR: radioctl_station_ws_opcode_code]
    // Tests [@ANCHOR: radioctl_station_ws_opcode_from_code]
    // Tests [@ANCHOR: radioctl_station_ws_decoder_new]
    // Tests [@ANCHOR: radioctl_station_ws_decoder_push]
    // Tests [@ANCHOR: radioctl_station_ws_decoder_frame]
    #[test]
    fn frames_round_trip_masked_unmasked_fragmented_and_byte_at_a_time() {
        let long = vec![b'x'; 70_000];
        for payload in [&b""[..], b"hi", &[b'y'; 125], &[b'z'; 126], &long] {
            for mask in [None, Some([1, 2, 3, 4])] {
                let wire = encode_frame(Opcode::Text, payload, mask);
                let mut decoder = Decoder::new(1 << 20);
                let mut got = Vec::new();
                for byte in &wire {
                    got.extend(decoder.push(std::slice::from_ref(byte)).unwrap());
                }
                assert_eq!(
                    got,
                    vec![Message {
                        opcode: Opcode::Text,
                        payload: payload.to_vec()
                    }]
                );
            }
        }
        // Fragmented: "he" + "llo", a ping in between is delivered on its own.
        let mut wire = vec![0x01, 2, b'h', b'e'];
        wire.extend(encode_frame(Opcode::Ping, b"p", None));
        wire.extend([0x80, 3, b'l', b'l', b'o']);
        let mut decoder = Decoder::new(1024);
        let got = decoder.push(&wire).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].opcode, Opcode::Ping);
        assert_eq!(got[1].payload, b"hello");
    }

    // Tests [@ANCHOR: radioctl_station_ws_decoder_push]
    #[test]
    fn hostile_streams_are_refused() {
        let mut d = Decoder::new(100);
        assert!(d
            .push(&encode_frame(Opcode::Text, &[0; 101], None))
            .is_err());
        assert!(Decoder::new(100).push(&[0xC1, 0]).is_err()); // reserved bit
        assert!(Decoder::new(100).push(&[0x83, 0]).is_err()); // unknown opcode
        assert!(Decoder::new(100).push(&[0x80, 0]).is_err()); // continuation from nothing
        assert!(Decoder::new(100).push(&[0x09, 0]).is_err()); // fragmented control frame
        let mut d = Decoder::new(100);
        assert!(d.push(&[0x01, 1, b'a']).unwrap().is_empty());
        assert!(d.push(&[0x01, 1, b'b']).is_err()); // a new message inside a fragmented one
                                                    // A 64-bit length that does not fit is refused, not allocated.
        let mut huge = vec![0x81, 127];
        huge.extend_from_slice(&u64::MAX.to_be_bytes());
        assert!(Decoder::new(1 << 20).push(&huge).is_err());
    }
}
