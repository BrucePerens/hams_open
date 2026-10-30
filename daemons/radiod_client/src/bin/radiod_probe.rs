// SPDX-License-Identifier: AGPL-3.0-or-later
//! Trial driver for `radiod_client`: `audio` creates an audio channel, decodes its RTP stream and
//! reports level, dominant tone and loss; `spectrum` creates a spectrum channel and measures the
//! response rate; `poll` shows one channel's status; `remove` deletes a channel.
//!
//! Receive only. Run with a loopback interface address and a group that `radiod` listens on.

use radiod_client::command::{
    AudioChannel, Command, Encoding, Mode, Policy, SpectrumChannel, Ssrc,
};
use radiod_client::net::{Control, NetError, RtpSocket};
use radiod_client::rtp::{AudioDecoder, Encoding as RtpEncoding, RtpPacket, SeqTracker};
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

type Args = std::collections::HashMap<String, String>;

fn parse_args() -> (String, Args) {
    let mut it = std::env::args().skip(1);
    let sub = it.next().unwrap_or_default();
    let mut m = Args::new();
    while let Some(k) = it.next() {
        if let Some(k) = k.strip_prefix("--") {
            let v = it.next().unwrap_or_default();
            m.insert(k.to_string(), v);
        }
    }
    (sub, m)
}

fn get<T: std::str::FromStr>(a: &Args, k: &str, d: T) -> T {
    a.get(k).and_then(|v| v.parse().ok()).unwrap_or(d)
}

fn dominant_hz(x: &[f32], rate: f32) -> (f32, f32) {
    // Goertzel scan, 10 Hz steps; adequate to confirm a tone.
    let n = x.len().min(8192);
    let x = &x[x.len() - n..];
    let (mut best_f, mut best_p) = (0.0f32, 0.0f32);
    let mut f = 20.0f32;
    while f < rate / 2.0 - 20.0 {
        let w = 2.0 * std::f32::consts::PI * f / rate;
        let c = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f32, 0.0f32);
        for v in x {
            let s = v + c * s1 - s2;
            s2 = s1;
            s1 = s;
        }
        let p = s1 * s1 + s2 * s2 - c * s1 * s2;
        if p > best_p {
            best_p = p;
            best_f = f;
        }
        f += 10.0;
    }
    (
        best_f,
        10.0 * (best_p / (n as f32 * n as f32 / 4.0)).max(1e-20).log10(),
    )
}

