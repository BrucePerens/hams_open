// SPDX-License-Identifier: AGPL-3.0-or-later
//! The closed command schema.
//!
//! Every datagram this crate sends to `radiod` is produced by [`Command::encode`] from typed,
//! validated fields. There is no entry point that takes caller-supplied TLV bytes, and no field
//! for `OUTPUT_DATA_DEST_SOCKET` (which would let a client send a channel's audio to any host) or
//! for the front end (`gain`, `tuner` and `lock` commands). A channel always gets a lifetime.

use crate::tlv::{ty, Writer};
use std::fmt;

/// Hard ceiling on a float-format (`BIN_DATA`) spectrum request, whatever the [`Policy`] says.
/// `radiod` builds a status datagram in a 65,536-byte stack buffer without checking the room:
/// 16,000 float bins worked in the trial and 17,000 crashed it with a segmentation fault.
pub const MAX_FLOAT_BINS: u32 = 16_000;
/// Hard ceiling on a byte-format (`BIN_BYTE_DATA`) spectrum request, whatever the [`Policy`]
/// says. 65,000 worked in the trial and 66,000 crashed `radiod`; this keeps a margin.
pub const MAX_BYTE_BINS: u32 = 60_000;

/// Channel identifier (RTP SSRC). `0` addresses the front end and `0xffff_ffff` means "all
/// channels" in `radiod`, so neither is constructible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ssrc(u32);

impl Ssrc {
    // [@ANCHOR: radiod_client_command_ssrc_new]
    /// Validate a gateway-chosen SSRC.
    pub fn new(v: u32) -> Result<Self, CommandError> {
        if v == 0 || v == u32::MAX {
            Err(CommandError::ReservedSsrc(v))
        } else {
            Ok(Self(v))
        }
    }
    // [@ANCHOR: radiod_client_command_ssrc_get]
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
    /// Largest spectrum bin count the site allows. It can only lower the hard ceilings
    /// [`MAX_FLOAT_BINS`] and [`MAX_BYTE_BINS`]; a larger value here changes nothing.
    pub max_bins: u32,
    /// Smallest bin width, Hz.
    pub min_bin_hz: f32,
    /// Largest lifetime, frames.
    pub max_lifetime_frames: u32,
    /// Largest averaging count.
    pub max_averaging: u32,
}

impl Default for Policy {
    // [@ANCHOR: radiod_client_command_policy_default]
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
    // [@ANCHOR: radiod_client_command_error_fmt]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for CommandError {}

// [@ANCHOR: radiod_client_command_check_freq]
fn check_freq(p: &Policy, f: f64) -> Result<(), CommandError> {
    if f.is_finite() && f >= p.min_hz && f <= p.max_hz {
        Ok(())
    } else {
        Err(CommandError::Frequency(f))
    }
}

// [@ANCHOR: radiod_client_command_check_life]
fn check_life(p: &Policy, l: u32) -> Result<(), CommandError> {
    // radiod treats lifetime 0 as "never expires" (DEFAULT_LIFETIME is 0), so 0 is refused.
    if l >= 1 && l <= p.max_lifetime_frames {
        Ok(())
    } else {
        Err(CommandError::Lifetime(l))
    }
}

impl Policy {
    // [@ANCHOR: radiod_client_command_policy_bin_limit]
    /// The largest bin count this policy permits for a format: the site limit, never above the
    /// hard ceiling that keeps `radiod` from crashing.
    pub fn bin_limit(&self, byte_format: bool) -> u32 {
        let hard = if byte_format {
            MAX_BYTE_BINS
        } else {
            MAX_FLOAT_BINS
        };
        self.max_bins.min(hard)
    }
}

impl Command {
    // [@ANCHOR: radiod_client_command_ssrc]
    /// The channel this command addresses.
    pub fn ssrc(&self) -> Ssrc {
        match self {
            Self::Audio(a) => a.ssrc,
            Self::Spectrum(s) => s.ssrc,
            Self::Poll { ssrc, .. } => *ssrc,
            Self::Remove(s) => *s,
        }
    }

