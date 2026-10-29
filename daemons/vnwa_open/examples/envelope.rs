// SPDX-License-Identifier: AGPL-3.0-or-later
//! `envelope <freq_hz> <count> [block_ms] [print]`: for `count` points measures, in the reference channels (codec 1 left, codec 2 right), where the tone at the IF
//! starts and where the 18 kHz burst (request 0x43) sits, relative to the end of the set-up send. With `print` it also prints the envelopes of the first points.
use vnwa_open::audio::tone_at_if;
use vnwa_open::sweep::{Config, Vna};
use vnwa_open::trace::onset_block as onset;

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let f: f64 = a
        .get(1)
        .and_then(|s| s.parse().ok())
        .ok_or("usage: envelope <freq_hz> <count> [block_ms] [print]")?;
    let count: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
    let block_ms: f64 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let print = a.iter().any(|s| s == "print");
    let cfg = Config {
        if_hz: 4000.0,
        ..Config::default()
    };
    let if_hz = cfg.if_hz;
    let vna = Vna::open(cfg)?;
    let b = (block_ms / 1000.0 * 48_000.0) as usize;
    let mut rows = Vec::new();
    for i in 0..count {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let r = match vna.trace_point(f, 20.0, 420.0) {
            Ok(r) => r,
            Err(e) => {
                println!("point {i}: {e}");
                continue;
            }
        };
        let env = |fr: &[[i16; 2]], ch: usize, hz: f64| -> Vec<f64> {
            fr.chunks_exact(b)
                .map(|c| {
                    tone_at_if(
                        &c.iter().map(|x| x[ch] as f64).collect::<Vec<_>>(),
                        48_000.0,
                        hz,
                    )
                    .abs()
                })
                .collect()
        };
        let send_end = r.send_end_frame as f64 / b as f64;
        let mut row = vec![r.send_ms];
        for (fr, ch) in [(&r.codec1, 0usize), (&r.codec2, 1usize)] {
            let (tone, burst) = (env(fr, ch, if_hz), env(fr, ch, 18_000.0));
            let rel = |x: Option<usize>| {
                x.map(|k| (k as f64 - send_end) * block_ms)
                    .unwrap_or(f64::NAN)
            };
            row.push(rel(onset(&tone)));
            row.push(rel(onset(&burst)));
            if print && i < 2 {
                let fmt = |v: &[f64]| {
                    v.iter()
                        .map(|x| format!("{x:.0}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                };
                println!("point {i} codec {} (blocks of {block_ms} ms from 20 ms before the send; send ends at block {send_end:.0})\n  tone at IF: {}\n  18 kHz:     {}", ch + 1, fmt(&tone[..tone.len().min(140)]), fmt(&burst[..burst.len().min(140)]));
            }
        }
        rows.push(row);
    }
    let stat = |name: &str, col: usize| {
        let mut v: Vec<f64> = rows
            .iter()
            .map(|r| r[col])
            .filter(|x| x.is_finite())
            .collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if v.is_empty() {
            println!("{name}: none found");
            return;
        }
        let q = |p: f64| v[((v.len() - 1) as f64 * p) as usize];
        println!(
            "{name}: n {} min {:.1} median {:.1} 90% {:.1} max {:.1} ms",
            v.len(),
            v[0],
            q(0.5),
            q(0.9),
            v[v.len() - 1]
        );
    };
    println!("onsets relative to the end of the send (ms):");
    stat("send duration", 0);
    stat("codec 1 IF tone onset", 1);
    stat("codec 1 18 kHz onset", 2);
    stat("codec 2 IF tone onset", 3);
    stat("codec 2 18 kHz onset", 4);
    println!("raw: send_ms,c1_tone,c1_18k,c2_tone,c2_18k");
    for r in &rows {
        println!(
            "{}",
            r.iter()
                .map(|x| format!("{x:.1}"))
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    Ok(())
}
