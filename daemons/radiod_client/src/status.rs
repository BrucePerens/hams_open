// SPDX-License-Identifier: AGPL-3.0-or-later
//! Parsed channel status.

use crate::tlv::{parse, ty, Kind, TlvError};
use std::net::SocketAddr;

// [@ANCHOR: radiod_client_status_channel_status]
/// A decoded status datagram. Unknown types are ignored, as the protocol intends.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChannelStatus {
    /// SSRC the status is about (`0` is the front end).
    pub ssrc: u32,
    /// Echoed command tag.
    pub tag: Option<u64>,
    /// Commands received by this channel.
    pub command_count: Option<u64>,
    /// Description (front end status only).
    pub description: Option<String>,
    /// Carrier frequency, Hz.
    pub frequency_hz: Option<f64>,
    /// Demodulator type number.
    pub demod_type: Option<u64>,
    /// Output sample rate, Hz.
    pub samprate: Option<u64>,
    /// Output channel count.
    pub channels: Option<u64>,
    /// Output encoding number (`rtp.h`: 2 S16BE, 3 Opus).
    pub encoding: Option<u64>,
    /// RTP payload type for this channel's stream.
    pub rtp_pt: Option<u64>,
    /// RTP destination.
    pub data_dest: Option<SocketAddr>,
    /// Remaining lifetime, frames.
    pub lifetime_frames: Option<u64>,
    /// Baseband power, dB re full scale.
    pub baseband_power_db: Option<f32>,
    /// Noise density, dB/Hz.
    pub noise_density_db: Option<f32>,
    /// Front end input sample rate, Hz.
    pub input_samprate: Option<u64>,
    /// A/D overrange count.
    pub ad_over: Option<u64>,
    /// Filter blocks dropped because the channel fell behind.
    pub filter_drops: Option<u64>,
    /// RTP packets sent.
    pub packets_out: Option<u64>,
    /// Spectrum: bins per response.
    pub bin_count: Option<u64>,
    /// Spectrum: bin width, Hz.
    pub bin_hz: Option<f32>,
    /// Spectrum: FFTs averaged.
    pub averaging: Option<u64>,
    /// Spectrum: FFT length.
    pub fft_n: Option<u64>,
    /// Spectrum: noise bandwidth of one bin, Hz. (Observed on the RX-888, 2026-11-05: 45,000 for a
    /// 30 kHz bin and 150 for a 100 Hz bin, so Hz, not bins as this field was first documented.)
    pub noise_bw_bins: Option<f32>,
    /// Spectrum: float bin power (linear), DC first, then positive, then negative frequencies.
    pub bin_data: Vec<f32>,
    /// Spectrum (byte format): one byte per bin, same order; level in dB is `base_db + step_db * byte`.
    pub bin_bytes: Vec<u8>,
    /// Spectrum (byte format): level of byte value 0, dB.
    pub base_db: Option<f32>,
    /// Spectrum (byte format): dB per byte step.
    pub step_db: Option<f32>,
    /// Channel passband low edge, Hz relative to the carrier.
    pub low_edge_hz: Option<f32>,
    /// Channel passband high edge, Hz relative to the carrier.
    pub high_edge_hz: Option<f32>,
    /// Channel AGC on.
    pub agc: Option<bool>,
    /// Channel (audio) gain, dB.
    pub gain_db: Option<f32>,
    /// Channel output level, dB re full scale.
    pub output_level_db: Option<f32>,
    /// Front end: A/D input power, dB re full scale (before the RF gain is removed). `-inf` while the
    /// front end is stopped (no channels).
    pub if_power_db: Option<f32>,
    /// Front end: RF gain now, dB (moves under the front end's RF AGC).
    pub rf_gain_db: Option<f32>,
    /// Front end: RF attenuation now, dB.
    pub rf_atten_db: Option<f32>,
    /// Front end: RF AGC on.
    pub rf_agc: Option<bool>,
    /// Front end: level calibration, dB (dBm to dBFS).
    pub rf_level_cal_db: Option<f32>,
    /// Front end: lowest usable frequency, Hz.
    pub fe_low_edge_hz: Option<f32>,
    /// Front end: highest usable frequency, Hz.
    pub fe_high_edge_hz: Option<f32>,
    /// Front end: real (direct-sampling) samples rather than complex.
    pub fe_is_real: Option<bool>,
    /// Front end: A/D bits per sample.
    pub ad_bits: Option<u64>,
    /// Front end: samples taken since start; two readings over a known time give the real rate.
    pub input_samples: Option<u64>,
    /// Front end: samples since the last A/D overrange.
    pub samples_since_over: Option<u64>,
}

