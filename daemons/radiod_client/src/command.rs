// SPDX-License-Identifier: AGPL-3.0-or-later
//! The closed command schema.
//!
//! Every datagram this crate sends to `radiod` is produced by [`Command::encode`] from typed,
//! validated fields. There is no entry point that takes caller-supplied TLV bytes, and no field
//! for `OUTPUT_DATA_DEST_SOCKET` (which would let a client send a channel's audio to any host) or
//! for the front end (`gain`, `tuner` and `lock` commands). A channel always gets a lifetime.

use crate::tlv::{ty, Writer};
use std::fmt;

/// Channel identifier (RTP SSRC). `0` addresses the front end and `0xffff_ffff` means "all
/// channels" in `radiod`, so neither is constructible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ssrc(u32);

impl Ssrc {
    /// Validate a gateway-chosen SSRC.
    pub fn new(v: u32) -> Result<Self, CommandError> {
        if v == 0 || v == u32::MAX {
            Err(CommandError::ReservedSsrc(v))
        } else {
            Ok(Self(v))
        }
    }
    /// The numeric value.
    pub fn get(self) -> u32 {
        self.0
    }
}

/// Audio demodulation modes the gateway offers. Each maps to explicit parameters, not to a
/// server-side preset name (the `radiod` documentation recommends explicit parameters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Upper sideband, 50 to 3000 Hz.
    Usb,
    /// Lower sideband, -3000 to -50 Hz.
    Lsb,
    /// Narrow CW, 400 to 800 Hz offset (BFO 600 Hz).
    Cw,
    /// AM with envelope detection, +-5 kHz.
    Am,
    /// Narrow FM, +-8 kHz, 24 kHz output.
    Fm,
}

/// Output encoding of the channel's RTP stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// 16-bit signed big-endian PCM, mono.
    Pcm16Be,
    /// Opus at the given bit rate (bits per second). `radiod` always sends Opus at 48 kHz stereo.
    Opus {
        /// Target bit rate, 6000..=128000.
        bitrate: u32,
    },
}

/// A narrow audio channel.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioChannel {
    /// Channel id.
    pub ssrc: Ssrc,
    /// Mode.
    pub mode: Mode,
    /// Carrier (dial) frequency, Hz.
    pub frequency_hz: f64,
    /// Output encoding.
    pub encoding: Encoding,
    /// Frames (20 ms each at the default block time) before an unrefreshed channel is removed.
    pub lifetime_frames: u32,
}

/// A spectrum (panadapter) channel.
#[derive(Debug, Clone, PartialEq)]
pub struct SpectrumChannel {
    /// Channel id.
    pub ssrc: Ssrc,
    /// Center frequency, Hz.
    pub center_hz: f64,
    /// Number of output bins.
    pub bin_count: u32,
    /// Bin width (resolution bandwidth), Hz.
    pub bin_hz: f32,
    /// FFTs averaged into each response (1 = none).
    pub averaging: u32,
    /// Frames before an unrefreshed channel is removed.
    pub lifetime_frames: u32,
    /// Ask for one byte per bin (demodulator type 4) instead of a float per bin: a quarter of the
    /// datagram size and room for 65,000 bins instead of 16,000.
    pub byte_format: bool,
}

/// Everything the gateway may ask of `radiod`.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Create an audio channel or retune an existing one.
    Audio(AudioChannel),
    /// Create or reconfigure a spectrum channel.
    Spectrum(SpectrumChannel),
    /// Ask for the channel's status and set its remaining lifetime. A bare poll is unsafe:
    /// `radiod` creates a channel for any SSRC it has not seen (or that just expired), with
    /// the template defaults and lifetime 0, which never expires. Carrying a lifetime means a poll
    /// that races an expiry creates a channel that also expires.
    Poll {
        /// Channel id.
        ssrc: Ssrc,
        /// Frames before an unrefreshed channel is removed.
        lifetime_frames: u32,
    },
    /// Remove a channel now (sets a one-frame lifetime).
    Remove(Ssrc),
}

/// Site limits the gateway enforces before anything reaches the wire.
#[derive(Debug, Clone, PartialEq)]
pub struct Policy {
    /// Lowest allowed frequency, Hz.
    pub min_hz: f64,
    /// Highest allowed frequency, Hz.
    pub max_hz: f64,
    /// Largest spectrum bin count (one status datagram carries 4 bytes per bin, 65536 total).
    pub max_bins: u32,
    /// Smallest bin width, Hz.
    pub min_bin_hz: f32,
    /// Largest lifetime, frames.
    pub max_lifetime_frames: u32,
    /// Largest averaging count.
    pub max_averaging: u32,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            min_hz: 10_000.0,
            max_hz: 30_000_000.0,
            max_bins: 8_192,
            min_bin_hz: 1.0,
            max_lifetime_frames: 15_000, // 5 minutes at 20 ms
            max_averaging: 64,
        }
    }
}

