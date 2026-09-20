//! Emit wire frames as hex + our decoders' PCM for cross-checking with JMBE.
//! `jmbe_frames <imbe|a2> <wav> <nframes> <outprefix> <ber> <seed>`
use ham_digital_modes::ambe::float::mbe_synthesis::ErrorPolicy;
use std::io::Write;

struct Lcg(u64);
impl Lcg {
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}
fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }
fn pcm_out(path: &str, v: &[f64]) {
    let mut f = std::fs::File::create(path).unwrap();
    for s in v { f.write_all(&(s.round().clamp(-32768.0, 32767.0) as i16).to_le_bytes()).unwrap(); }
}
fn flip(bytes: &mut [u8], ber: f64, rng: &mut Lcg) {
    for b in bytes.iter_mut() { for bit in 0..8 { if rng.unit() < ber { *b ^= 0x80 >> bit; } } }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (mode, wav, n, pre) = (a[1].as_str(), &a[2], a[3].parse::<usize>().unwrap(), &a[4]);
    let ber: f64 = a[5].parse().unwrap();
    let seed: u64 = a[6].parse().unwrap();
    let bytes = std::fs::read(wav).unwrap();
    let pcm: Vec<f64> = bytes[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f64).take(n * 160).collect();
    let mut rng = Lcg(seed);
    if mode == "imbe" {
        use ham_digital_modes::ambe::float::tia_102_baba::{decode::DecoderState, encoder::Encoder, interleave::*};
        let mut e = Encoder::new();
        e.push_samples(&pcm);
        let mut fr = Vec::new();
        while let Some(x) = e.next_frame() { fr.push(x) }
        fr.extend(e.finish());
        fr.truncate(n);
        let to_wire = |c: [u32; 8]| -> [u8; 18] {
            let s = interleave_to_dibit_symbols(c);
            let mut w = [0u8; 18];
            for (i, &(b1, b0)) in s.iter().enumerate() {
                if b1 { w[(2 * i) / 8] |= 0x80 >> ((2 * i) % 8); }
                if b0 { w[(2 * i + 1) / 8] |= 0x80 >> ((2 * i + 1) % 8); }
            }
            w
        };
        let from_wire = |w: &[u8; 18]| -> [u32; 8] {
            let bit = |k: usize| (w[k / 8] >> (7 - k % 8)) & 1 == 1;
            let s: [(bool, bool); 72] = std::array::from_fn(|i| (bit(2 * i), bit(2 * i + 1)));
            deinterleave_from_dibit_symbols(s)
        };
        let clean: Vec<[u8; 18]> = fr.iter().map(|&c| to_wire(c)).collect();
        let mut err = clean.clone();
        for w in err.iter_mut() { flip(w, ber, &mut rng); }
        let dec = |ws: &[[u8; 18]], fade: bool| { let mut d = DecoderState::new(); d.set_fade_concealment(fade);
            ws.iter().flat_map(|w| d.decode_frame(from_wire(w)).unwrap_or([0.0; 160])).collect::<Vec<f64>>() };
        assert!(clean.iter().zip(&fr).all(|(w, &c)| from_wire(w) == c), "wire roundtrip");
        std::fs::write(format!("{pre}.clean.hex"), clean.iter().map(|w| hex(w) + "\n").collect::<String>()).unwrap();
        std::fs::write(format!("{pre}.err.hex"), err.iter().map(|w| hex(w) + "\n").collect::<String>()).unwrap();
        pcm_out(&format!("{pre}.ours.clean.raw"), &dec(&clean, false));
        pcm_out(&format!("{pre}.ours.err.raw"), &dec(&err, false));
        pcm_out(&format!("{pre}.ours.errfade.raw"), &dec(&err, true));
    } else {
        #[cfg(feature = "ambe_plus_2")]
        {
            use ham_digital_modes::ambe::float::ambe_plus_2::{encoder::Encoder, interleave::*, synthesis::AmbePlus2SynthesisDecoder};
            let mut e = Encoder::new();
            e.push_samples(&pcm);
            let mut fr = Vec::new();
            while let Some(x) = e.next_frame() { fr.push(x) }
            fr.extend(e.finish());
            fr.truncate(n);
            let to_wire = |f: u128| -> [u8; 9] { let w = frame_to_interleaved(f); std::array::from_fn(|i| ((w >> (8 * (8 - i))) & 0xFF) as u8) };
            let from_wire = |b: &[u8; 9]| -> u128 { let mut w = 0u128; for &x in b { w = (w << 8) | x as u128; } interleaved_to_frame(w) };
            let clean: Vec<[u8; 9]> = fr.iter().map(|&f| to_wire(f)).collect();
            let mut err = clean.clone();
            for w in err.iter_mut() { flip(w, ber, &mut rng); }
            let dec = |ws: &[[u8; 9]], p: ErrorPolicy| { let mut d = AmbePlus2SynthesisDecoder::new().with_error_policy(p);
                ws.iter().flat_map(|w| d.decode_frame(from_wire(w)).unwrap_or([0.0; 160])).collect::<Vec<f64>>() };
            std::fs::write(format!("{pre}.clean.hex"), clean.iter().map(|w| hex(w) + "\n").collect::<String>()).unwrap();
            std::fs::write(format!("{pre}.err.hex"), err.iter().map(|w| hex(w) + "\n").collect::<String>()).unwrap();
            pcm_out(&format!("{pre}.ours.clean.raw"), &dec(&clean, ErrorPolicy::Concealing));
            pcm_out(&format!("{pre}.ours.err.raw"), &dec(&err, ErrorPolicy::Concealing));
            pcm_out(&format!("{pre}.ours.errclean.raw"), &dec(&err, ErrorPolicy::Clean));
        }
        #[cfg(not(feature = "ambe_plus_2"))]
        panic!("build with --features ambe_plus_2");
    }
}
