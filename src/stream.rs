//! Parsing of the isochronous sample stream (protocol document 1.7).
//!
//! Each isochronous packet carries one to three 1024-byte blocks. A block is a 16-byte header
//! (bytes 0-3: little-endian sample counter; bytes 4-15: status words of unknown meaning) and
//! 1008 bytes of interleaved I/Q samples in one of four codings.

use crate::encode::{SampleFormat, StreamConfig};

pub const BLOCK_LEN: usize = 1024;
pub const HEADER_LEN: usize = 16;
pub const PAYLOAD_LEN: usize = BLOCK_LEN - HEADER_LEN;

/// One complex sample, I and Q left-aligned to 16 bits whatever the wire coding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IqI16 {
    pub i: i16,
    pub q: i16,
}

/// One complex sample as floats in -1.0..1.0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IqF32 {
    pub i: f32,
    pub q: f32,
}

impl From<IqI16> for IqF32 {
    fn from(s: IqI16) -> Self {
        IqF32 { i: s.i as f32 / 32768.0, q: s.q as f32 / 32768.0 }
    }
}

/// Convert a slice of 16-bit samples to floats.
pub fn to_f32(src: &[IqI16]) -> Vec<IqF32> {
    src.iter().map(|&s| s.into()).collect()
}

/// Running statistics of a stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamStats {
    /// Blocks accepted.
    pub blocks: u64,
    /// Discontinuities in the sample counter.
    pub gaps: u64,
    /// Samples (ADC clocks, in decimate-by-2 mode) missing according to the counter.
    pub lost_samples: u64,
    /// Packets shorter than a block or with trailing bytes (ignored).
    pub short_packets: u64,
    /// Samples delivered.
    pub samples: u64,
}

/// Splits packets into blocks, checks the counter and decodes samples.
#[derive(Clone, Debug)]
pub struct BlockParser {
    format: SampleFormat,
    step: u32,
    next: Option<u32>,
    pub stats: StreamStats,
}

/// The block counter of one block.
pub fn block_counter(block: &[u8]) -> u32 {
    u32::from_le_bytes([block[0], block[1], block[2], block[3]])
}

impl BlockParser {
    pub fn new(cfg: &StreamConfig) -> Self {
        BlockParser { format: cfg.format, step: cfg.counter_step(), next: None, stats: StreamStats::default() }
    }

    /// Forget the expected counter (after a restart).
    pub fn reset_counter(&mut self) {
        self.next = None;
    }

    /// Parse one isochronous packet, appending decoded samples to `out`.
    pub fn push_packet(&mut self, pkt: &[u8], out: &mut Vec<IqI16>) {
        if !pkt.len().is_multiple_of(BLOCK_LEN) || pkt.is_empty() {
            self.stats.short_packets += 1;
        }
        for block in pkt.as_chunks::<BLOCK_LEN>().0 {
            let cnt = block_counter(block);
            if let Some(exp) = self.next {
                if cnt != exp {
                    self.stats.gaps += 1;
                    self.stats.lost_samples += cnt.wrapping_sub(exp) as u64;
                }
            }
            self.next = Some(cnt.wrapping_add(self.step));
            self.stats.blocks += 1;
            let before = out.len();
            decode_payload(self.format, &block[HEADER_LEN..], out);
            self.stats.samples += (out.len() - before) as u64;
        }
    }
}

fn sext(v: u32, bits: u32) -> i32 {
    let shift = 32 - bits;
    ((v << shift) as i32) >> shift
}

