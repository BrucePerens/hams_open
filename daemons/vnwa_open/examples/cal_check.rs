// SPDX-License-Identifier: AGPL-3.0-or-later
//! `cal_check <start_hz> <stop_hz> <points> <cal_file>`: sweeps the cabling as a through, builds and saves a through calibration, loads
//! it back, then sweeps again at full level and at half the RF level (6 dB lower) and prints the worst deviation of
//! the corrected S21 from 1 at 0 degrees. Repeatability and linearity of the whole path.
use vnwa_open::cal::Calibration;
use vnwa_open::sweep::{Config, Vna};

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let num = |i: usize| -> Result<f64, String> {
        a.get(i)
            .and_then(|s| s.parse().ok())
            .ok_or(format!("argument {i} must be a number"))
    };
    let (start, stop, n) = (num(1)?, num(2)?, num(3)? as usize);
    let file = a
        .get(4)
        .ok_or("usage: cal_check <start> <stop> <points> <file>")?;
    let cfg = Config {
        if_hz: 4000.0,
        ..Config::default()
    };
    let mut vna = Vna::open(cfg.clone())?;
    let thru = vna.sweep(start, stop, n)?;
    drop(vna); // the codecs can be opened once
    let freqs: Vec<f64> = thru.iter().map(|p| p.freq_hz).collect();
    let s21: Vec<_> = thru.iter().map(|p| p.s21).collect();
    let s11: Vec<_> = thru.iter().map(|p| p.s11).collect();
    std::fs::write(
        file,
        Calibration::new(freqs, None, None, None, Some(&s21))?.to_text(),
    )
    .map_err(|e| e.to_string())?;
    let cal = Calibration::from_text(&std::fs::read_to_string(file).map_err(|e| e.to_string())?)?;
    println!("S11 channel raw, for reference: first {:.4}", s11[0].abs());
    for (name, level) in [("full level", 0x3fffu16), ("-6 dB", 0x2000u16)] {
        let mut v = Vna::open(Config {
            rf_level: level,
            ..cfg.clone()
        })?;
        let (mut worst_mag, mut worst_deg) = (0.0f64, 0.0f64);
        for p in v.sweep(start, stop, n)? {
            let z = cal.correct(&p)?.s21.ok_or("no s21")?;
            if std::env::var("CAL_VERBOSE").is_ok() {
                eprintln!(
                    "{name} {:.0} Hz: {:+.3} % {:+.3} deg",
                    p.freq_hz,
                    (z.abs() - 1.0) * 100.0,
                    z.phase_deg()
                );
            }
            worst_mag = worst_mag.max((z.abs() - 1.0).abs() * 100.0);
            worst_deg = worst_deg.max(z.phase_deg().abs());
        }
        println!("{name}: corrected S21 worst {worst_mag:.3} % from 1, {worst_deg:.3} deg from 0");
    }
    Ok(())
}
