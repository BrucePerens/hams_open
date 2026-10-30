// SPDX-License-Identifier: AGPL-3.0-or-later
//! Type/length/value encoding used by `radiod` for commands and status.
//!
//! A datagram is one packet-kind byte (0 status, 1 command) followed by entries
//! `type, length, value` and ends with an `EOL` entry (type 0, no length). Lengths 0..=127 are
//! direct; otherwise the high bit is set and the low 7 bits give the count of big-endian length
//! bytes. Integers are big-endian with leading zero bytes removed (zero may have length 0).

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// Status/command type identifiers (values from `status.h` of the pinned commit; verified by
/// compiling the enum).
pub mod ty {
    /// End of list.
    pub const EOL: u8 = 0;
    /// Echoed tag.
    pub const COMMAND_TAG: u8 = 1;
    /// Number of commands received.
    pub const CMD_CNT: u8 = 2;
    /// GPS time, ns.
    pub const GPS_TIME: u8 = 3;
    /// Free-form description.
    pub const DESCRIPTION: u8 = 4;
    /// Status destination socket.
    pub const STATUS_DEST_SOCKET: u8 = 5;
    /// 1-byte spectrum bins.
    pub const BIN_BYTE_DATA: u8 = 9;
    /// Front end sample rate.
    pub const INPUT_SAMPRATE: u8 = 10;
    /// Base level of byte spectrum, dB.
    pub const SPECTRUM_BASE: u8 = 11;
    /// FFTs averaged per response.
    pub const SPECTRUM_AVG: u8 = 12;
    /// Spectrum window type.
    pub const WINDOW_TYPE: u8 = 14;
    /// Noise bandwidth of a spectrum bin, bins.
    pub const NOISE_BW: u8 = 15;
    /// RTP source socket.
    pub const OUTPUT_DATA_SOURCE_SOCKET: u8 = 16;
    /// RTP destination socket (never sent by this client).
    pub const OUTPUT_DATA_DEST_SOCKET: u8 = 17;
    /// RTP SSRC.
    pub const OUTPUT_SSRC: u8 = 18;
    /// RTP TTL.
    pub const OUTPUT_TTL: u8 = 19;
    /// Output sample rate.
    pub const OUTPUT_SAMPRATE: u8 = 20;
    /// RTP packets sent.
    pub const OUTPUT_DATA_PACKETS: u8 = 22;
    /// Carrier frequency, Hz (double).
    pub const RADIO_FREQUENCY: u8 = 33;
    /// First LO.
    pub const FIRST_LO_FREQUENCY: u8 = 34;
    /// Filter low edge, Hz (float).
    pub const LOW_EDGE: u8 = 39;
    /// Filter high edge, Hz (float).
    pub const HIGH_EDGE: u8 = 40;
    /// Kaiser beta.
    pub const KAISER_BETA: u8 = 41;
    /// IF power, dB.
    pub const IF_POWER: u8 = 45;
    /// Baseband power, dB.
    pub const BASEBAND_POWER: u8 = 46;
    /// Noise density, dB/Hz.
    pub const NOISE_DENSITY: u8 = 47;
    /// Demodulator type (0 linear, 1 FM, 2 WFM, 3 spectrum, 4 spectrum byte data).
    pub const DEMOD_TYPE: u8 = 48;
    /// Output channels.
    pub const OUTPUT_CHANNELS: u8 = 49;
    /// Envelope detection (linear).
    pub const ENVELOPE: u8 = 56;
    /// SNR squelch enable.
    pub const SNR_SQUELCH: u8 = 57;
    /// Frequency offset, Hz.
    pub const FREQ_OFFSET: u8 = 59;
    /// Peak deviation, Hz.
    pub const PEAK_DEVIATION: u8 = 60;
    /// AGC enable.
    pub const AGC_ENABLE: u8 = 62;
    /// Headroom, dB.
    pub const HEADROOM: u8 = 63;
    /// Manual gain, dB.
    pub const GAIN: u8 = 68;
    /// Output level, dB.
    pub const OUTPUT_LEVEL: u8 = 69;
    /// Output samples sent.
    pub const OUTPUT_SAMPLES: u8 = 70;
    /// Opus bit rate.
    pub const OPUS_BIT_RATE: u8 = 71;
    /// Spectrum FFT length.
    pub const SPECTRUM_FFT_N: u8 = 76;
    /// Dropped filter blocks.
    pub const FILTER_DROPS: u8 = 77;
    /// A/D bits.
    pub const AD_BITS_PER_SAMPLE: u8 = 82;
    /// FM de-emphasis time constant, us.
    pub const DEEMPH_TC: u8 = 86;
    /// FM de-emphasis gain, dB.
    pub const DEEMPH_GAIN: u8 = 87;
    /// FM threshold extension.
    pub const THRESH_EXTEND: u8 = 90;
    /// Spectrum window shape.
    pub const SPECTRUM_SHAPE: u8 = 91;
    /// Spectrum bin width, Hz.
    pub const RESOLUTION_BW: u8 = 93;
    /// Spectrum bin count.
    pub const BIN_COUNT: u8 = 94;
    /// Spectrum algorithm crossover, Hz.
    pub const CROSSOVER: u8 = 95;
    /// Float spectrum bins (power, linear).
    pub const BIN_DATA: u8 = 96;
    /// Front end A/D overrange count.
    pub const AD_OVER: u8 = 104;
    /// RTP payload type.
    pub const RTP_PT: u8 = 105;
    /// Status interval, frames.
    pub const STATUS_INTERVAL: u8 = 106;
    /// Output encoding (see [`crate::rtp::Encoding`]).
    pub const OUTPUT_ENCODING: u8 = 107;
    /// Samples since last overrange.
    pub const SAMPLES_SINCE_OVER: u8 = 108;
    /// dBm-to-dBFS calibration.
    pub const RF_LEVEL_CAL: u8 = 110;
    /// Byte spectrum step, dB.
    pub const SPECTRUM_STEP: u8 = 115;
    /// Spectrum FFT overlap.
    pub const SPECTRUM_OVERLAP: u8 = 116;
    /// Remaining channel lifetime, frames.
    pub const LIFETIME: u8 = 117;
}

