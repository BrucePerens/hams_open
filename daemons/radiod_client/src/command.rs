// SPDX-License-Identifier: AGPL-3.0-or-later
//! The closed command schema.
//!
//! Every datagram this crate sends to `radiod` is produced by [`Command::encode`] from typed,
//! validated fields. There is no entry point that takes caller-supplied TLV bytes, and no field
//! for `OUTPUT_DATA_DEST_SOCKET` (which would let a client send a channel's audio to any host) or
//! for the front end (RF `gain`, attenuation, `tuner` and `lock` commands). A channel always gets a
//! lifetime. The one exception to "every command names a channel" is [`encode_front_end_poll`], a
//! read of the front end's status (SSRC 0) that carries nothing but a tag.

use crate::tlv::{ty, Writer};
use std::fmt;

/// Hard ceiling on a float-format (`BIN_DATA`) spectrum request, whatever the [`Policy`] says.
/// `radiod` builds a status datagram in a 65,536-byte stack buffer without checking the room:
/// 16,000 float bins worked in the trial and 17,000 crashed it with a segmentation fault.
pub const MAX_FLOAT_BINS: u32 = 16_000;
/// Hard ceiling on a byte-format (`BIN_BYTE_DATA`) spectrum request, whatever the [`Policy`]
/// says. 65,000 worked in the trial and 66,000 crashed `radiod`; this keeps a margin.
pub const MAX_BYTE_BINS: u32 = 60_000;
// [@ANCHOR: radiod_client_command_limits]
/// Largest filter edge magnitude, Hz: half the widest output rate the schema asks for (24 kHz FM).
/// An edge beyond the output Nyquist frequency only narrows the passband (`radiod` cannot pass
/// what its output rate cannot carry), so this bound loses nothing.
pub const MAX_FILTER_EDGE_HZ: f32 = 12_000.0;
/// Channel (audio) gain range for a fixed-gain channel, dB. The RX-888 bring-up (2026-11-05) saw
/// about 61 dB under AGC on an AM channel and needed about 100 dB for I/Q noise to reach -35 dBFS.
pub const GAIN_RANGE_DB: (f32, f32) = (-20.0, 160.0);

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
// [@ANCHOR: radiod_client_command_mode]
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
    /// Raw I/Q: linear demodulator with no detection, +-5 kHz, 12 kHz output, two channels (I then
    /// Q in each sample pair). The parameters of `radiod`'s own `[iq]` preset, sent explicitly.
    Iq,
    /// Synchronous AM, both sidebands: linear demodulator with `radiod`'s carrier PLL on (the
    /// documented `pll = on`), envelope detection off, +-5 kHz, 12 kHz mono. The squelch the PLL
    /// brings with it is held open (`SQUELCH_OPEN` -1000 dB, as `radiod`'s documentation says).
    Sam,
    /// Synchronous AM, lower sideband and carrier: as [`Mode::Sam`], passband -5000 to +100 Hz
    /// (the carrier must stay inside it for the PLL to track; `radiod`'s `[ame]` preset mirrored).
    SamLower,
    /// Synchronous AM, upper sideband and carrier: as [`Mode::Sam`], passband -100 to +5000 Hz (the
    /// placement of `radiod`'s `[ame]` preset).
    SamUpper,
}

// [@ANCHOR: radiod_client_command_sam_squelch]
/// The squelch thresholds sent with a synchronous mode, dB SNR: `radiod`'s carrier squelch comes on
/// with its PLL, and its documentation says an opening threshold of -1000 dB keeps it open.
pub const SAM_SQUELCH_OPEN_DB: f32 = -1000.0;

