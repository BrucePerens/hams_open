// SPDX-License-Identifier: AGPL-3.0-or-later
//! The sigrok session file (`.sr`) writer, with no dependency: a ZIP archive (stored, not compressed) holding `version`, `metadata` and
//! `logic-1-N` chunks of packed samples. PulseView and sigrok-cli open it. Entries: `docs/GLASGOW_LOGIC_TOOLS.md`.

use super::wave::Sampled;

/// Samples per `logic-1-N` chunk entry.
const CHUNK_SAMPLES: usize = 1 << 20;

/// CRC-32 (IEEE, as ZIP uses): `crc32(b"123456789") == 0xCBF43926`.
pub fn crc32(data: &[u8]) -> u32 {
    // [@ANCHOR: gla_crc32]
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A ZIP archive of the entries, each stored uncompressed, with a central directory (DOS time and date fixed at 1980-01-01 so a file's
/// bytes depend only on its content).
pub fn zip_store(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    // [@ANCHOR: gla_zip_store]
    let mut out: Vec<u8> = vec![];
    let mut central: Vec<u8> = vec![];
    for (name, data) in entries {
        let crc = crc32(data);
        let offset = out.len() as u32;
        let hdr = |buf: &mut Vec<u8>, sig: u32, local: bool| {
            buf.extend(sig.to_le_bytes());
            if !local {
                buf.extend(20u16.to_le_bytes());
            }
            buf.extend(20u16.to_le_bytes()); // version needed
            buf.extend(0u16.to_le_bytes()); // flags
            buf.extend(0u16.to_le_bytes()); // method: stored
            buf.extend(0u16.to_le_bytes()); // time
            buf.extend(0x21u16.to_le_bytes()); // date 1980-01-01
            buf.extend(crc.to_le_bytes());
            buf.extend((data.len() as u32).to_le_bytes());
            buf.extend((data.len() as u32).to_le_bytes());
            buf.extend((name.len() as u16).to_le_bytes());
            buf.extend(0u16.to_le_bytes()); // extra
        };
        hdr(&mut out, 0x0403_4b50, true);
        out.extend(name.as_bytes());
        out.extend(data);
        hdr(&mut central, 0x0201_4b50, false);
        central.extend(0u16.to_le_bytes()); // comment
        central.extend(0u16.to_le_bytes()); // disk
        central.extend(0u16.to_le_bytes()); // internal attrs
        central.extend(0u32.to_le_bytes()); // external attrs
        central.extend(offset.to_le_bytes());
        central.extend(name.as_bytes());
    }
    let cd_off = out.len() as u32;
    out.extend(&central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend((entries.len() as u16).to_le_bytes());
    out.extend((entries.len() as u16).to_le_bytes());
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(cd_off.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

/// The rate as sigrok writes it in `metadata`: `1 MHz`, `250 kHz`, `1234 Hz`.
fn rate_text(hz: f64) -> String {
    // [@ANCHOR: gla_rate_text]
    let r = hz.round() as u64;
    if r.is_multiple_of(1_000_000) {
        format!("{} MHz", r / 1_000_000)
    } else if r.is_multiple_of(1000) {
        format!("{} kHz", r / 1000)
    } else {
        format!("{r} Hz")
    }
}

/// The sigrok session file of a sampled capture: `names` are the probe names (one per pin), samples are packed little-endian, one
/// or two bytes per sample.
pub fn write_sr(s: &Sampled, names: &[String]) -> Vec<u8> {
    // [@ANCHOR: gla_write_sr]
    let unit = if names.len() > 8 { 2 } else { 1 };
    let mut meta = format!("[global]\nsigrok version=0.5.2\n\n[device 1]\ncapturefile=logic-1\ntotal probes={}\nsamplerate={}\ntotal analog=0\n", names.len(), rate_text(s.rate_hz));
    for (i, n) in names.iter().enumerate() {
        meta.push_str(&format!("probe{}={}\n", i + 1, n));
    }
    meta.push_str(&format!("unitsize={unit}\n"));
    let mut entries: Vec<(String, Vec<u8>)> = vec![
        ("version".into(), b"2".to_vec()),
        ("metadata".into(), meta.into_bytes()),
    ];
    for (n, chunk) in s.data.chunks(CHUNK_SAMPLES).enumerate() {
        let mut raw = Vec::with_capacity(chunk.len() * unit);
        for v in chunk {
            raw.extend(&v.to_le_bytes()[..unit]);
        }
        entries.push((format!("logic-1-{}", n + 1), raw));
    }
    let refs: Vec<(&str, Vec<u8>)> = entries
        .iter()
        .map(|(n, d)| (n.as_str(), d.clone()))
        .collect();
    zip_store(&refs)
}