/// Why a datagram was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TlvError {
    /// Datagram shorter than a packet kind and one entry.
    TooShort,
    /// An entry's declared length runs past the end of the datagram.
    Truncated,
    /// A length-of-length field larger than four bytes.
    BadLength,
    /// The packet-kind byte was neither 0 nor 1.
    BadKind(u8),
    /// A value had a length that its type cannot have.
    BadValue(u8),
}

impl fmt::Display for TlvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => write!(f, "datagram too short"),
            Self::Truncated => write!(f, "entry runs past end of datagram"),
            Self::BadLength => write!(f, "unsupported length-of-length"),
            Self::BadKind(k) => write!(f, "unknown packet kind {k}"),
            Self::BadValue(t) => write!(f, "malformed value for type {t}"),
        }
    }
}

impl std::error::Error for TlvError {}

/// Packet kind (first byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Status from `radiod`.
    Status,
    /// Command to `radiod`.
    Command,
}

/// One decoded entry; `value` borrows from the datagram.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    /// Type identifier.
    pub ty: u8,
    /// Raw value bytes.
    pub value: &'a [u8],
}

impl Entry<'_> {
    /// Unsigned integer (up to 8 bytes, big-endian, zero length is 0).
    pub fn uint(&self) -> Result<u64, TlvError> {
        if self.value.len() > 8 {
            return Err(TlvError::BadValue(self.ty));
        }
        Ok(self
            .value
            .iter()
            .fold(0u64, |a, b| (a << 8) | u64::from(*b)))
    }
    /// Boolean (any non-zero integer).
    pub fn bool(&self) -> Result<bool, TlvError> {
        Ok(self.uint()? != 0)
    }
    /// 32-bit float; a value longer than four bytes is read as a double, as `radiod` does.
    pub fn f32(&self) -> Result<f32, TlvError> {
        match self.value.len() {
            0..=4 => Ok(f32::from_bits(self.uint()? as u32)),
            5..=8 => Ok(f64::from_bits(self.uint()?) as f32),
            _ => Err(TlvError::BadValue(self.ty)),
        }
    }
    /// 64-bit float; a value of four bytes or fewer is read as a float.
    pub fn f64(&self) -> Result<f64, TlvError> {
        match self.value.len() {
            0..=4 => Ok(f64::from(f32::from_bits(self.uint()? as u32))),
            5..=8 => Ok(f64::from_bits(self.uint()?)),
            _ => Err(TlvError::BadValue(self.ty)),
        }
    }
    /// UTF-8 text (lossy, so a hostile description cannot fail the parse).
    pub fn text(&self) -> String {
        String::from_utf8_lossy(self.value).into_owned()
    }
    /// Socket: 6 bytes IPv4 plus port, 18 bytes IPv6 plus port, or empty (null socket).
    pub fn socket(&self) -> Result<Option<SocketAddr>, TlvError> {
        match self.value.len() {
            0 => Ok(None),
            6 => {
                let v = self.value;
                let ip = Ipv4Addr::new(v[0], v[1], v[2], v[3]);
                Ok(Some(SocketAddr::new(
                    IpAddr::V4(ip),
                    u16::from_be_bytes([v[4], v[5]]),
                )))
            }
            18 => {
                let mut o = [0u8; 16];
                o.copy_from_slice(&self.value[..16]);
                let port = u16::from_be_bytes([self.value[16], self.value[17]]);
                Ok(Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::from(o)), port)))
            }
            _ => Err(TlvError::BadValue(self.ty)),
        }
    }
    /// A vector of big-endian `f32` (`BIN_DATA`).
    pub fn f32_vec(&self) -> Result<Vec<f32>, TlvError> {
        if !self.value.len().is_multiple_of(4) {
            return Err(TlvError::BadValue(self.ty));
        }
        Ok(self
            .value
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_be_bytes(*c))
            .collect())
    }
}