    // [@ANCHOR: radiod_client_command_validate]
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
                if s.bin_count == 0 || s.bin_count > p.bin_limit(s.byte_format) {
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

    // [@ANCHOR: radiod_client_command_encode]
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
    use crate::tlv::{parse, Entry, Kind};

    fn ssrc(n: u32) -> Ssrc {
        Ssrc::new(n).unwrap_or(Ssrc(100))
    }

    fn audio(freq: f64, life: u32) -> Command {
        Command::Audio(AudioChannel {
            ssrc: ssrc(100),
            mode: Mode::Usb,
            frequency_hz: freq,
            encoding: Encoding::Pcm16Be,
            lifetime_frames: life,
        })
    }

    fn spectrum(bins: u32, byte_format: bool) -> SpectrumChannel {
        SpectrumChannel {
            ssrc: ssrc(5),
            center_hz: 7.1e6,
            bin_count: bins,
            bin_hz: 100.0,
            averaging: 1,
            lifetime_frames: 100,
            byte_format,
        }
    }

    fn entries(pkt: &[u8]) -> Vec<Entry<'_>> {
        let (kind, e) = parse(pkt).unwrap_or_else(|_| (Kind::Status, vec![]));
        assert_eq!(kind, Kind::Command);
        e
    }

    // Tests [@ANCHOR: radiod_client_command_ssrc_new]
    // Tests [@ANCHOR: radiod_client_command_ssrc_get]
    #[test]
    fn reserved_ssrcs_are_not_constructible() {
        assert_eq!(Ssrc::new(0), Err(CommandError::ReservedSsrc(0)));
        assert_eq!(
            Ssrc::new(u32::MAX),
            Err(CommandError::ReservedSsrc(u32::MAX))
        );
        assert_eq!(Ssrc::new(1).map(Ssrc::get), Ok(1));
        assert_eq!(Ssrc::new(u32::MAX - 1).map(Ssrc::get), Ok(u32::MAX - 1));
    }

    // Tests [@ANCHOR: radiod_client_command_policy_default]
    #[test]
    fn default_policy_is_inside_the_hard_ceilings() {
        let p = Policy::default();
        assert!(p.max_bins <= MAX_FLOAT_BINS);
        assert!(p.min_hz > 0.0 && p.max_hz <= 30_000_000.0);
        assert!(p.max_lifetime_frames >= 1);
    }

    // Tests [@ANCHOR: radiod_client_command_check_freq]
    // Tests [@ANCHOR: radiod_client_command_check_life]
    // Tests [@ANCHOR: radiod_client_command_validate]
    #[test]
    fn rejects_unsafe_values() {
        let p = Policy::default();
        assert!(audio(f64::NAN, 100).validate(&p).is_err());
        assert!(audio(f64::INFINITY, 100).validate(&p).is_err());
        assert!(audio(1.0e9, 100).validate(&p).is_err());
        assert!(audio(1.0, 100).validate(&p).is_err());
        assert_eq!(
            audio(7.0e6, 0).validate(&p),
            Err(CommandError::Lifetime(0)),
            "lifetime 0 means immortal in radiod"
        );
        assert!(audio(7.0e6, 1_000_000).validate(&p).is_err());
        assert!(audio(7.0e6, 1).validate(&p).is_ok());
        let opus = |bitrate| {
            Command::Audio(AudioChannel {
                encoding: Encoding::Opus { bitrate },
                ..base_audio()
            })
        };
        assert_eq!(opus(5_999).validate(&p), Err(CommandError::Bitrate(5_999)));
        assert!(opus(24_000).validate(&p).is_ok());
        assert!(opus(128_001).validate(&p).is_err());
    }

    fn base_audio() -> AudioChannel {
        AudioChannel {
            ssrc: ssrc(100),
            mode: Mode::Usb,
            frequency_hz: 7.0e6,
            encoding: Encoding::Pcm16Be,
            lifetime_frames: 10,
        }
    }