impl ChannelStatus {
    // [@ANCHOR: radiod_client_status_decode]
    /// Decode a datagram. Returns `Ok(None)` for a command packet (our own, echoed back by
    /// multicast loopback).
    pub fn decode(pkt: &[u8]) -> Result<Option<Self>, TlvError> {
        let (kind, entries) = parse(pkt)?;
        if kind != Kind::Status {
            return Ok(None);
        }
        let mut s = Self::default();
        for e in entries {
            match e.ty {
                ty::OUTPUT_SSRC => s.ssrc = e.uint()? as u32,
                ty::COMMAND_TAG => s.tag = Some(e.uint()?),
                ty::CMD_CNT => s.command_count = Some(e.uint()?),
                ty::DESCRIPTION => s.description = Some(e.text()),
                ty::RADIO_FREQUENCY => s.frequency_hz = Some(e.f64()?),
                ty::DEMOD_TYPE => s.demod_type = Some(e.uint()?),
                ty::OUTPUT_SAMPRATE => s.samprate = Some(e.uint()?),
                ty::OUTPUT_CHANNELS => s.channels = Some(e.uint()?),
                ty::OUTPUT_ENCODING => s.encoding = Some(e.uint()?),
                ty::RTP_PT => s.rtp_pt = Some(e.uint()?),
                ty::OUTPUT_DATA_DEST_SOCKET => s.data_dest = e.socket()?,
                ty::LIFETIME => s.lifetime_frames = Some(e.uint()?),
                ty::BASEBAND_POWER => s.baseband_power_db = Some(e.f32()?),
                ty::NOISE_DENSITY => s.noise_density_db = Some(e.f32()?),
                ty::INPUT_SAMPRATE => s.input_samprate = Some(e.uint()?),
                ty::AD_OVER => s.ad_over = Some(e.uint()?),
                ty::FILTER_DROPS => s.filter_drops = Some(e.uint()?),
                ty::OUTPUT_DATA_PACKETS => s.packets_out = Some(e.uint()?),
                ty::BIN_COUNT => s.bin_count = Some(e.uint()?),
                ty::RESOLUTION_BW => s.bin_hz = Some(e.f32()?),
                ty::SPECTRUM_AVG => s.averaging = Some(e.uint()?),
                ty::SPECTRUM_FFT_N => s.fft_n = Some(e.uint()?),
                ty::NOISE_BW => s.noise_bw_bins = Some(e.f32()?),
                ty::BIN_DATA => s.bin_data = e.f32_vec()?,
                ty::BIN_BYTE_DATA => s.bin_bytes = e.value.to_vec(),
                ty::SPECTRUM_BASE => s.base_db = Some(e.f32()?),
                ty::SPECTRUM_STEP => s.step_db = Some(e.f32()?),
                ty::LOW_EDGE => s.low_edge_hz = Some(e.f32()?),
                ty::HIGH_EDGE => s.high_edge_hz = Some(e.f32()?),
                ty::AGC_ENABLE => s.agc = Some(e.bool()?),
                ty::GAIN => s.gain_db = Some(e.f32()?),
                ty::OUTPUT_LEVEL => s.output_level_db = Some(e.f32()?),
                ty::IF_POWER => s.if_power_db = Some(e.f32()?),
                ty::RF_GAIN => s.rf_gain_db = Some(e.f32()?),
                ty::RF_ATTEN => s.rf_atten_db = Some(e.f32()?),
                ty::RF_AGC => s.rf_agc = Some(e.bool()?),
                ty::RF_LEVEL_CAL => s.rf_level_cal_db = Some(e.f32()?),
                ty::FE_LOW_EDGE => s.fe_low_edge_hz = Some(e.f32()?),
                ty::FE_HIGH_EDGE => s.fe_high_edge_hz = Some(e.f32()?),
                ty::FE_ISREAL => s.fe_is_real = Some(e.bool()?),
                ty::AD_BITS_PER_SAMPLE => s.ad_bits = Some(e.uint()?),
                ty::INPUT_SAMPLES => s.input_samples = Some(e.uint()?),
                ty::SAMPLES_SINCE_OVER => s.samples_since_over = Some(e.uint()?),
                _ => {}
            }
        }
        Ok(Some(s))
    }

