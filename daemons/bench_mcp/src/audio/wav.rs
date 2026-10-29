// SPDX-License-Identifier: AGPL-3.0-or-later
//! Minimal WAV reading and writing: 16, 24 and 32-bit integer PCM and 32-bit float in, 16-bit PCM out.

use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Wav {
    pub rate: u32,
    pub channels: u16,
    /// Interleaved samples, full scale 1.0.
    pub samples: Vec<f32>,
}

impl Wav {
    pub fn seconds(&self) -> f64 {
        self.samples.len() as f64 / f64::from(self.channels.max(1)) / f64::from(self.rate.max(1))
    }

    /// The channels averaged into one.
    pub fn mono(&self) -> Vec<f32> {
        let c = usize::from(self.channels.max(1));
        if c == 1 {
            return self.samples.clone();
        }
        self.samples
            .chunks_exact(c)
            .map(|f| f.iter().sum::<f32>() / c as f32)
            .collect()
    }
}

fn u16le(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

pub fn parse(b: &[u8]) -> Result<Wav, String> {
    // [@ANCHOR: audio_wav]
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".into());
    }
    let (mut at, mut fmt, mut data) = (12, None, None);
    while at + 8 <= b.len() {
        let id = &b[at..at + 4];
        let size = u32le(b, at + 4) as usize;
        let body = at + 8;
        let end = body.saturating_add(size).min(b.len());
        if id == b"fmt " && end - body >= 16 {
            fmt = Some((
                u16le(b, body),
                u16le(b, body + 2),
                u32le(b, body + 4),
                u16le(b, body + 14),
            ));
        } else if id == b"data" {
            data = Some(&b[body..end]);
        }
        at = body.saturating_add(size).saturating_add(size & 1);
    }
    let ((tag, channels, rate, bits), data) = fmt
        .zip(data)
        .ok_or("the WAV file has no fmt or data chunk")?;
    if channels == 0 || rate == 0 {
        return Err("the WAV file has zero channels or a zero sample rate".into());
    }
    let samples: Vec<f32> = match (tag, bits) {
        (1 | 0xfffe, 16) => data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| f32::from(i16::from_le_bytes(*c)) / 32768.0)
            .collect(),
        (1 | 0xfffe, 24) => data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0)
            .collect(),
        (1 | 0xfffe, 32) => data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| i32::from_le_bytes(*c) as f32 / 2_147_483_648.0)
            .collect(),
        (3, 32) => data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect(),
        _ => return Err(format!("unsupported WAV format tag {tag} with {bits} bits")),
    };
    Ok(Wav {
        rate,
        channels,
        samples,
    })
}

pub fn read(path: &Path) -> Result<Wav, String> {
    let b = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&b).map_err(|e| format!("{}: {e}", path.display()))
}

/// The bytes of a 16-bit PCM WAV; samples beyond full scale are clipped.
pub fn encode(w: &Wav) -> Vec<u8> {
    let data_len = (w.samples.len() * 2) as u32;
    let mut o = Vec::with_capacity(44 + data_len as usize);
    o.extend_from_slice(b"RIFF");
    o.extend_from_slice(&(36 + data_len).to_le_bytes());
    o.extend_from_slice(b"WAVEfmt ");
    o.extend_from_slice(&16u32.to_le_bytes());
    o.extend_from_slice(&1u16.to_le_bytes());
    o.extend_from_slice(&w.channels.to_le_bytes());
    o.extend_from_slice(&w.rate.to_le_bytes());
    o.extend_from_slice(&(w.rate * u32::from(w.channels) * 2).to_le_bytes());
    o.extend_from_slice(&(w.channels * 2).to_le_bytes());
    o.extend_from_slice(&16u16.to_le_bytes());
    o.extend_from_slice(b"data");
    o.extend_from_slice(&data_len.to_le_bytes());
    for s in &w.samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        o.extend_from_slice(&v.to_le_bytes());
    }
    o
}

pub fn write(path: &Path, w: &Wav) -> Result<(), String> {
    std::fs::write(path, encode(w)).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: audio_wav]
    #[test]
    fn a_wav_round_trips_within_one_sixteen_bit_step() {
        let w = Wav {
            rate: 44100,
            channels: 2,
            samples: vec![0.0, 0.5, -0.5, 1.0, -1.0, 0.25],
        };
        let back = parse(&encode(&w)).unwrap();
        assert_eq!((back.rate, back.channels), (44100, 2));
        for (a, b) in w.samples.iter().zip(&back.samples) {
            assert!((a - b).abs() <= 1.0 / 32767.0, "{a} {b}");
        }
        assert!((w.seconds() - 3.0 / 44100.0).abs() < 1e-12);
    }

    #[test]
    fn mono_averages_the_channels() {
        let w = Wav {
            rate: 8000,
            channels: 2,
            samples: vec![1.0, 0.0, 0.5, 0.5],
        };
        assert_eq!(w.mono(), vec![0.5, 0.5]);
    }

    #[test]
    fn clipping_is_applied_on_write() {
        let loud = Wav {
            rate: 8000,
            channels: 1,
            samples: vec![3.0, -3.0],
        };
        assert_eq!(parse(&encode(&loud)).unwrap().samples[0], 32767.0 / 32768.0);
    }

    #[test]
    fn float_and_24_bit_files_and_unknown_chunks_are_read() {
        let mut f = Vec::new();
        f.extend_from_slice(b"RIFF\0\0\0\0WAVE");
        f.extend_from_slice(b"junk\x03\0\0\0abc\0");
        f.extend_from_slice(b"fmt \x10\0\0\0\x03\0\x01\0\x40\x1f\0\0\0\0\0\0\x04\0\x20\0");
        f.extend_from_slice(b"data\x08\0\0\0");
        f.extend_from_slice(&0.25f32.to_le_bytes());
        f.extend_from_slice(&(-0.5f32).to_le_bytes());
        let w = parse(&f).unwrap();
        assert_eq!((w.rate, w.samples.clone()), (8000, vec![0.25, -0.5]));
        let mut g = Vec::new();
        g.extend_from_slice(b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x40\x1f\0\0\0\0\0\0\x03\0\x18\0data\x03\0\0\0\0\0\x40");
        assert_eq!(parse(&g).unwrap().samples, vec![0.5]);
    }

    #[test]
    fn garbage_and_unsupported_files_are_errors() {
        assert!(parse(b"hello").is_err());
        assert!(parse(b"RIFF\0\0\0\0WAVEdata\0\0\0\0").is_err());
        let mut f = Vec::new();
        f.extend_from_slice(b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x02\0\x01\0\x40\x1f\0\0\0\0\0\0\x01\0\x04\0data\0\0\0\0");
        assert!(parse(&f).unwrap_err().contains("unsupported"));
    }
}