/// Parse a datagram into its kind and entries. Never panics; every length is checked against the
/// remaining bytes. Parsing stops at `EOL` or at the end of the datagram.
pub fn parse(pkt: &[u8]) -> Result<(Kind, Vec<Entry<'_>>), TlvError> {
    let (&k, mut rest) = pkt.split_first().ok_or(TlvError::TooShort)?;
    let kind = match k {
        0 => Kind::Status,
        1 => Kind::Command,
        other => return Err(TlvError::BadKind(other)),
    };
    let mut out = Vec::new();
    while let Some((&t, r)) = rest.split_first() {
        if t == ty::EOL {
            break;
        }
        let (&l0, r) = r.split_first().ok_or(TlvError::Truncated)?;
        let (len, r) = if l0 & 0x80 == 0 {
            (usize::from(l0), r)
        } else {
            let n = usize::from(l0 & 0x7f);
            if n == 0 || n > 4 {
                return Err(TlvError::BadLength);
            }
            if r.len() < n {
                return Err(TlvError::Truncated);
            }
            let len = r[..n]
                .iter()
                .fold(0usize, |a, b| (a << 8) | usize::from(*b));
            (len, &r[n..])
        };
        if r.len() < len {
            return Err(TlvError::Truncated);
        }
        out.push(Entry {
            ty: t,
            value: &r[..len],
        });
        rest = &r[len..];
    }
    Ok((kind, out))
}