    // Tests [@ANCHOR: radiod_client_command_ssrc]
    #[test]
    fn every_command_names_its_channel() {
        assert_eq!(audio(7.0e6, 10).ssrc(), ssrc(100));
        assert_eq!(Command::Spectrum(spectrum(10, false)).ssrc(), ssrc(5));
        let poll = Command::Poll {
            ssrc: ssrc(9),
            lifetime_frames: 5,
        };
        assert_eq!(poll.ssrc(), ssrc(9));
        assert_eq!(Command::Remove(ssrc(8)).ssrc(), ssrc(8));
    }

    // Tests [@ANCHOR: radiod_client_command_encode]
    // Tests [@ANCHOR: radiod_client_command_validate]
    #[test]
    fn encoded_command_never_names_a_destination() {
        let p = Policy::default();
        let mut cmds = vec![
            audio(7.074e6, 500),
            Command::Spectrum(spectrum(1024, false)),
            Command::Spectrum(spectrum(1024, true)),
            Command::Poll {
                ssrc: ssrc(3),
                lifetime_frames: 50,
            },
            Command::Remove(ssrc(3)),
        ];
        for mode in [Mode::Lsb, Mode::Cw, Mode::Am, Mode::Fm] {
            cmds.push(Command::Audio(AudioChannel {
                mode,
                encoding: Encoding::Opus { bitrate: 24_000 },
                ..base_audio()
            }));
        }
        for cmd in cmds {
            let pkt = cmd.encode(&p, 7).unwrap_or_default();
            assert!(!pkt.is_empty());
            let e = entries(&pkt);
            // Closed schema: no destination, status group, TTL, source, front end or tuner
            // field can ever appear.
            for forbidden in [
                ty::OUTPUT_DATA_DEST_SOCKET,
                ty::STATUS_DEST_SOCKET,
                ty::OUTPUT_TTL,
                ty::OUTPUT_DATA_SOURCE_SOCKET,
                ty::FIRST_LO_FREQUENCY,
                ty::GAIN,
                ty::RF_LEVEL_CAL,
            ] {
                assert!(e.iter().all(|x| x.ty != forbidden), "{cmd:?}");
            }
            assert!(e
                .iter()
                .any(|x| x.ty == ty::COMMAND_TAG && x.uint() == Ok(7)));
        }
    }

    // Tests [@ANCHOR: radiod_client_command_encode]
    #[test]
    fn audio_command_carries_frequency_and_lifetime() {
        let pkt = audio(7.074e6, 500)
            .encode(&Policy::default(), 7)
            .unwrap_or_default();
        let e = entries(&pkt);
        assert!(e
            .iter()
            .any(|x| x.ty == ty::LIFETIME && x.uint() == Ok(500)));
        assert!(e
            .iter()
            .any(|x| x.ty == ty::RADIO_FREQUENCY && x.f64() == Ok(7.074e6)));
        assert!(e
            .iter()
            .any(|x| x.ty == ty::OUTPUT_SSRC && x.uint() == Ok(100)));
    }

    // Tests [@ANCHOR: radiod_client_command_encode]
    #[test]
    fn a_bare_unknown_ssrc_poll_cannot_be_built() {
        let p = Policy::default();
        let bare = Command::Poll {
            ssrc: ssrc(777),
            lifetime_frames: 0,
        };
        assert_eq!(bare.encode(&p, 1), Err(CommandError::Lifetime(0)));
        // Every poll and removal that does encode carries a LIFETIME of at least one frame.
        for life in [1u32, 2, 100, p.max_lifetime_frames] {
            let poll = Command::Poll {
                ssrc: ssrc(777),
                lifetime_frames: life,
            };
            let pkt = poll.encode(&p, 1).unwrap_or_default();
            let found = entries(&pkt)
                .iter()
                .find(|x| x.ty == ty::LIFETIME)
                .and_then(|x| x.uint().ok());
            assert_eq!(found, Some(u64::from(life)));
        }
        let pkt = Command::Remove(ssrc(777)).encode(&p, 1).unwrap_or_default();
        let found = entries(&pkt)
            .iter()
            .find(|x| x.ty == ty::LIFETIME)
            .and_then(|x| x.uint().ok());
        assert_eq!(found, Some(1));
    }