/// A rejected command; nothing was sent.
#[derive(Debug, Clone, PartialEq)]
pub enum CommandError {
    /// SSRC 0 or 0xffffffff.
    ReservedSsrc(u32),
    /// Frequency outside the policy range or not finite.
    Frequency(f64),
    /// Bin count zero or above the policy limit.
    BinCount(u32),
    /// Bin width not finite or below the policy minimum.
    BinWidth(f32),
    /// Lifetime zero (never expires) or above the policy limit.
    Lifetime(u32),
    /// Averaging zero or above the policy limit.
    Averaging(u32),
    /// Opus bit rate out of range.
    Bitrate(u32),
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for CommandError {}

fn check_freq(p: &Policy, f: f64) -> Result<(), CommandError> {
    if f.is_finite() && f >= p.min_hz && f <= p.max_hz {
        Ok(())
    } else {
        Err(CommandError::Frequency(f))
    }
}

fn check_life(p: &Policy, l: u32) -> Result<(), CommandError> {
    // radiod treats lifetime 0 as "never expires" (DEFAULT_LIFETIME is 0), so 0 is refused.
    if l >= 1 && l <= p.max_lifetime_frames {
        Ok(())
    } else {
        Err(CommandError::Lifetime(l))
    }
}

impl Command {
    /// The channel this command addresses.
    pub fn ssrc(&self) -> Ssrc {
        match self {
            Self::Audio(a) => a.ssrc,
            Self::Spectrum(s) => s.ssrc,
            Self::Poll { ssrc, .. } => *ssrc,
            Self::Remove(s) => *s,
        }
    }

    /// Check the command against the site policy.
    pub fn validate(&self, p: &Policy) -> Result<(), CommandError> {
        match self {
            Self::Audio(a) => {
                check_freq(p, a.frequency_hz)?;
                check_life(p, a.lifetime_frames)?;
                if let Encoding::Opus { bitrate } = a.encoding {
                    if !(6_000..=128_000).contains(&bitrate) {
                        return Err(CommandError::Bitrate(bitrate));
                    }
                }
                Ok(())
            }
            Self::Spectrum(s) => {
                check_freq(p, s.center_hz)?;
                check_life(p, s.lifetime_frames)?;
                if s.bin_count == 0 || s.bin_count > p.max_bins {
                    return Err(CommandError::BinCount(s.bin_count));
                }
                if !s.bin_hz.is_finite() || s.bin_hz < p.min_bin_hz {
                    return Err(CommandError::BinWidth(s.bin_hz));
                }
                if s.averaging == 0 || s.averaging > p.max_averaging {
                    return Err(CommandError::Averaging(s.averaging));
                }
                Ok(())
            }
            Self::Poll {
                lifetime_frames, ..
            } => check_life(p, *lifetime_frames),
            Self::Remove(_) => Ok(()),
        }
    }