impl Mode {
    // [@ANCHOR: radiod_client_command_mode_is_synchronous]
    /// True for the synchronous AM modes (`Sam`, `SamLower`, `SamUpper`): the ones that turn on
    /// `radiod`'s carrier PLL, so their status carries a carrier lock.
    pub fn is_synchronous(self) -> bool {
        matches!(self, Mode::Sam | Mode::SamLower | Mode::SamUpper)
    }
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
// [@ANCHOR: radiod_client_command_command]
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
    /// Set the passband of an existing channel, Hz relative to the carrier. A later
    /// [`Command::Audio`] puts the mode's own passband back.
    Filter {
        /// Channel id.
        ssrc: Ssrc,
        /// Low edge, Hz (negative is below the carrier).
        low_hz: f32,
        /// High edge, Hz; above `low_hz`.
        high_hz: f32,
        /// Frames before an unrefreshed channel is removed.
        lifetime_frames: u32,
    },
    /// Set the channel's own audio gain: a fixed gain in dB (AGC off) or `None` for AGC. This is the
    /// demodulator's output gain, not the front end's RF gain, which this schema cannot touch. A later
    /// [`Command::Audio`] turns AGC back on.
    Gain {
        /// Channel id.
        ssrc: Ssrc,
        /// Fixed gain, dB, inside [`GAIN_RANGE_DB`]; `None` for AGC.
        gain_db: Option<f32>,
        /// Frames before an unrefreshed channel is removed.
        lifetime_frames: u32,
    },
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
// [@ANCHOR: radiod_client_command_error]
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
    /// Filter edges not finite, beyond [`MAX_FILTER_EDGE_HZ`], or low not below high.
    Filter(f32, f32),
    /// Fixed gain not finite or outside [`GAIN_RANGE_DB`].
    Gain(f32),
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

// [@ANCHOR: radiod_client_command_check_filter]
fn check_filter(low: f32, high: f32) -> Result<(), CommandError> {
    let edge_ok = |e: f32| e.is_finite() && e.abs() <= MAX_FILTER_EDGE_HZ;
    if edge_ok(low) && edge_ok(high) && low < high {
        Ok(())
    } else {
        Err(CommandError::Filter(low, high))
    }
}

// [@ANCHOR: radiod_client_command_encode_front_end_poll]
/// The datagram that asks `radiod` for its front end's status: SSRC 0 and the tag, nothing else.
/// SSRC 0 is the front end, not a channel, so no channel is created and no lifetime applies. Observed
/// on the RX-888 (2026-11-05): the reply carries no `COMMAND_TAG`, so it is recognised by SSRC 0
/// alone ([`crate::net::is_front_end`]), and `radiod` sends it only when asked.
pub fn encode_front_end_poll(tag: u32) -> Vec<u8> {
    let mut w = Writer::command();
    w.uint(ty::OUTPUT_SSRC, 0);
    w.uint(ty::COMMAND_TAG, u64::from(tag));
    w.finish()
}

// [@ANCHOR: radiod_client_command_max_spectrum_span_hz]
/// The widest spectrum span (bin count times bin width) a front end can fill: half its sample rate
/// for a real (direct-sampling) front end such as the RX-888, the whole rate for a complex one.
/// Observed on the RX-888 at 64.8 Msps (2026-11-05): spans up to 32.41 MHz came back with every bin
/// filled, and 32.44 MHz or wider with about half the bins exactly zero; half the rate (32.4 MHz) is
/// the bound kept here. A caller that knows the front end (its status has `input_samprate` and
/// `fe_is_real`) keeps requests inside this.
pub fn max_spectrum_span_hz(input_samprate: u64, real: bool) -> f64 {
    let rate = input_samprate as f64;
    if real {
        rate / 2.0
    } else {
        rate
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
            Self::Poll { ssrc, .. } | Self::Filter { ssrc, .. } | Self::Gain { ssrc, .. } => *ssrc,
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
            Self::Filter {
                low_hz,
                high_hz,
                lifetime_frames,
                ..
            } => {
                check_life(p, *lifetime_frames)?;
                check_filter(*low_hz, *high_hz)
            }
            Self::Gain {
                gain_db,
                lifetime_frames,
                ..
            } => {
                check_life(p, *lifetime_frames)?;
                match gain_db {
                    Some(g)
                        if !(g.is_finite() && *g >= GAIN_RANGE_DB.0 && *g <= GAIN_RANGE_DB.1) =>
                    {
                        Err(CommandError::Gain(*g))
                    }
                    _ => Ok(()),
                }
            }
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
            Self::Filter {
                low_hz,
                high_hz,
                lifetime_frames,
                ..
            } => {
                w.uint(ty::LIFETIME, u64::from(*lifetime_frames));
                w.f32(ty::LOW_EDGE, *low_hz);
                w.f32(ty::HIGH_EDGE, *high_hz);
            }
            Self::Gain {
                gain_db,
                lifetime_frames,
                ..
            } => {
                w.uint(ty::LIFETIME, u64::from(*lifetime_frames));
                match gain_db {
                    Some(g) => {
                        w.bool(ty::AGC_ENABLE, false);
                        w.f32(ty::GAIN, *g);
                    }
                    None => w.bool(ty::AGC_ENABLE, true),
                }
            }
            Self::Audio(a) => {
                w.uint(ty::LIFETIME, u64::from(a.lifetime_frames));
                w.f64(ty::RADIO_FREQUENCY, a.frequency_hz);
                w.uint(ty::OUTPUT_CHANNELS, if a.mode == Mode::Iq { 2 } else { 1 });
                match a.mode {
                    Mode::Iq => {
                        w.uint(ty::DEMOD_TYPE, 0);
                        w.uint(ty::OUTPUT_SAMPRATE, 12_000);
                        w.f32(ty::LOW_EDGE, -5000.0);
                        w.f32(ty::HIGH_EDGE, 5000.0);
                        w.bool(ty::ENVELOPE, false);
                        w.bool(ty::PLL_ENABLE, false);
                        w.bool(ty::AGC_ENABLE, true);
                    }
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
                        w.bool(ty::PLL_ENABLE, false);
                        w.bool(ty::AGC_ENABLE, true);
                    }
                    Mode::Sam | Mode::SamLower | Mode::SamUpper => {
                        let (lo, hi) = match a.mode {
                            Mode::SamLower => (-5000.0, 100.0),
                            Mode::SamUpper => (-100.0, 5000.0),
                            _ => (-5000.0, 5000.0),
                        };
                        w.uint(ty::DEMOD_TYPE, 0);
                        w.uint(ty::OUTPUT_SAMPRATE, 12_000);
                        w.f32(ty::LOW_EDGE, lo);
                        w.f32(ty::HIGH_EDGE, hi);
                        w.bool(ty::ENVELOPE, false);
                        w.bool(ty::PLL_ENABLE, true);
                        w.bool(ty::PLL_SQUARE, false);
                        w.bool(ty::SNR_SQUELCH, false);
                        w.f32(ty::SQUELCH_OPEN, SAM_SQUELCH_OPEN_DB);
                        w.f32(ty::SQUELCH_CLOSE, SAM_SQUELCH_OPEN_DB);
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
            filter(-3000.0, -50.0, 10),
            gain(None, 10),
        ];
        for mode in [
            Mode::Lsb,
            Mode::Cw,
            Mode::Am,
            Mode::Fm,
            Mode::Iq,
            Mode::Sam,
            Mode::SamLower,
            Mode::SamUpper,
        ] {
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
        assert_eq!(
            CommandError::Filter(1.0, 0.0).to_string(),
            "Filter(1.0, 0.0)"
        );
        assert_eq!(CommandError::Gain(200.0).to_string(), "Gain(200.0)");
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

    fn filter(low: f32, high: f32, life: u32) -> Command {
        Command::Filter {
            ssrc: ssrc(100),
            low_hz: low,
            high_hz: high,
            lifetime_frames: life,
        }
    }

    fn gain(g: Option<f32>, life: u32) -> Command {
        Command::Gain {
            ssrc: ssrc(100),
            gain_db: g,
            lifetime_frames: life,
        }
    }

    // Tests [@ANCHOR: radiod_client_command_check_filter]
    // Tests [@ANCHOR: radiod_client_command_command]
    // Tests [@ANCHOR: radiod_client_command_error]
    // Tests [@ANCHOR: radiod_client_command_limits]
    // Tests [@ANCHOR: radiod_client_command_validate]
    // Tests [@ANCHOR: radiod_client_command_encode]
    // Tests [@ANCHOR: radiod_client_command_ssrc]
    #[test]
    fn filter_command_is_bounded_and_carries_a_lifetime() {
        let p = Policy::default();
        assert_eq!(filter(300.0, 800.0, 10).ssrc(), ssrc(100));
        for (lo, hi) in [
            (f32::NAN, 100.0),
            (100.0, f32::INFINITY),
            (500.0, 500.0),
            (800.0, 300.0),
            (-12_001.0, 0.0),
            (0.0, 12_001.0),
        ] {
            assert!(
                matches!(
                    filter(lo, hi, 10).validate(&p),
                    Err(CommandError::Filter(..))
                ),
                "{lo} {hi}"
            );
        }
        assert_eq!(
            filter(300.0, 800.0, 0).validate(&p),
            Err(CommandError::Lifetime(0))
        );
        assert!(filter(-12_000.0, 12_000.0, 10).validate(&p).is_ok());
        let pkt = filter(-2700.0, -300.0, 75)
            .encode(&p, 3)
            .unwrap_or_default();
        let e = entries(&pkt);
        assert!(e
            .iter()
            .any(|x| x.ty == ty::LOW_EDGE && x.f32() == Ok(-2700.0)));
        assert!(e
            .iter()
            .any(|x| x.ty == ty::HIGH_EDGE && x.f32() == Ok(-300.0)));
        assert!(e.iter().any(|x| x.ty == ty::LIFETIME && x.uint() == Ok(75)));
        assert!(e
            .iter()
            .all(|x| x.ty != ty::RADIO_FREQUENCY && x.ty != ty::OUTPUT_DATA_DEST_SOCKET));
    }

    // Tests [@ANCHOR: radiod_client_command_validate]
    // Tests [@ANCHOR: radiod_client_command_encode]
    #[test]
    fn gain_command_is_the_channel_gain_or_agc() {
        let p = Policy::default();
        let fixed = gain(Some(95.0), 50).encode(&p, 4).unwrap_or_default();
        let e = entries(&fixed);
        assert!(e
            .iter()
            .any(|x| x.ty == ty::AGC_ENABLE && x.bool() == Ok(false)));
        assert!(e.iter().any(|x| x.ty == ty::GAIN && x.f32() == Ok(95.0)));
        assert!(e.iter().any(|x| x.ty == ty::LIFETIME && x.uint() == Ok(50)));
        let agc = gain(None, 50).encode(&p, 4).unwrap_or_default();
        let e = entries(&agc);
        assert!(e
            .iter()
            .any(|x| x.ty == ty::AGC_ENABLE && x.bool() == Ok(true)));
        assert!(e.iter().all(|x| x.ty != ty::GAIN));
        // Never the front end's RF gain or attenuation.
        for pkt in [&fixed, &agc] {
            assert!(entries(pkt)
                .iter()
                .all(|x| x.ty != ty::RF_GAIN && x.ty != ty::RF_ATTEN && x.ty != ty::RF_AGC));
        }
        for bad in [f32::NAN, f32::INFINITY, -20.5, 160.5] {
            assert!(
                matches!(gain(Some(bad), 50).validate(&p), Err(CommandError::Gain(_))),
                "{bad}"
            );
        }
        assert_eq!(gain(None, 0).validate(&p), Err(CommandError::Lifetime(0)));
        assert!(
            gain(Some(-20.0), 1).validate(&p).is_ok() && gain(Some(160.0), 1).validate(&p).is_ok()
        );
    }

    // Tests [@ANCHOR: radiod_client_command_encode]
    // Tests [@ANCHOR: radiod_client_command_mode]
    #[test]
    fn iq_mode_is_two_channel_linear_without_detection() {
        let pkt = Command::Audio(AudioChannel {
            mode: Mode::Iq,
            ..base_audio()
        })
        .encode(&Policy::default(), 2)
        .unwrap_or_default();
        let e = entries(&pkt);
        let has = |t: u8, v: u64| e.iter().any(|x| x.ty == t && x.uint() == Ok(v));
        assert!(has(ty::OUTPUT_CHANNELS, 2));
        assert!(has(ty::DEMOD_TYPE, 0));
        assert!(has(ty::OUTPUT_SAMPRATE, 12_000));
        assert!(e
            .iter()
            .any(|x| x.ty == ty::ENVELOPE && x.bool() == Ok(false)));
        assert!(e
            .iter()
            .any(|x| x.ty == ty::LOW_EDGE && x.f32() == Ok(-5000.0)));
        // Every other mode stays mono.
        for mode in [
            Mode::Usb,
            Mode::Lsb,
            Mode::Cw,
            Mode::Am,
            Mode::Fm,
            Mode::Sam,
            Mode::SamLower,
            Mode::SamUpper,
        ] {
            let pkt = Command::Audio(AudioChannel {
                mode,
                ..base_audio()
            })
            .encode(&Policy::default(), 2)
            .unwrap_or_default();
            assert!(entries(&pkt)
                .iter()
                .any(|x| x.ty == ty::OUTPUT_CHANNELS && x.uint() == Ok(1)));
        }
    }

    // Tests [@ANCHOR: radiod_client_command_encode_front_end_poll]
    #[test]
    fn front_end_poll_names_only_ssrc_zero_and_the_tag() {
        let pkt = encode_front_end_poll(77);
        let e = entries(&pkt);
        assert_eq!(e.len(), 2, "{e:?}");
        assert!(e
            .iter()
            .any(|x| x.ty == ty::OUTPUT_SSRC && x.uint() == Ok(0)));
        assert!(e
            .iter()
            .any(|x| x.ty == ty::COMMAND_TAG && x.uint() == Ok(77)));
        assert_eq!(pkt.last(), Some(&ty::EOL));
    }

    // Tests [@ANCHOR: radiod_client_command_max_spectrum_span_hz]
    #[test]
    fn spectrum_span_is_half_the_rate_for_a_real_front_end() {
        assert_eq!(max_spectrum_span_hz(64_800_000, true), 32_400_000.0);
        assert_eq!(max_spectrum_span_hz(2_000_000, false), 2_000_000.0);
        assert_eq!(max_spectrum_span_hz(0, true), 0.0);
        // The RX-888 observations: 8192 x 3950 Hz fits, 8192 x 3960 Hz does not.
        assert!(8192.0 * 3950.0 <= max_spectrum_span_hz(64_800_000, true));
        assert!(8192.0 * 3960.0 > max_spectrum_span_hz(64_800_000, true));
    }

    // Tests [@ANCHOR: radiod_client_command_mode]
    // Tests [@ANCHOR: radiod_client_command_sam_squelch]
    // Tests [@ANCHOR: radiod_client_command_encode]
    #[test]
    fn synchronous_modes_turn_the_pll_on_and_hold_its_squelch_open() {
        for (mode, lo, hi) in [
            (Mode::Sam, -5000.0f32, 5000.0f32),
            (Mode::SamLower, -5000.0, 100.0),
            (Mode::SamUpper, -100.0, 5000.0),
        ] {
            let pkt = Command::Audio(AudioChannel {
                mode,
                ..base_audio()
            })
            .encode(&Policy::default(), 9)
            .unwrap_or_default();
            let e = entries(&pkt);
            let flag = |t: u8| e.iter().find(|x| x.ty == t).map(|x| x.bool());
            let float = |t: u8| e.iter().find(|x| x.ty == t).map(|x| x.f32());
            assert_eq!(flag(ty::PLL_ENABLE), Some(Ok(true)), "{mode:?}");
            assert_eq!(flag(ty::PLL_SQUARE), Some(Ok(false)), "{mode:?}");
            assert_eq!(flag(ty::ENVELOPE), Some(Ok(false)), "{mode:?}");
            assert_eq!(flag(ty::SNR_SQUELCH), Some(Ok(false)), "{mode:?}");
            assert_eq!(flag(ty::AGC_ENABLE), Some(Ok(true)), "{mode:?}");
            assert_eq!(float(ty::SQUELCH_OPEN), Some(Ok(SAM_SQUELCH_OPEN_DB)));
            assert_eq!(float(ty::SQUELCH_CLOSE), Some(Ok(SAM_SQUELCH_OPEN_DB)));
            assert_eq!(float(ty::LOW_EDGE), Some(Ok(lo)), "{mode:?}");
            assert_eq!(float(ty::HIGH_EDGE), Some(Ok(hi)), "{mode:?}");
            // The carrier (0 Hz) stays inside the passband, or the PLL has nothing to track.
            assert!(lo < 0.0 && hi > 0.0, "{mode:?}");
            assert!(e
                .iter()
                .any(|x| x.ty == ty::DEMOD_TYPE && x.uint() == Ok(0)));
            assert!(mode.is_synchronous());
        }
    }

    // Tests [@ANCHOR: radiod_client_command_mode_is_synchronous]
    #[test]
    fn every_other_linear_mode_turns_the_pll_off() {
        // A channel switched from SAM back to AM (or any other linear mode) must lose its PLL:
        // radiod keeps a channel's parameters across commands.
        for mode in [Mode::Usb, Mode::Lsb, Mode::Cw, Mode::Am, Mode::Iq] {
            assert!(!mode.is_synchronous(), "{mode:?}");
            let pkt = Command::Audio(AudioChannel {
                mode,
                ..base_audio()
            })
            .encode(&Policy::default(), 9)
            .unwrap_or_default();
            assert!(
                entries(&pkt)
                    .iter()
                    .any(|x| x.ty == ty::PLL_ENABLE && x.bool() == Ok(false)),
                "{mode:?}"
            );
        }
        assert!(!Mode::Fm.is_synchronous());
    }
}
