// SPDX-License-Identifier: AGPL-3.0-or-later
//! RTP header parsing and payload decoding for `radiod` audio streams.
//!
//! The SSRC of a stream is the channel's SSRC. The payload type is dynamic: `radiod` hands them
//! out from 77 upward (skipping 100) as it first sees each (rate, channels, encoding)
//! combination, so the type is learned from the channel's status, never assumed.

use std::fmt;

/// Why a packet was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RtpError {
    /// Shorter than the fixed header, or the CSRC list / extension / padding run past the end.
    Truncated,
    /// RTP version other than 2.
    Version(u8),
    /// Payload bytes the codec cannot decode.
    Payload(String),
}

impl fmt::Display for RtpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for RtpError {}

/// A parsed RTP packet; the payload borrows from the datagram.
#[derive(Debug, Clone, Copy)]
pub struct RtpPacket<'a> {
    /// Payload type.
    pub pt: u8,
    /// Marker bit (set at the start of a talk spurt after silence).
    pub marker: bool,
    /// Sequence number.
    pub seq: u16,
    /// Timestamp, in samples of the stream's rate (Opus: 48 kHz ticks).
    pub timestamp: u32,
    /// Synchronization source.
    pub ssrc: u32,
    /// Payload.
    pub payload: &'a [u8],
}

impl<'a> RtpPacket<'a> {
    /// Parse a datagram.
    pub fn parse(d: &'a [u8]) -> Result<Self, RtpError> {
        if d.len() < 12 {
            return Err(RtpError::Truncated);
        }
        let version = d[0] >> 6;
        if version != 2 {
            return Err(RtpError::Version(version));
        }
        let pad = d[0] & 0x20 != 0;
        let ext = d[0] & 0x10 != 0;
        let cc = usize::from(d[0] & 0x0f);
        let mut off = 12 + 4 * cc;
        if ext {
            let h = d.get(off..off + 4).ok_or(RtpError::Truncated)?;
            off += 4 + 4 * usize::from(u16::from_be_bytes([h[2], h[3]]));
        }
        let mut end = d.len();
        if pad {
            end = end
                .checked_sub(usize::from(*d.last().ok_or(RtpError::Truncated)?))
                .ok_or(RtpError::Truncated)?;
        }
        if off > end {
            return Err(RtpError::Truncated);
        }
        Ok(Self {
            pt: d[1] & 0x7f,
            marker: d[1] & 0x80 != 0,
            seq: u16::from_be_bytes([d[2], d[3]]),
            timestamp: u32::from_be_bytes([d[4], d[5], d[6], d[7]]),
            ssrc: u32::from_be_bytes([d[8], d[9], d[10], d[11]]),
            payload: &d[off..end],
        })
    }
}

/// Output encodings from `rtp.h` that this client decodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// Signed 16-bit big-endian PCM (value 2 in `radiod`).
    S16Be,
    /// Signed 16-bit little-endian PCM (value 1).
    S16Le,
    /// Opus (value 3), 48 kHz stereo.
    Opus,
}

impl Encoding {
    /// Map `radiod`'s encoding number.
    pub fn from_radiod(n: u64) -> Option<Self> {
        match n {
            1 => Some(Self::S16Le),
            2 => Some(Self::S16Be),
            3 => Some(Self::Opus),
            _ => None,
        }
    }
}

/// Decoder for one stream. Output is mono `f32` (stereo is downmixed).
pub struct AudioDecoder {
    encoding: Encoding,
    channels: usize,
    #[cfg(feature = "opus")]
    opus: Option<opus::Decoder>,
}

impl fmt::Debug for AudioDecoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudioDecoder")
            .field("encoding", &self.encoding)
            .field("channels", &self.channels)
            .finish()
    }
}

impl AudioDecoder {
    /// Build a decoder from the channel status (`channels` for PCM; Opus is always 2).
    pub fn new(encoding: Encoding, channels: usize) -> Result<Self, RtpError> {
        match encoding {
            Encoding::Opus => {
                #[cfg(feature = "opus")]
                {
                    let d = opus::Decoder::new(48_000, opus::Channels::Stereo)
                        .map_err(|e| RtpError::Payload(e.to_string()))?;
                    Ok(Self {
                        encoding,
                        channels: 2,
                        opus: Some(d),
                    })
                }
                #[cfg(not(feature = "opus"))]
                Err(RtpError::Payload("built without the opus feature".into()))
            }
            _ => Ok(Self {
                encoding,
                channels: channels.clamp(1, 2),
                #[cfg(feature = "opus")]
                opus: None,
            }),
        }
    }