    // [@ANCHOR: radiod_client_status_spectrum_db]
    /// Spectrum bins in ascending frequency order with their frequencies, in dB.
    ///
    /// The two formats differ in order, as observed from `radiod`: the float format
    /// (`BIN_DATA`) sends DC first, then positive, then negative frequencies; the byte format
    /// (`BIN_BYTE_DATA`) is already low to high with the most negative bin first. The level is
    /// power per bin relative to full scale (a tone at -20 dBFS reads -20 dB); it is not
    /// calibrated to dBm without the front end's level calibration.
    pub fn spectrum_db(&self, center_hz: f64) -> Vec<(f64, f32)> {
        let bw = f64::from(self.bin_hz.unwrap_or(0.0));
        if bw == 0.0 {
            return Vec::new();
        }
        if self.bin_data.is_empty() {
            let (base, step) = (self.base_db.unwrap_or(-150.0), self.step_db.unwrap_or(0.5));
            let n = self.bin_bytes.len();
            return self
                .bin_bytes
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    (
                        center_hz + (i as f64 - (n / 2) as f64) * bw,
                        base + step * f32::from(*b),
                    )
                })
                .collect();
        }
        let n = self.bin_data.len();
        let half = n / 2;
        let mut out: Vec<(f64, f32)> = (0..n)
            .map(|i| {
                let off = if i < half {
                    i as f64
                } else {
                    i as f64 - n as f64
                };
                (
                    center_hz + off * bw,
                    10.0 * self.bin_data[i].max(1e-30).log10(),
                )
            })
            .collect();
        out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a status datagram (packet kind 0) from raw entries.
    fn status(entries: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let mut v = vec![0u8];
        for (t, val) in entries {
            v.push(*t);
            v.push(val.len() as u8);
            v.extend_from_slice(val);
        }
        v.push(ty::EOL);
        v
    }

    // Tests [@ANCHOR: radiod_client_status_decode]
    #[test]
    fn decodes_channel_fields_by_tag() {
        let pkt = status(&[
            (ty::OUTPUT_SSRC, 777u32.to_be_bytes().to_vec()),
            (ty::COMMAND_TAG, vec![9]),
            (ty::RADIO_FREQUENCY, 7.074e6f64.to_be_bytes().to_vec()),
            (ty::OUTPUT_SAMPRATE, vec![0x2e, 0xe0]),
            (ty::OUTPUT_ENCODING, vec![2]),
            (ty::OUTPUT_DATA_DEST_SOCKET, vec![239, 1, 1, 2, 0x13, 0x8c]),
            (ty::LIFETIME, vec![100]),
            (ty::BASEBAND_POWER, (-20.0f32).to_be_bytes().to_vec()),
            (ty::FILTER_DROPS, vec![]),
            (200, vec![1, 2, 3]),
        ]);
        let s = ChannelStatus::decode(&pkt)
            .unwrap_or_default()
            .unwrap_or_default();
        assert_eq!(s.ssrc, 777);
        assert_eq!(s.tag, Some(9));
        assert_eq!(s.frequency_hz, Some(7.074e6));
        assert_eq!(s.samprate, Some(12_000));
        assert_eq!(s.encoding, Some(2));
        assert_eq!(s.lifetime_frames, Some(100));
        assert_eq!(s.baseband_power_db, Some(-20.0));
        assert_eq!(s.filter_drops, Some(0));
        assert_eq!(
            s.data_dest.map(|d| d.to_string()),
            Some("239.1.1.2:5004".to_string())
        );
    }

    // Tests [@ANCHOR: radiod_client_status_decode]
    #[test]
    fn our_own_command_packets_are_skipped_and_garbage_is_an_error() {
        let mut pkt = status(&[(ty::OUTPUT_SSRC, vec![5])]);
        pkt[0] = 1;
        assert_eq!(ChannelStatus::decode(&pkt), Ok(None));
        assert!(ChannelStatus::decode(&[]).is_err());
        assert!(ChannelStatus::decode(&[0, ty::OUTPUT_SSRC, 9]).is_err());
        // A destination field of the wrong length is an error, not a panic.
        let bad = status(&[(ty::OUTPUT_DATA_DEST_SOCKET, vec![1, 2, 3])]);
        assert!(ChannelStatus::decode(&bad).is_err());
    }

    // Tests [@ANCHOR: radiod_client_status_spectrum_db]
    // Tests [@ANCHOR: radiod_client_status_decode]
    #[test]
    fn float_bins_are_unwrapped_to_ascending_frequency() {
        // DC first, then positive, then negative frequencies; the tone is at +2 bins.
        let mut bins = [1e-10f32; 8];
        bins[2] = 0.01; // -20 dB
        let raw: Vec<u8> = bins.iter().flat_map(|b| b.to_be_bytes()).collect();
        let mut pkt = vec![0u8, ty::BIN_DATA, 0x81, raw.len() as u8];
        pkt.extend_from_slice(&raw);
        pkt.extend_from_slice(&[ty::RESOLUTION_BW, 4]);
        pkt.extend_from_slice(&100.0f32.to_be_bytes());
        pkt.push(ty::EOL);
        let s = ChannelStatus::decode(&pkt)
            .unwrap_or_default()
            .unwrap_or_default();
        let db = s.spectrum_db(10.0e6);
        assert_eq!(db.len(), 8);
        assert!(db.windows(2).all(|w| w[0].0 < w[1].0));
        let peak = db
            .iter()
            .fold((0.0, f32::MIN), |a, b| if b.1 > a.1 { *b } else { a });
        assert_eq!(peak.0, 10_000_200.0);
        assert!((peak.1 + 20.0).abs() < 1e-3);
        assert_eq!(db[0].0, 10_000_000.0 - 4.0 * 100.0);
    }

    // Tests [@ANCHOR: radiod_client_status_spectrum_db]
    // Tests [@ANCHOR: radiod_client_status_decode]
    #[test]
    fn byte_bins_are_already_ascending_and_use_base_and_step() {
        let mut raw = vec![0u8; 8];
        raw[6] = 255;
        let mut pkt = vec![0u8, ty::BIN_BYTE_DATA, raw.len() as u8];
        pkt.extend_from_slice(&raw);
        pkt.extend_from_slice(&[ty::RESOLUTION_BW, 4]);
        pkt.extend_from_slice(&1000.0f32.to_be_bytes());
        pkt.extend_from_slice(&[ty::SPECTRUM_BASE, 4]);
        pkt.extend_from_slice(&(-150.0f32).to_be_bytes());
        pkt.extend_from_slice(&[ty::SPECTRUM_STEP, 4]);
        pkt.extend_from_slice(&0.5f32.to_be_bytes());
        pkt.push(ty::EOL);
        let s = ChannelStatus::decode(&pkt)
            .unwrap_or_default()
            .unwrap_or_default();
        let db = s.spectrum_db(10.0e6);
        assert_eq!(db.len(), 8);
        assert_eq!(db[0], (10.0e6 - 4000.0, -150.0));
        assert_eq!(db[6], (10.0e6 + 2000.0, -150.0 + 0.5 * 255.0));
        // No bin width means no frequency axis.
        assert!(ChannelStatus::default().spectrum_db(1.0).is_empty());
    }

    /// A front-end status as the RX-888's `radiod` sent it on 2026-11-05 (pinned commit 8c50dbbe), in
    /// reply to a front-end poll while no channel existed. Fields: description, GPS time, input rate,
    /// real samples, RF gain, attenuation (a float 0 sent as an empty value), RF AGC, level
    /// calibration, usable edges, A/D bits, IF power (`-inf`, the front end is stopped), overrange
    /// count and the status group. No command tag: front-end replies do not echo one.
    const RX888_FRONT_END: &str = "001200041e72783838382062656e63682062727563653136206e6f20616e74656e6e6103081482c5860288e40d0a0403dcc50066010118006204411fc5af220061006e043fb333336301016404466a600065044be85c605201102a0313c6802b0304f1a12d04ff80000068000506ef580801138e00";

    fn unhex(h: &str) -> Vec<u8> {
        (0..h.len() / 2)
            .filter_map(|i| u8::from_str_radix(h.get(2 * i..2 * i + 2)?, 16).ok())
            .collect()
    }

    // Tests [@ANCHOR: radiod_client_status_decode]
    // Tests [@ANCHOR: radiod_client_status_channel_status]
    #[test]
    fn decodes_a_real_rx888_front_end_status() {
        let pkt = unhex(RX888_FRONT_END);
        assert_eq!(pkt.len(), 117);
        let st = ChannelStatus::decode(&pkt)
            .ok()
            .flatten()
            .unwrap_or_default();
        assert_eq!(st.ssrc, 0);
        assert_eq!(st.tag, None, "front-end replies carry no tag");
        assert_eq!(
            st.description.as_deref(),
            Some("rx888 bench bruce16 no antenna")
        );
        assert_eq!(st.input_samprate, Some(64_800_000));
        assert_eq!(st.fe_is_real, Some(true));
        assert_eq!(st.rf_agc, Some(true));
        assert_eq!(st.rf_atten_db, Some(0.0));
        assert!(st.rf_gain_db.is_some_and(|g| (g - 9.99).abs() < 0.01));
        assert!(st.rf_level_cal_db.is_some_and(|c| (c - 1.4).abs() < 1e-6));
        assert_eq!(st.fe_low_edge_hz, Some(15_000.0));
        assert_eq!(st.fe_high_edge_hz, Some(30_456_000.0));
        assert_eq!(st.ad_bits, Some(16));
        assert_eq!(st.ad_over, Some(0));
        assert!(st.if_power_db.is_some_and(|p| p == f32::NEG_INFINITY));
        assert_eq!(st.frequency_hz, None);
    }

    // Tests [@ANCHOR: radiod_client_status_decode]
    #[test]
    fn decodes_channel_passband_gain_and_counters() {
        let f = |v: f32| v.to_bits().to_be_bytes().to_vec();
        let pkt = status(&[
            (ty::OUTPUT_SSRC, vec![0x03, 0xe9]),
            (ty::LOW_EDGE, f(-5000.0)),
            (ty::HIGH_EDGE, f(5000.0)),
            (ty::AGC_ENABLE, vec![1]),
            (ty::GAIN, f(61.2)),
            (ty::OUTPUT_LEVEL, f(-53.0)),
            (ty::INPUT_SAMPLES, vec![0x02, 0x16, 0x0e, 0xc0]),
            (ty::SAMPLES_SINCE_OVER, vec![0x10]),
        ]);
        let st = ChannelStatus::decode(&pkt)
            .ok()
            .flatten()
            .unwrap_or_default();
        assert_eq!(st.ssrc, 1001);
        assert_eq!(
            (st.low_edge_hz, st.high_edge_hz),
            (Some(-5000.0), Some(5000.0))
        );
        assert_eq!(st.agc, Some(true));
        assert_eq!(st.gain_db, Some(61.2));
        assert_eq!(st.output_level_db, Some(-53.0));
        assert_eq!(st.input_samples, Some(35_000_000));
        assert_eq!(st.samples_since_over, Some(16));
    }
}