/// Builder for a command datagram. Crate-private on purpose: callers go through
/// [`crate::command::Command`].
#[derive(Debug)]
pub(crate) struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub(crate) fn command() -> Self {
        Self { buf: vec![1] }
    }
    fn header(&mut self, t: u8, len: usize) {
        self.buf.push(t);
        // Command values here are at most a few bytes; the long form is never needed.
        self.buf.push(len as u8);
    }
    pub(crate) fn uint(&mut self, t: u8, v: u64) {
        let bytes = v.to_be_bytes();
        let skip = bytes.iter().take_while(|b| **b == 0).count();
        self.header(t, 8 - skip);
        self.buf.extend_from_slice(&bytes[skip..]);
    }
    pub(crate) fn bool(&mut self, t: u8, v: bool) {
        self.uint(t, u64::from(v));
    }
    pub(crate) fn f32(&mut self, t: u8, v: f32) {
        self.header(t, 4);
        self.buf.extend_from_slice(&v.to_be_bytes());
    }
    pub(crate) fn f64(&mut self, t: u8, v: f64) {
        self.header(t, 8);
        self.buf.extend_from_slice(&v.to_be_bytes());
    }
    pub(crate) fn finish(mut self) -> Vec<u8> {
        self.buf.push(ty::EOL);
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_round_trip_strips_leading_zeroes() {
        let mut w = Writer::command();
        w.uint(ty::COMMAND_TAG, 0x1234);
        w.uint(ty::LIFETIME, 0);
        w.f64(ty::RADIO_FREQUENCY, 7_074_000.0);
        let pkt = w.finish();
        let (k, e) = parse(&pkt).unwrap_or_else(|_| (Kind::Status, vec![]));
        assert_eq!(k, Kind::Command);
        assert_eq!(e[0].value, &[0x12, 0x34]);
        assert_eq!(e[0].uint(), Ok(0x1234));
        assert_eq!(e[1].value.len(), 0);
        assert_eq!(e[2].f64(), Ok(7_074_000.0));
    }

    #[test]
    fn long_form_length() {
        let mut pkt = vec![0u8, ty::BIN_DATA, 0x82, 0x01, 0x00];
        pkt.extend(std::iter::repeat_n(0u8, 256));
        pkt.push(0);
        let (_, e) = parse(&pkt).unwrap_or_else(|_| (Kind::Status, vec![]));
        assert_eq!(e[0].value.len(), 256);
    }

    #[test]
    fn hostile_inputs_are_errors_not_panics() {
        assert_eq!(parse(&[]).err(), Some(TlvError::TooShort));
        assert_eq!(parse(&[7]).err(), Some(TlvError::BadKind(7)));
        assert_eq!(parse(&[0, 5]).err(), Some(TlvError::Truncated));
        assert_eq!(parse(&[0, 5, 9, 1]).err(), Some(TlvError::Truncated));
        assert_eq!(
            parse(&[0, 5, 0x85, 1, 2, 3, 4, 5]).err(),
            Some(TlvError::BadLength)
        );
        assert_eq!(
            parse(&[0, 5, 0x84, 0xff, 0xff, 0xff, 0xff]).err(),
            Some(TlvError::Truncated)
        );
        assert_eq!(parse(&[0, 5, 0x80]).err(), Some(TlvError::BadLength));
    }

    #[test]
    fn socket_forms() {
        let e = Entry {
            ty: ty::OUTPUT_DATA_DEST_SOCKET,
            value: &[239, 1, 1, 2, 0x13, 0x8c],
        };
        assert_eq!(
            e.socket(),
            Ok(Some(
                "239.1.1.2:5004"
                    .parse()
                    .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0)))
            ))
        );
        let n = Entry { ty: 17, value: &[] };
        assert_eq!(n.socket(), Ok(None));
        let bad = Entry {
            ty: 17,
            value: &[1, 2, 3],
        };
        assert!(bad.socket().is_err());
    }
}

#[cfg(test)]
mod fuzz_like {
    use super::*;

    /// Cheap deterministic stand-in for fuzzing: the parser and every accessor must return, never panic.
    #[test]
    fn random_datagrams_never_panic() {
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..200_000 {
            let len = (next() % 80) as usize;
            let mut pkt: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            if let Some(b) = pkt.first_mut() {
                *b %= 2;
            }
            if let Ok((_, entries)) = parse(&pkt) {
                for e in entries {
                    let _ = (
                        e.uint(),
                        e.f32(),
                        e.f64(),
                        e.socket(),
                        e.f32_vec(),
                        e.text(),
                    );
                }
            }
            let _ = crate::status::ChannelStatus::decode(&pkt);
            let _ = crate::rtp::RtpPacket::parse(&pkt);
        }
    }
}