    /// Validate and build the datagram. `tag` is echoed by `radiod` in the status it sends in
    /// reply, which is how an acknowledgement is recognised (`radiod` has no other).
    pub fn encode(&self, p: &Policy, tag: u32) -> Result<Vec<u8>, CommandError> {
        self.validate(p)?;
        let mut w = Writer::command();
        w.uint(ty::OUTPUT_SSRC, u64::from(self.ssrc().get()));
        w.uint(ty::COMMAND_TAG, u64::from(tag));
        match self {
            Self::Poll {
                lifetime_frames, ..
            } => w.uint(ty::LIFETIME, u64::from(*lifetime_frames)),
            Self::Remove(_) => w.uint(ty::LIFETIME, 1),
            Self::Audio(a) => {
                w.uint(ty::LIFETIME, u64::from(a.lifetime_frames));
                w.f64(ty::RADIO_FREQUENCY, a.frequency_hz);
                w.uint(ty::OUTPUT_CHANNELS, 1);
                match a.mode {
                    Mode::Usb | Mode::Lsb | Mode::Cw | Mode::Am => {
                        let (lo, hi) = match a.mode {
                            Mode::Usb => (50.0, 3000.0),
                            Mode::Lsb => (-3000.0, -50.0),
                            Mode::Cw => (400.0, 800.0),
                            _ => (-5000.0, 5000.0),
                        };
                        w.uint(ty::DEMOD_TYPE, 0);
                        w.uint(ty::OUTPUT_SAMPRATE, 12_000);
                        w.f32(ty::LOW_EDGE, lo);
                        w.f32(ty::HIGH_EDGE, hi);
                        w.bool(ty::ENVELOPE, a.mode == Mode::Am);
                        w.bool(ty::AGC_ENABLE, true);
                    }
                    Mode::Fm => {
                        w.uint(ty::DEMOD_TYPE, 1);
                        w.uint(ty::OUTPUT_SAMPRATE, 24_000);
                        w.f32(ty::LOW_EDGE, -8000.0);
                        w.f32(ty::HIGH_EDGE, 8000.0);
                        w.bool(ty::THRESH_EXTEND, true);
                        w.f32(ty::DEEMPH_TC, 530.5);
                        w.f32(ty::DEEMPH_GAIN, 12.0);
                    }
                }
                // OUTPUT_ENCODING values from rtp.h: S16BE = 2, OPUS = 3.
                match a.encoding {
                    Encoding::Pcm16Be => w.uint(ty::OUTPUT_ENCODING, 2),
                    Encoding::Opus { bitrate } => {
                        w.uint(ty::OUTPUT_ENCODING, 3);
                        w.uint(ty::OPUS_BIT_RATE, u64::from(bitrate));
                    }
                }
            }
            Self::Spectrum(s) => {
                w.uint(ty::LIFETIME, u64::from(s.lifetime_frames));
                w.uint(ty::DEMOD_TYPE, if s.byte_format { 4 } else { 3 }); // 3 float BIN_DATA, 4 BIN_BYTE_DATA
                w.f64(ty::RADIO_FREQUENCY, s.center_hz);
                w.uint(ty::BIN_COUNT, u64::from(s.bin_count));
                w.f32(ty::RESOLUTION_BW, s.bin_hz);
                w.uint(ty::SPECTRUM_AVG, u64::from(s.averaging));
            }
        }
        Ok(w.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tlv::{parse, Kind};

    fn audio(freq: f64, life: u32) -> Command {
        let ssrc = Ssrc::new(100).unwrap_or(Ssrc(100));
        Command::Audio(AudioChannel {
            ssrc,
            mode: Mode::Usb,
            frequency_hz: freq,
            encoding: Encoding::Pcm16Be,
            lifetime_frames: life,
        })
    }

    #[test]
    fn rejects_unsafe_values() {
        let p = Policy::default();
        assert!(audio(f64::NAN, 100).encode(&p, 1).is_err());
        assert!(audio(1.0e9, 100).encode(&p, 1).is_err());
        assert!(
            audio(7.0e6, 0).encode(&p, 1).is_err(),
            "lifetime 0 means immortal in radiod"
        );
        assert!(audio(7.0e6, 1_000_000).encode(&p, 1).is_err());
        assert!(Ssrc::new(0).is_err());
        assert!(Ssrc::new(u32::MAX).is_err());
    }

    #[test]
    fn encoded_command_never_names_a_destination() {
        let p = Policy::default();
        let pkt = audio(7.074e6, 500).encode(&p, 7).unwrap_or_default();
        let (k, e) = parse(&pkt).unwrap_or_else(|_| (Kind::Status, vec![]));
        assert_eq!(k, Kind::Command);
        for forbidden in [
            ty::OUTPUT_DATA_DEST_SOCKET,
            ty::STATUS_DEST_SOCKET,
            ty::OUTPUT_TTL,
            ty::OUTPUT_DATA_SOURCE_SOCKET,
        ] {
            assert!(e.iter().all(|x| x.ty != forbidden));
        }
        assert!(e
            .iter()
            .any(|x| x.ty == ty::LIFETIME && x.uint() == Ok(500)));
    }

    #[test]
    fn spectrum_limits() {
        let p = Policy::default();
        let ssrc = Ssrc::new(5).unwrap_or(Ssrc(5));
        let mut s = SpectrumChannel {
            ssrc,
            center_hz: 7.1e6,
            bin_count: 1024,
            bin_hz: 100.0,
            averaging: 1,
            lifetime_frames: 100,
            byte_format: false,
        };
        assert!(Command::Spectrum(s.clone()).encode(&p, 1).is_ok());
        s.bin_count = 20_000;
        assert!(Command::Spectrum(s.clone()).encode(&p, 1).is_err());
        s.bin_count = 100;
        s.bin_hz = 0.0;
        assert!(Command::Spectrum(s).encode(&p, 1).is_err());
    }
}