fn run() -> Result<(), NetError> {
    let (sub, a) = parse_args();
    let group: Ipv4Addr = get(&a, "group", Ipv4Addr::new(239, 1, 1, 1));
    let data: Ipv4Addr = get(&a, "data-group", Ipv4Addr::new(239, 1, 1, 2));
    let iface: Ipv4Addr = get(&a, "iface", Ipv4Addr::LOCALHOST);
    let ssrc = Ssrc::new(get(&a, "ssrc", 4000u32)).map_err(NetError::Command)?;
    let policy = Policy::default();
    let mut ctl = Control::open(group, iface, policy)?;
    let life: u32 = get(&a, "lifetime", 500u32);
    match sub.as_str() {
        "audio" => {
            let mode = match a.get("mode").map(String::as_str).unwrap_or("usb") {
                "lsb" => Mode::Lsb,
                "cw" => Mode::Cw,
                "am" => Mode::Am,
                "fm" => Mode::Fm,
                _ => Mode::Usb,
            };
            let encoding = match a.get("opus").and_then(|v| v.parse().ok()) {
                Some(bitrate) => Encoding::Opus { bitrate },
                None => Encoding::Pcm16Be,
            };
            let rtp = RtpSocket::open(data, iface)?;
            let cmd = Command::Audio(AudioChannel { ssrc, mode, frequency_hz: get(&a, "freq", 9_999_000.0), encoding, lifetime_frames: life });
            let t0 = Instant::now();
            let st = ctl.request(&cmd, Duration::from_secs(3))?;
            let Some(st) = st else {
                println!("no status reply within 3 s");
                return Ok(());
            };
            println!("reply after {:?}: {st:#?}", t0.elapsed());
            let enc = st.encoding.and_then(RtpEncoding::from_radiod).unwrap_or(RtpEncoding::S16Be);
            let mut dec = AudioDecoder::new(enc, st.channels.unwrap_or(1) as usize)
                .map_err(|e| NetError::Io(std::io::Error::other(e.to_string())))?;
            let rate = if enc == RtpEncoding::Opus { 48_000.0 } else { st.samprate.unwrap_or(12_000) as f32 };
            let (mut seq, mut samples, mut bytes, mut bad, mut other) = (SeqTracker::default(), Vec::<f32>::new(), 0u64, 0u64, 0u64);
            let mut buf = vec![0u8; 65_536];
            let secs: f64 = get(&a, "seconds", 5.0);
            let end = Instant::now() + Duration::from_secs_f64(secs);
            let mut first_pt = None;
            while Instant::now() < end {
                let Some(n) = rtp.recv(&mut buf, Duration::from_millis(100))? else { continue };
                match RtpPacket::parse(&buf[..n]) {
                    Ok(p) if p.ssrc == ssrc.get() => {
                        first_pt.get_or_insert(p.pt);
                        seq.push(p.seq);
                        bytes += n as u64;
                        match dec.decode(p.payload) {
                            Ok(mut s) => samples.append(&mut s),
                            Err(_) => bad += 1,
                        }
                    }
                    Ok(_) => other += 1,
                    Err(_) => bad += 1,
                }
            }
            let rms = (samples.iter().map(|v| v * v).sum::<f32>() / samples.len().max(1) as f32).sqrt();
            let (f, db) = dominant_hz(&samples, rate);
            println!(
                "rtp: pt {first_pt:?} packets {} lost {} ooo {} bad {bad} other-ssrc {other}, {:.1} kbit/s on the wire, {} samples at {rate} Hz, rms {rms:.4}, dominant {f} Hz ({db:.1} dB re full scale)",
                seq.received, seq.lost, seq.out_of_order, bytes as f64 * 8.0 / secs / 1000.0, samples.len()
            );
            if !a.contains_key("keep") {
                ctl.send(&Command::Remove(ssrc))?;
            }
        }
        "spectrum" => {
            let center = get(&a, "center", 10_000_000.0);
            let bin_count: u32 = get(&a, "bins", 1024);
            let bin_hz: f32 = get(&a, "bin-hz", 100.0);
            let avg: u32 = get(&a, "avg", 1);
            let frames: u32 = get(&a, "frames", 100);
            let cmd = Command::Spectrum(SpectrumChannel { ssrc, center_hz: center, bin_count, bin_hz, averaging: avg, lifetime_frames: life, byte_format: a.contains_key("bytes") });
            let mut got = ctl.request(&cmd, Duration::from_secs(5))?;
            if let Some(rate) = a.get("pipeline-hz").and_then(|v| v.parse::<f64>().ok()) {
                // Send polls at a fixed rate and count the distinct responses that come back.
                let period = Duration::from_secs_f64(1.0 / rate);
                let t0 = Instant::now();
                let (mut sent, mut recvd, mut sizes) = (0u32, 0u32, 0usize);
                let mut next = t0;
                let dur = Duration::from_secs_f64(get(&a, "max-seconds", 5.0));
                while t0.elapsed() < dur {
                    if Instant::now() >= next {
                        ctl.send(&Command::Poll { ssrc, lifetime_frames: life })?;
                        sent += 1;
                        next += period;
                    }
                    if let Some(s) = ctl.recv_status(Duration::from_millis(2))? {
                        if s.ssrc == ssrc.get() && (!s.bin_data.is_empty() || !s.bin_bytes.is_empty()) {
                            recvd += 1;
                            sizes = s.bin_data.len().max(s.bin_bytes.len());
                        }
                    }
                }
                println!("pipelined: sent {sent} polls at {rate} Hz, received {recvd} spectrum responses in {:.2} s = {:.1} per second, {sizes} bins", t0.elapsed().as_secs_f64(), f64::from(recvd) / t0.elapsed().as_secs_f64());
                if !a.contains_key("keep") {
                    ctl.send(&Command::Remove(ssrc))?;
                }
                return Ok(());
            }
            let first = got.clone();
            let t0 = Instant::now();
            let mut n = 0u32;
            let mut last = first.clone();
            while n < frames && t0.elapsed() < Duration::from_secs_f64(get(&a, "max-seconds", 30.0)) {
                // Poll and wait for the response; the spectrum channel answers each command once.
                let tag = ctl.send(&Command::Poll { ssrc, lifetime_frames: life })?;
                let until = Instant::now() + Duration::from_millis(1000);
                got = None;
                while Instant::now() < until {
                    if let Some(s) = ctl.recv_status(until.saturating_duration_since(Instant::now()))? {
                        if s.ssrc == ssrc.get() && s.tag == Some(u64::from(tag)) {
                            got = Some(s);
                            break;
                        }
                    }
                }
                match got.take() {
                    Some(s) => {
                        n += 1;
                        last = Some(s);
                    }
                    None => println!("poll {n}: no reply in 1 s"),
                }
            }
            let dt = t0.elapsed().as_secs_f64();
            println!("{n} responses in {dt:.3} s = {:.1} per second (serial request/response)", f64::from(n) / dt);
            if let Some(s) = last {
                let spec = s.spectrum_db(center);
                let peak = spec.iter().cloned().fold((0.0, f32::MIN), |m, x| if x.1 > m.1 { x } else { m });
                let mut lv: Vec<f32> = spec.iter().map(|x| x.1).collect();
                lv.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let med = lv.get(lv.len() / 2).copied().unwrap_or(0.0);
                println!(
                    "bins {} (asked {bin_count}) bin_hz {:?} fft_n {:?} avg {:?} noise_bw {:?}; span {:.0}..{:.0} Hz; peak {:.1} Hz at {:.1} dB, median {med:.1} dB",
                    s.bin_data.len().max(s.bin_bytes.len()), s.bin_hz, s.fft_n, s.averaging, s.noise_bw_bins,
                    spec.first().map(|x| x.0).unwrap_or(0.0), spec.last().map(|x| x.0).unwrap_or(0.0), peak.0, peak.1
                );
            }
            if !a.contains_key("keep") {
                ctl.send(&Command::Remove(ssrc))?;
            }
        }
        "poll" => {
            let tag = ctl.send(&Command::Poll { ssrc, lifetime_frames: life })?;
            let until = Instant::now() + Duration::from_secs(2);
            while Instant::now() < until {
                if let Some(s) = ctl.recv_status(until.saturating_duration_since(Instant::now()))? {
                    if s.ssrc == ssrc.get() && s.tag == Some(u64::from(tag)) {
                        println!("{s:#?}");
                        return Ok(());
                    }
                }
            }
            println!("no reply");
        }
        "remove" => {
            ctl.send(&Command::Remove(ssrc))?;
            println!("sent remove for {}", ssrc.get());
        }
        _ => println!("usage: radiod_probe audio|spectrum|poll|remove [--group G] [--data-group G] [--iface IP] [--ssrc N] [--freq HZ] [--mode usb|lsb|cw|am|fm] [--opus BPS] [--lifetime FRAMES] [--seconds S] [--center HZ --bins N --bin-hz HZ --avg N --frames N] [--keep 1]"),
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
