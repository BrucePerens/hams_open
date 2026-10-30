// SPDX-License-Identifier: AGPL-3.0-or-later
//! Parsed channel status.

use crate::tlv::{parse, ty, Kind, TlvError};
use std::net::SocketAddr;

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
    /// Spectrum: noise bandwidth of a bin, in bins.
    pub noise_bw_bins: Option<f32>,
    /// Spectrum: float bin power (linear), DC first, then positive, then negative frequencies.
    pub bin_data: Vec<f32>,
    /// Spectrum (byte format): one byte per bin, same order; level in dB is `base_db + step_db * byte`.
    pub bin_bytes: Vec<u8>,
    /// Spectrum (byte format): level of byte value 0, dB.
    pub base_db: Option<f32>,
    /// Spectrum (byte format): dB per byte step.
    pub step_db: Option<f32>,
}

impl ChannelStatus {
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
                _ => {}
            }
        }
        Ok(Some(s))
    }

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