    // Tests [@ANCHOR: radiod_client_command_policy_bin_limit]
    // Tests [@ANCHOR: radiod_client_command_validate]
    #[test]
    fn oversized_bin_requests_are_refused_even_by_a_permissive_policy() {
        let permissive = Policy {
            max_bins: u32::MAX,
            ..Policy::default()
        };
        assert_eq!(permissive.bin_limit(false), 16_000);
        assert_eq!(permissive.bin_limit(true), 60_000);
        for (bins, byte_format, ok) in [
            (16_000, false, true),
            (16_001, false, false),
            (17_000, false, false), // crashed radiod in the trial
            (60_000, true, true),
            (60_001, true, false),
            (66_000, true, false), // crashed radiod in the trial
            (65_000, true, false),
            (u32::MAX, true, false),
            (u32::MAX, false, false),
            (0, false, false),
            (0, true, false),
        ] {
            let cmd = Command::Spectrum(spectrum(bins, byte_format));
            assert_eq!(
                cmd.encode(&permissive, 1).is_ok(),
                ok,
                "{bins} bins byte_format={byte_format}"
            );
            if !ok {
                assert_eq!(
                    cmd.encode(&permissive, 1),
                    Err(CommandError::BinCount(bins))
                );
            }
        }
        // A site policy can lower the ceiling, never raise it.
        let strict = Policy {
            max_bins: 1_000,
            ..Policy::default()
        };
        assert_eq!(strict.bin_limit(true), 1_000);
        assert!(Command::Spectrum(spectrum(1_001, true))
            .encode(&strict, 1)
            .is_err());
        assert!(Command::Spectrum(spectrum(1_000, true))
            .encode(&strict, 1)
            .is_ok());
    }

    // Tests [@ANCHOR: radiod_client_command_validate]
    // Tests [@ANCHOR: radiod_client_command_encode]
    #[test]
    fn spectrum_limits() {
        let p = Policy::default();
        let mut s = spectrum(1024, false);
        assert!(Command::Spectrum(s.clone()).encode(&p, 1).is_ok());
        s.bin_hz = 0.0;
        assert!(Command::Spectrum(s.clone()).encode(&p, 1).is_err());
        s.bin_hz = f32::NAN;
        assert!(Command::Spectrum(s.clone()).encode(&p, 1).is_err());
        s.bin_hz = 100.0;
        s.averaging = 0;
        assert!(Command::Spectrum(s.clone()).encode(&p, 1).is_err());
        s.averaging = p.max_averaging + 1;
        assert!(Command::Spectrum(s.clone()).encode(&p, 1).is_err());
        s.averaging = 1;
        s.lifetime_frames = 0;
        assert!(Command::Spectrum(s).encode(&p, 1).is_err());
        let pkt = Command::Spectrum(spectrum(1024, true))
            .encode(&p, 1)
            .unwrap_or_default();
        let e = entries(&pkt);
        assert!(e
            .iter()
            .any(|x| x.ty == ty::DEMOD_TYPE && x.uint() == Ok(4)));
        assert!(e
            .iter()
            .any(|x| x.ty == ty::BIN_COUNT && x.uint() == Ok(1024)));
    }

    // Tests [@ANCHOR: radiod_client_command_error_fmt]
    #[test]
    fn errors_display() {
        assert_eq!(CommandError::BinCount(7).to_string(), "BinCount(7)");
    }

    // Tests [@ANCHOR: radiod_client_command_encode]
    #[test]
    fn no_public_path_takes_caller_bytes() {
        // The only constructors of wire bytes are Command::encode and the crate-private
        // Writer. This pins that the crate's public command surface is the closed schema: a
        // Command is built from typed fields and the output is always one command packet.
        let pkt = audio(7.0e6, 10)
            .encode(&Policy::default(), 1)
            .unwrap_or_default();
        assert_eq!(pkt.first(), Some(&1));
        assert_eq!(pkt.last(), Some(&ty::EOL));
    }
}