    /// Decode a payload to mono samples in -1.0..=1.0.
    pub fn decode(&mut self, payload: &[u8]) -> Result<Vec<f32>, RtpError> {
        match self.encoding {
            Encoding::S16Be | Encoding::S16Le => {
                let be = self.encoding == Encoding::S16Be;
                if !payload.len().is_multiple_of(2 * self.channels) {
                    return Err(RtpError::Payload(format!(
                        "{} bytes is not a whole number of frames",
                        payload.len()
                    )));
                }
                let s = |c: &[u8]| {
                    let v = if be {
                        i16::from_be_bytes([c[0], c[1]])
                    } else {
                        i16::from_le_bytes([c[0], c[1]])
                    };
                    f32::from(v) / 32768.0
                };
                Ok(payload
                    .chunks_exact(2 * self.channels)
                    .map(|f| {
                        f.as_chunks::<2>().0.iter().map(|c| s(&c[..])).sum::<f32>()
                            / self.channels as f32
                    })
                    .collect())
            }
            Encoding::Opus => {
                #[cfg(feature = "opus")]
                {
                    let dec = self
                        .opus
                        .as_mut()
                        .ok_or_else(|| RtpError::Payload("no decoder".into()))?;
                    // 120 ms at 48 kHz is the largest Opus frame.
                    let mut pcm = vec![0f32; 5760 * 2];
                    let n = dec
                        .decode_float(payload, &mut pcm, false)
                        .map_err(|e| RtpError::Payload(e.to_string()))?;
                    Ok(pcm[..n * 2]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|f| (f[0] + f[1]) * 0.5)
                        .collect())
                }
                #[cfg(not(feature = "opus"))]
                Err(RtpError::Payload("built without the opus feature".into()))
            }
        }
    }
}

/// Tracks sequence numbers to count loss and reordering.
#[derive(Debug, Default, Clone)]
pub struct SeqTracker {
    last: Option<u16>,
    /// Packets seen.
    pub received: u64,
    /// Packets missing in sequence (gaps).
    pub lost: u64,
    /// Packets that arrived out of order or duplicated.
    pub out_of_order: u64,
}

impl SeqTracker {
    /// Account for one packet.
    pub fn push(&mut self, seq: u16) {
        self.received += 1;
        if let Some(last) = self.last {
            let d = seq.wrapping_sub(last);
            match d {
                1 => {}
                0 => self.out_of_order += 1,
                2..=0x7fff => self.lost += u64::from(d) - 1,
                _ => {
                    self.out_of_order += 1;
                    return;
                }
            }
        }
        self.last = Some(seq);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkt(pt: u8, seq: u16, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![
            0x80,
            pt,
            (seq >> 8) as u8,
            seq as u8,
            0,
            0,
            0,
            1,
            0,
            0,
            0x27,
            0x10,
        ];
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn parses_header() {
        let d = pkt(111, 5, &[1, 2, 3, 4]);
        let p = RtpPacket::parse(&d).unwrap_or(RtpPacket {
            pt: 0,
            marker: false,
            seq: 0,
            timestamp: 0,
            ssrc: 0,
            payload: &[],
        });
        assert_eq!(
            (p.pt, p.seq, p.ssrc, p.payload),
            (111, 5, 10_000, &[1u8, 2, 3, 4][..])
        );
    }

    #[test]
    fn rejects_bad_packets() {
        assert_eq!(
            RtpPacket::parse(&[0x80; 5]).err(),
            Some(RtpError::Truncated)
        );
        assert_eq!(
            RtpPacket::parse(&[0x40; 20]).err(),
            Some(RtpError::Version(1))
        );
        // CSRC count 15 on a 12 byte packet
        let mut d = pkt(0, 0, &[]);
        d[0] = 0x8f;
        assert_eq!(RtpPacket::parse(&d).err(), Some(RtpError::Truncated));
        // padding longer than the packet
        let mut d = pkt(0, 0, &[200]);
        d[0] = 0xa0;
        assert_eq!(RtpPacket::parse(&d).err(), Some(RtpError::Truncated));
    }

    #[test]
    fn pcm_big_endian() {
        let mut dec =
            AudioDecoder::new(Encoding::S16Be, 1).unwrap_or_else(|_| unreachable_decoder());
        let out = dec.decode(&[0x40, 0x00, 0xc0, 0x00]).unwrap_or_default();
        assert_eq!(out, vec![0.5, -0.5]);
        assert!(dec.decode(&[1]).is_err());
    }

    #[test]
    fn loss_accounting() {
        let mut t = SeqTracker::default();
        for s in [65534u16, 65535, 0, 1, 4, 3, 5] {
            t.push(s);
        }
        assert_eq!((t.received, t.lost, t.out_of_order), (7, 2, 1));
    }

    fn unreachable_decoder() -> AudioDecoder {
        AudioDecoder {
            encoding: Encoding::S16Be,
            channels: 1,
            #[cfg(feature = "opus")]
            opus: None,
        }
    }
}