/// Decode the 1008-byte payload of one block.
pub fn decode_payload(format: SampleFormat, p: &[u8], out: &mut Vec<IqI16>) {
    debug_assert_eq!(p.len(), PAYLOAD_LEN);
    match format {
        SampleFormat::S14 => {
            // 16-bit little-endian words, I then Q; samples are 14-bit signed.
            for w in p.as_chunks::<4>().0 {
                let i = sext(u16::from_le_bytes([w[0], w[1]]) as u32, 14) << 2;
                let q = sext(u16::from_le_bytes([w[2], w[3]]) as u32, 14) << 2;
                out.push(IqI16 { i: i as i16, q: q as i16 });
            }
        }
        SampleFormat::S8 => {
            for w in p.as_chunks::<2>().0 {
                out.push(IqI16 { i: ((w[0] as i8 as i16) << 8), q: ((w[1] as i8 as i16) << 8) });
            }
        }
        SampleFormat::S12 => {
            // Each 3 bytes hold one I/Q pair: I = low 12 bits, Q = high 12 bits (LE bit order).
            for w in p.as_chunks::<3>().0 {
                let v = w[0] as u32 | (w[1] as u32) << 8 | (w[2] as u32) << 16;
                let i = sext(v & 0xfff, 12) << 4;
                let q = sext(v >> 12, 12) << 4;
                out.push(IqI16 { i: i as i16, q: q as i16 });
            }
        }
        SampleFormat::S10x384 => {
            // Six groups of 164 bytes: 16 runs of 10 bytes (8 x 10-bit values, LE bit order,
            // alternating I/Q) followed by a 32-bit word holding a 2-bit scale per run
            // (0: x1, 1: x2, 2 or 3: x4). The last 24 bytes of the payload carry no samples.
            for g in 0..6 {
                let grp = &p[g * 164..g * 164 + 164];
                let scale = u32::from_le_bytes([grp[160], grp[161], grp[162], grp[163]]);
                for k in 0..16 {
                    let run = &grp[k * 10..k * 10 + 10];
                    let shift = match (scale >> (2 * k)) & 3 {
                        0 => 4,
                        1 => 5,
                        _ => 6,
                    };
                    let bits = run.iter().enumerate().fold(0u128, |a, (n, &b)| a | (b as u128) << (8 * n));
                    for j in 0..4 {
                        let iv = sext(((bits >> (20 * j)) & 0x3ff) as u32, 10) << shift;
                        let qv = sext(((bits >> (20 * j + 10)) & 0x3ff) as u32, 10) << shift;
                        out.push(IqI16 { i: iv as i16, q: qv as i16 });
                    }
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::encode::{stream_config, IfMode};

    /// Build a canned S14 block with counter `cnt` and samples (k, -k) for k in 0..252.
    pub fn s14_block(cnt: u32) -> Vec<u8> {
        let mut b = vec![0u8; BLOCK_LEN];
        b[..4].copy_from_slice(&cnt.to_le_bytes());
        b[4..8].copy_from_slice(&[0x28, 0x10, 0, 0]);
        for k in 0..252usize {
            let i = (k as i16) * 30 - 3000;
            let q = -i;
            b[16 + 4 * k..18 + 4 * k].copy_from_slice(&(i as u16 & 0x3fff | if i < 0 { 0xc000 } else { 0 }).to_le_bytes());
            b[18 + 4 * k..20 + 4 * k].copy_from_slice(&(q as u16).to_le_bytes());
        }
        b
    }

    #[test]
    fn s14_packet_with_gap() {
        let cfg = stream_config(2_000_000, IfMode::Zero).unwrap();
        let mut p = BlockParser::new(&cfg);
        let mut out = Vec::new();
        p.push_packet(&s14_block(1000), &mut out);
        p.push_packet(&s14_block(1252), &mut out);
        assert_eq!(p.stats.gaps, 0);
        // One block missing: 1504 skipped.
        p.push_packet(&s14_block(1756), &mut out);
        assert_eq!(p.stats.gaps, 1);
        assert_eq!(p.stats.lost_samples, 252);
        assert_eq!(p.stats.blocks, 3);
        assert_eq!(out.len(), 3 * 252);
        assert_eq!(out[0], IqI16 { i: -3000 * 4, q: 3000 * 4 });
        assert_eq!(out[251], IqI16 { i: (251 * 30 - 3000) * 4, q: -(251 * 30 - 3000) * 4 });
    }

    #[test]
    fn counter_wraps() {
        let cfg = stream_config(2_000_000, IfMode::Zero).unwrap();
        let mut p = BlockParser::new(&cfg);
        let mut out = Vec::new();
        p.push_packet(&s14_block(u32::MAX - 100), &mut out);
        p.push_packet(&s14_block((u32::MAX - 100).wrapping_add(252)), &mut out);
        assert_eq!(p.stats.gaps, 0);
    }

    #[test]
    fn three_blocks_per_packet_and_decim2_step() {
        let cfg = stream_config(6_000_000, IfMode::Low1620k).unwrap();
        let mut p = BlockParser::new(&cfg);
        let mut pkt = Vec::new();
        for n in 0..3 {
            pkt.extend(s14_block(n * 504));
        }
        let mut out = Vec::new();
        p.push_packet(&pkt, &mut out);
        assert_eq!((p.stats.blocks, p.stats.gaps, out.len()), (3, 0, 756));
        p.push_packet(&pkt[..100], &mut out);
        assert_eq!(p.stats.short_packets, 1);
    }

    #[test]
    fn s8_decode() {
        let mut pl = vec![0u8; PAYLOAD_LEN];
        pl[0] = 0x7f;
        pl[1] = 0x80;
        pl[2] = 0xff;
        pl[3] = 0x01;
        let mut out = Vec::new();
        decode_payload(SampleFormat::S8, &pl, &mut out);
        assert_eq!(out.len(), 504);
        assert_eq!(out[0], IqI16 { i: 127 << 8, q: -128 << 8 });
        assert_eq!(out[1], IqI16 { i: -256, q: 256 });
    }

    #[test]
    fn s12_decode() {
        // I = 0x7ff (max), Q = 0x800 (min); then I = -1 (0xfff), Q = 1.
        let mut pl = vec![0u8; PAYLOAD_LEN];
        let pack = |i: u32, q: u32| -> [u8; 3] {
            let v = (i & 0xfff) | (q & 0xfff) << 12;
            [v as u8, (v >> 8) as u8, (v >> 16) as u8]
        };
        pl[0..3].copy_from_slice(&pack(0x7ff, 0x800));
        pl[3..6].copy_from_slice(&pack(0xfff, 0x001));
        let mut out = Vec::new();
        decode_payload(SampleFormat::S12, &pl, &mut out);
        assert_eq!(out.len(), 336);
        assert_eq!(out[0], IqI16 { i: 0x7ff << 4, q: -2048 << 4 });
        assert_eq!(out[1], IqI16 { i: -16, q: 16 });
    }

    #[test]
    fn s384_decode() {
        let mut pl = vec![0u8; PAYLOAD_LEN];
        // First run of group 0: values 1, -1, 511, -512, then zeros; scale code 0 (x1 -> <<4).
        let vals: [i32; 8] = [1, -1, 511, -512, 0, 0, 0, 0];
        let mut bits: u128 = 0;
        for (n, v) in vals.iter().enumerate() {
            bits |= ((*v as u32 & 0x3ff) as u128) << (10 * n);
        }
        for n in 0..10 {
            pl[n] = (bits >> (8 * n)) as u8;
        }
        // Second run of group 0 the same values with scale code 2 (<<6).
        let r1: Vec<u8> = pl[0..10].to_vec();
        pl[10..20].copy_from_slice(&r1);
        pl[160..164].copy_from_slice(&(2u32 << 2).to_le_bytes());
        let mut out = Vec::new();
        decode_payload(SampleFormat::S10x384, &pl, &mut out);
        assert_eq!(out.len(), 384);
        assert_eq!(out[0], IqI16 { i: 16, q: -16 });
        assert_eq!(out[1], IqI16 { i: 511 << 4, q: -512 << 4 });
        assert_eq!(out[4], IqI16 { i: 64, q: -64 });
        assert_eq!(out[5], IqI16 { i: 511 << 6, q: -512 << 6 });
    }

    #[test]
    fn f32_conversion() {
        let v = to_f32(&[IqI16 { i: 16384, q: -32768 }]);
        assert_eq!(v[0], IqF32 { i: 0.5, q: -1.0 });
    }
}
