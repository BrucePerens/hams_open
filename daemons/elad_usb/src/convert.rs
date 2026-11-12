// SPDX-License-Identifier: AGPL-3.0-or-later
//! Wire bytes to complex samples. Below 6.144 Msps a component is a signed little-endian 32-bit integer (8 bytes per I/Q pair);
//! at 6.144 Msps it is a signed little-endian 16-bit integer (4 bytes per pair). I comes before Q. Byte order and I/Q order are
//! `verified=unverified` until a capture of a known tone has been decoded (README.md).

/// The component width of a rate.
// [@ANCHOR: convert_format_enum]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// 32-bit signed little-endian components.
    S32Le,
    /// 16-bit signed little-endian components.
    S16Le,
}

impl Format {
    /// The format used at `rate_hz`.
    // [@ANCHOR: elad_format_for_rate]
    pub fn for_rate(rate_hz: u32) -> Format {
        if rate_hz >= 6_144_000 {
            Format::S16Le
        } else {
            Format::S32Le
        }
    }

    /// Bytes per component.
    // [@ANCHOR: elad_format_component_bytes]
    pub fn component_bytes(self) -> usize {
        match self {
            Format::S32Le => 4,
            Format::S16Le => 2,
        }
    }

    /// Full-scale divisor.
    // [@ANCHOR: elad_format_full_scale]
    pub fn full_scale(self) -> f32 {
        match self {
            Format::S32Le => 2_147_483_648.0,
            Format::S16Le => 32_768.0,
        }
    }
}

/// Decodes a byte stream to interleaved I, Q floats in +-1.0, carrying a partial pair across calls.
// [@ANCHOR: convert_decoder_struct]
#[derive(Debug, Clone)]
pub struct Decoder {
    format: Format,
    carry: Vec<u8>,
}

impl Decoder {
    /// A decoder for `format`.
    pub fn new(format: Format) -> Decoder {
        Decoder {
            format,
            carry: Vec::new(),
        }
    }

    /// Forgets a partial pair, as after a stall or a restart.
    pub fn reset(&mut self) {
        self.carry.clear();
    }

    /// Appends the I/Q pairs completed by `bytes` to `out` as I, Q, I, Q floats. Returns the number of pairs appended.
    // [@ANCHOR: elad_decoder_push]
    pub fn push(&mut self, bytes: &[u8], out: &mut Vec<f32>) -> usize {
        let cb = self.format.component_bytes();
        let pair = cb * 2;
        let scale = 1.0 / self.format.full_scale();
        let mut data: Vec<u8>;
        let src: &[u8] = if self.carry.is_empty() {
            bytes
        } else {
            data = std::mem::take(&mut self.carry);
            data.extend_from_slice(bytes);
            &data
        };
        let whole = src.len() / pair * pair;
        for c in src[..whole].chunks_exact(cb) {
            let v = match self.format {
                Format::S32Le => i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32,
                Format::S16Le => f32::from(i16::from_le_bytes([c[0], c[1]])),
            };
            out.push(v * scale);
        }
        self.carry = src[whole..].to_vec();
        whole / pair
    }
}
