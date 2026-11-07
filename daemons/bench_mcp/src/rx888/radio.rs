// SPDX-License-Identifier: AGPL-3.0-or-later
//! The RX-888 tools that open a `radiod` channel: tune (level, noise, SNR), spectrum and capture.
//!
//! Each call opens its own channel on a fixed SSRC of this instrument (`ssrc_base` + 0, 1, 2), always
//! with a lifetime (`radiod_client` cannot send a command without one), and removes it before it
//! returns. A channel left behind by a server that died expires by itself within its lifetime.

use super::{control, front_end, number, runtime, Config};
use crate::config::Instrument;
use radiod_client::command::{
    max_spectrum_span_hz, AudioChannel, Command, Encoding, Mode, SpectrumChannel, Ssrc,
    GAIN_RANGE_DB,
};
use radiod_client::net::{Control, RtpSocket};
use radiod_client::rtp::{RtpPacket, SeqTracker};
use radiod_client::status::ChannelStatus;
use serde_json::{json, Value};
use std::io::Write;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Lifetime of every channel these tools open: 5 s (250 frames of 20 ms); each read renews it.
const LIFETIME: u32 = 250;
/// Acknowledgement wait for one command on a running channel.
const ACK: Duration = Duration::from_secs(1);
/// Acknowledgement wait for the command that opens a channel: the first channel after `radiod` starts
/// starts the front end, and with no FFTW wisdom file the 1.62M-point input FFT is planned first
/// (over 1 s on bruce16, 2026-11-05).
const OPEN_ACK: Duration = Duration::from_secs(10);
/// The first spectrum response of a new geometry can take seconds while FFTW plans (no wisdom file).
const SPECTRUM_FIRST_WAIT: Duration = Duration::from_secs(10);
/// `radiod`'s block time: its input sample counter advances a block at a time.
const BLOCK_S: f64 = 0.020;
/// I/Q capture aims the noise at this level when no gain is given.
const IQ_TARGET_DBFS: f64 = -30.0;

/// The tune channel's, the spectrum channel's and the capture channel's SSRC.
fn ssrc(cfg: &Config, which: u32) -> Result<Ssrc, String> {
    // [@ANCHOR: bench_mcp_rx888_ssrc]
    Ssrc::new(cfg.ssrc_base + which).map_err(|e| format!("ssrc: {e}"))
}

/// The mode argument (`usb`, `lsb`, `cw`, `am`, `fm`), `default` when absent.
pub(crate) fn mode_arg(args: &Value, default: Mode) -> Result<Mode, String> {
    // [@ANCHOR: bench_mcp_rx888_mode_arg]
    match args.get("mode").and_then(|m| m.as_str()) {
        None => Ok(default),
        Some("usb") => Ok(Mode::Usb),
        Some("lsb") => Ok(Mode::Lsb),
        Some("cw") => Ok(Mode::Cw),
        Some("am") => Ok(Mode::Am),
        Some("fm") => Ok(Mode::Fm),
        Some(o) => Err(format!("mode {o:?} is not usb, lsb, cw, am or fm")),
    }
}

/// Where a passband of `width` sits for a mode, Hz from the carrier (as radioctl's radiod channel).
pub(crate) fn edges(mode: Mode, width: f64) -> (f32, f32) {
    // [@ANCHOR: bench_mcp_rx888_edges]
    let w = width as f32;
    match mode {
        Mode::Usb => (50.0, 50.0 + w),
        Mode::Lsb => (-50.0 - w, -50.0),
        Mode::Cw => (600.0 - w / 2.0, 600.0 + w / 2.0),
        Mode::Am | Mode::Fm | Mode::Iq | Mode::Sam => (-w / 2.0, w / 2.0),
        // Synchronous single sideband keeps 100 Hz across the carrier (radioctl's radiod channel).
        Mode::SamLower => (100.0 - w.max(200.0), 100.0),
        Mode::SamUpper => (-100.0, w.max(200.0) - 100.0),
    }
}

/// Sends `cmd` and returns `radiod`'s acknowledging status.
fn ask(
    rt: &tokio::runtime::Runtime,
    c: &mut Control,
    cmd: &Command,
    wait: Duration,
) -> Result<ChannelStatus, String> {
    // [@ANCHOR: bench_mcp_rx888_ask]
    rt.block_on(c.request(cmd, wait))
        .map_err(|e| format!("radiod command failed: {e}"))?
        .ok_or_else(|| {
            format!("radiod did not acknowledge within {wait:?}. Next: bench_rx888_status.")
        })
}

/// Removes a channel (best effort; it expires by its lifetime anyway).
fn remove(rt: &tokio::runtime::Runtime, c: &mut Control, s: Ssrc) {
    // [@ANCHOR: bench_mcp_rx888_remove]
    let _ = rt.block_on(c.request(&Command::Remove(s), ACK));
}

/// SNR in dB from a baseband power (signal plus noise in the passband) and the noise in the same band:
/// `10 log10(P/N - 1)`, floored at -30 dB when the power is at or below the noise.
pub(crate) fn snr_db(baseband_db: f64, noise_in_band_db: f64) -> f64 {
    // [@ANCHOR: bench_mcp_rx888_snr_db]
    let ratio = 10f64.powf((baseband_db - noise_in_band_db) / 10.0) - 1.0;
    if ratio <= 1e-3 {
        -30.0
    } else {
        10.0 * ratio.log10()
    }
}

pub fn tune(inst: &Instrument, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_rx888_tune]
    let cfg = &inst.rx888;
    let freq = number(args, "freq_hz", None, 10_000.0, 30_000_000.0)?;
    let mode = mode_arg(args, Mode::Am)?;
    let width = match args.get("bandwidth_hz") {
        None | Some(Value::Null) => None,
        Some(_) => Some(number(args, "bandwidth_hz", None, 100.0, 24_000.0)?),
    };
    let readings = number(args, "readings", Some(10.0), 1.0, 50.0)? as usize;
    let rt = runtime()?;
    let mut c = control(cfg, &rt)?;
    let s = ssrc(cfg, 0)?;
    let open = Command::Audio(AudioChannel {
        ssrc: s,
        mode,
        frequency_hz: freq,
        encoding: Encoding::Pcm16Be,
        lifetime_frames: LIFETIME,
    });
    let result = (|| {
        ask(&rt, &mut c, &open, OPEN_ACK)?;
        if let Some(w) = width {
            let (low_hz, high_hz) = edges(mode, w);
            ask(
                &rt,
                &mut c,
                &Command::Filter {
                    ssrc: s,
                    low_hz,
                    high_hz,
                    lifetime_frames: LIFETIME,
                },
                ACK,
            )?;
        }
        std::thread::sleep(Duration::from_millis(500));
        let poll = Command::Poll {
            ssrc: s,
            lifetime_frames: LIFETIME,
        };
        let mut got: Vec<(Instant, ChannelStatus)> = Vec::new();
        for i in 0..readings {
            if i > 0 {
                std::thread::sleep(Duration::from_millis(100));
            }
            got.push((Instant::now(), ask(&rt, &mut c, &poll, ACK)?));
        }
        Ok::<_, String>(got)
    })();
    remove(&rt, &mut c, s);
    let got = result?;
    let mean = |f: &dyn Fn(&ChannelStatus) -> Option<f32>| {
        let v: Vec<f64> = got
            .iter()
            .filter_map(|(_, s)| f(s))
            .filter(|x| x.is_finite())
            .map(f64::from)
            .collect();
        (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
    };
    let last = &got[got.len() - 1].1;
    let bb = mean(&|s| s.baseband_power_db).ok_or("radiod sent no baseband power")?;
    let n0 = mean(&|s| s.noise_density_db).ok_or("radiod sent no noise density")?;
    let (lo, hi) = (
        last.low_edge_hz.unwrap_or(0.0),
        last.high_edge_hz.unwrap_or(0.0),
    );
    let bw = f64::from(hi - lo);
    let noise_in_band = if bw > 0.0 {
        n0 + 10.0 * bw.log10()
    } else {
        f64::NAN
    };
    let interval_s = match (got.first(), got.last()) {
        (Some((t0, _)), Some((t1, _))) => t1.duration_since(*t0).as_secs_f64(),
        _ => 0.0,
    };
    let rate = match (got.first(), got.last()) {
        (Some((_, a)), Some((_, b))) if interval_s > 0.0 => {
            match (a.input_samples, b.input_samples) {
                (Some(x), Some(y)) if y > x => Some((y - x) as f64 / interval_s),
                _ => None,
            }
        }
        _ => None,
    };
    Ok(json!({
        "frequency_hz": last.frequency_hz,
        "mode": format!("{mode:?}").to_lowercase(),
        "passband_hz": [lo, hi],
        "baseband_power_db": bb,
        "noise_density_db_per_hz": n0,
        "noise_in_passband_db": noise_in_band.is_finite().then_some(noise_in_band),
        "snr_db": noise_in_band.is_finite().then(|| snr_db(bb, noise_in_band)),
        "agc": last.agc,
        "channel_gain_db": last.gain_db,
        "output_level_dbfs": last.output_level_db,
        "readings": got.len(),
        "input_sample_rate_measured_sps": rate,
        "rate_interval_s": interval_s,
        "rate_resolution_fraction": (interval_s > 0.0).then(|| BLOCK_S / interval_s),
        "note": "levels are radiod's, dB re full scale referred to the input (the front end's RF gain removed), not dBm. snr_db -30 means no signal above the noise. The input counter advances in 20 ms blocks, so the measured rate is good to rate_resolution_fraction; more readings give a longer interval.",
    }))
}

/// A new file name in the output directory: `<stem>_<ms since the epoch>.<ext>`.
fn out_path(stem: &str, ext: &str) -> PathBuf {
    // [@ANCHOR: bench_mcp_rx888_out_path]
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    crate::out_dir().join(format!("{stem}_{ms}.{ext}"))
}

/// The strongest local maxima at least `above_db` over `floor`, strongest first, none within 2 bins of a stronger one.
pub(crate) fn peaks(
    bins: &[(f64, f32)],
    floor: f64,
    above_db: f64,
    count: usize,
) -> Vec<(f64, f64)> {
    // [@ANCHOR: bench_mcp_rx888_peaks]
    let mut cand: Vec<(usize, f64)> = (0..bins.len())
        .filter(|&i| {
            let v = bins[i].1;
            // A neighbour that is not a finite level (NaN) cannot hide a peak.
            let level = |x: f32| if x.is_finite() { x } else { f32::NEG_INFINITY };
            let left = i
                .checked_sub(1)
                .map_or(f32::NEG_INFINITY, |j| level(bins[j].1));
            let right = bins.get(i + 1).map_or(f32::NEG_INFINITY, |b| level(b.1));
            v.is_finite() && f64::from(v) >= floor + above_db && v >= left && v >= right
        })
        .map(|i| (i, f64::from(bins[i].1)))
        .collect();
    cand.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<(usize, f64)> = Vec::new();
    for (i, v) in cand {
        if out.len() >= count {
            break;
        }
        if out.iter().all(|(j, _)| i.abs_diff(*j) > 2) {
            out.push((i, v));
        }
    }
    out.into_iter().map(|(i, v)| (bins[i].0, v)).collect()
}

pub fn spectrum(inst: &Instrument, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_rx888_spectrum]
    let cfg = &inst.rx888;
    let center = number(args, "center_hz", Some(16_200_000.0), 0.0, 64_800_000.0)?;
    let span = number(args, "span_hz", Some(32_400_000.0), 1_000.0, 129_600_000.0)?;
    let bins = number(args, "bins", Some(1024.0), 64.0, 16_000.0)? as u32;
    let averaging = number(args, "averaging", Some(10.0), 1.0, 64.0)? as u32;
    let npeaks = number(args, "peaks", Some(10.0), 0.0, 50.0)? as usize;
    let rt = runtime()?;
    let fe = front_end(cfg, &rt, Duration::from_secs(2))?;
    let rate = fe
        .input_samprate
        .ok_or("the front end did not report its sample rate")?;
    let max_span = max_spectrum_span_hz(rate, fe.fe_is_real.unwrap_or(true));
    if span > max_span {
        return Err(format!("span_hz {span} is wider than the front end can fill ({max_span} Hz at {rate} S/s: half the bins would be zero)"));
    }
    let bin_hz = (span / f64::from(bins)) as f32;
    let mut c = control(cfg, &rt)?;
    let s = ssrc(cfg, 1)?;
    let cmd = Command::Spectrum(SpectrumChannel {
        ssrc: s,
        center_hz: center,
        bin_count: bins,
        bin_hz,
        averaging,
        lifetime_frames: LIFETIME,
        byte_format: false,
    });
    let result = (|| {
        ask(&rt, &mut c, &cmd, SPECTRUM_FIRST_WAIT)?;
        // A response is computed per command; the second one has the requested averaging behind it.
        std::thread::sleep(Duration::from_millis(300));
        let st = ask(&rt, &mut c, &cmd, SPECTRUM_FIRST_WAIT)?;
        Ok::<_, String>(st)
    })();
    remove(&rt, &mut c, s);
    let st = result?;
    let data = st.spectrum_db(center);
    if data.is_empty() {
        return Err("radiod returned no spectrum bins".into());
    }
    let zero = st.bin_data.iter().filter(|x| **x == 0.0).count();
    let mut levels: Vec<f64> = data
        .iter()
        .map(|(_, v)| f64::from(*v))
        .filter(|v| v.is_finite())
        .collect();
    levels.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let floor = levels[levels.len() / 2];
    let path = out_path(&format!("rx888_{}_spectrum", inst.name), "csv");
    let mut f = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    writeln!(f, "# RX-888 via radiod: center {center} Hz, span {span} Hz, {bins} bins of {bin_hz} Hz, {averaging} averaged; dB re full scale per bin, input-referred").map_err(|e| e.to_string())?;
    writeln!(f, "freq_hz,level_db").map_err(|e| e.to_string())?;
    for (hz, db) in &data {
        writeln!(f, "{hz:.1},{db:.2}").map_err(|e| e.to_string())?;
    }
    Ok(json!({
        "csv": path.display().to_string(),
        "center_hz": center, "span_hz": span, "bins": data.len(), "bin_hz": bin_hz,
        "noise_bandwidth_hz": st.noise_bw_bins, "fft_n": st.fft_n, "averaging": averaging,
        "floor_db": floor, "min_db": levels.first(), "max_db": levels.last(),
        "zero_bins": zero,
        "peaks": peaks(&data, floor, 10.0, npeaks).iter().map(|(hz, db)| json!({"freq_hz": hz, "level_db": db, "above_floor_db": db - floor})).collect::<Vec<_>>(),
        "front_end": {"ad_overrange_count": fe.ad_over, "rf_gain_db": fe.rf_gain_db.filter(|g| g.is_finite())},
    }))
}

/// A 16-bit PCM WAV header for `frames` frames of `channels` channels at `rate` Hz.
pub(crate) fn wav_header(rate: u32, channels: u16, frames: u32) -> Vec<u8> {
    // [@ANCHOR: bench_mcp_rx888_wav_header]
    let data = frames * u32::from(channels) * 2;
    let mut h = Vec::with_capacity(44);
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&(36 + data).to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes());
    h.extend_from_slice(&channels.to_le_bytes());
    h.extend_from_slice(&rate.to_le_bytes());
    h.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    h.extend_from_slice(&(channels * 2).to_le_bytes());
    h.extend_from_slice(&16u16.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&data.to_le_bytes());
    h
}

pub fn capture(inst: &Instrument, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_rx888_capture]
    let cfg = &inst.rx888;
    let freq = number(args, "freq_hz", None, 10_000.0, 30_000_000.0)?;
    let seconds = number(args, "seconds", Some(5.0), 0.1, 60.0)?;
    let iq = match args.get("kind").and_then(|k| k.as_str()) {
        None | Some("iq") => true,
        Some("audio") => false,
        Some(o) => return Err(format!("kind {o:?} is not iq or audio")),
    };
    let mode = if iq {
        Mode::Iq
    } else {
        mode_arg(args, Mode::Am)?
    };
    let gain = match args.get("gain_db") {
        None | Some(Value::Null) => None,
        Some(_) if !iq => return Err("gain_db applies to kind iq only (audio uses AGC)".into()),
        Some(_) => Some(number(
            args,
            "gain_db",
            None,
            f64::from(GAIN_RANGE_DB.0),
            f64::from(GAIN_RANGE_DB.1),
        )?),
    };
    let rt = runtime()?;
    let mut c = control(cfg, &rt)?;
    let rtp = RtpSocket::open_on_port(
        rt.handle(),
        cfg.data_group,
        cfg.data_port,
        Ipv4Addr::LOCALHOST,
    )
    .map_err(|e| {
        format!(
            "cannot join the data group {}:{}: {e}",
            cfg.data_group, cfg.data_port
        )
    })?;
    let s = ssrc(cfg, 2)?;
    let open = Command::Audio(AudioChannel {
        ssrc: s,
        mode,
        frequency_hz: freq,
        encoding: Encoding::Pcm16Be,
        lifetime_frames: LIFETIME,
    });
    let result = (|| {
        let mut st = ask(&rt, &mut c, &open, OPEN_ACK)?;
        let mut used_gain = None;
        if iq {
            std::thread::sleep(Duration::from_millis(400));
            let poll = Command::Poll {
                ssrc: s,
                lifetime_frames: LIFETIME,
            };
            let bb = ask(&rt, &mut c, &poll, ACK)?
                .baseband_power_db
                .filter(|b| b.is_finite());
            let g = match (gain, bb) {
                (Some(g), _) => g,
                (None, Some(bb)) => (IQ_TARGET_DBFS - f64::from(bb))
                    .clamp(f64::from(GAIN_RANGE_DB.0), f64::from(GAIN_RANGE_DB.1)),
                (None, None) => {
                    return Err(
                        "radiod sent no baseband power to set the gain from; give gain_db"
                            .to_string(),
                    )
                }
            };
            st = ask(
                &rt,
                &mut c,
                &Command::Gain {
                    ssrc: s,
                    gain_db: Some(g as f32),
                    lifetime_frames: LIFETIME,
                },
                ACK,
            )?;
            used_gain = Some(g);
        }
        let pt = st
            .rtp_pt
            .and_then(|p| u8::try_from(p).ok())
            .ok_or("radiod announced no payload type")?;
        let channels = st
            .channels
            .and_then(|c| u16::try_from(c).ok())
            .filter(|c| (1..=2).contains(c))
            .ok_or("radiod announced no channel count")?;
        let rate = st
            .samprate
            .and_then(|r| u32::try_from(r).ok())
            .filter(|r| *r > 0)
            .ok_or("radiod announced no sample rate")?;
        if st.encoding != Some(2) {
            return Err(format!(
                "radiod sends encoding {:?}, not 16-bit big-endian PCM",
                st.encoding
            ));
        }
        let want = (seconds * f64::from(rate)) as usize * usize::from(channels);
        let mut samples: Vec<i16> = Vec::with_capacity(want);
        let mut seq = SeqTracker::default();
        let mut buf = vec![0u8; 9000];
        let deadline = Instant::now() + Duration::from_secs_f64(seconds + 3.0);
        let mut last_renew = Instant::now();
        let mut skipped = 0u64;
        while samples.len() < want && Instant::now() < deadline {
            if last_renew.elapsed() > Duration::from_secs(1) {
                let poll = Command::Poll {
                    ssrc: s,
                    lifetime_frames: LIFETIME,
                };
                rt.block_on(c.send(&poll)).map_err(|e| e.to_string())?;
                last_renew = Instant::now();
            }
            let Some(n) = rt
                .block_on(rtp.recv(&mut buf, Duration::from_millis(200)))
                .map_err(|e| e.to_string())?
            else {
                continue;
            };
            let Ok(p) = RtpPacket::parse(&buf[..n]) else {
                continue;
            };
            if p.ssrc != s.get() {
                continue;
            }
            if p.pt != pt || p.payload.len() % (2 * usize::from(channels)) != 0 {
                skipped += 1;
                continue;
            }
            seq.push(p.seq);
            samples.extend(
                p.payload
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| i16::from_be_bytes(*b)),
            );
        }
        samples.truncate(want);
        Ok::<_, String>((samples, rate, channels, seq, used_gain, skipped))
    })();
    remove(&rt, &mut c, s);
    let (samples, rate, channels, seq, used_gain, skipped) = result?;
    if samples.is_empty() {
        return Err("no audio arrived from radiod for this channel. Next: bench_rx888_status; check that loopback multicast is on.".into());
    }
    let frames = (samples.len() / usize::from(channels)) as u32;
    let path = out_path(
        &format!("rx888_{}_{}", inst.name, if iq { "iq" } else { "audio" }),
        "wav",
    );
    let mut f = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(&wav_header(rate, channels, frames))
        .map_err(|e| e.to_string())?;
    let mut body = Vec::with_capacity(samples.len() * 2);
    for v in &samples {
        body.extend_from_slice(&v.to_le_bytes());
    }
    f.write_all(&body).map_err(|e| e.to_string())?;
    let rms =
        (samples.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / samples.len() as f64).sqrt();
    let peak = samples
        .iter()
        .map(|v| i32::from(*v).abs())
        .max()
        .unwrap_or(0);
    Ok(json!({
        "wav": path.display().to_string(),
        "kind": if iq { "iq" } else { "audio" },
        "mode": format!("{mode:?}").to_lowercase(),
        "frequency_hz": freq,
        "sample_rate_hz": rate, "channels": channels, "frames": frames,
        "seconds": f64::from(frames) / f64::from(rate),
        "rtp_packets": seq.received, "rtp_lost": seq.lost, "rtp_out_of_order": seq.out_of_order, "rtp_restarts": seq.restarts, "rtp_skipped": skipped,
        "gain_db": used_gain,
        "rms_dbfs": if rms > 0.0 { Some(20.0 * (rms / 32768.0).log10()) } else { None },
        "peak_counts": peak,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: bench_mcp_rx888_snr_db]
    #[test]
    fn snr_is_signal_over_noise_with_a_floor() {
        assert!((snr_db(-117.0, -120.0) - 10.0 * (10f64.powf(0.3) - 1.0).log10()).abs() < 1e-9);
        assert!((snr_db(-100.0, -120.0) - 19.957).abs() < 0.01);
        assert_eq!(snr_db(-120.0, -120.0), -30.0);
        assert_eq!(snr_db(-125.0, -120.0), -30.0);
    }

    // Tests [@ANCHOR: bench_mcp_rx888_peaks]
    #[test]
    fn peaks_are_local_maxima_above_the_floor_strongest_first_and_apart() {
        let bins: Vec<(f64, f32)> = [
            -120.0,
            -80.0,
            -79.0,
            -120.0,
            -120.0,
            -95.0,
            -120.0,
            -120.0,
            -60.0,
            f32::NAN,
        ]
        .iter()
        .enumerate()
        .map(|(i, v)| (i as f64 * 10.0, *v))
        .collect();
        assert_eq!(
            peaks(&bins, -120.0, 10.0, 5),
            vec![(80.0, -60.0), (20.0, -79.0), (50.0, -95.0)]
        );
        assert_eq!(peaks(&bins, -120.0, 10.0, 1), vec![(80.0, -60.0)]);
        assert!(peaks(&bins, -120.0, 70.0, 5).is_empty());
    }

    // Tests [@ANCHOR: bench_mcp_rx888_wav_header]
    #[test]
    fn wav_header_is_16_bit_pcm() {
        let h = wav_header(12_000, 2, 100);
        assert_eq!(h.len(), 44);
        assert_eq!(&h[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes([h[4], h[5], h[6], h[7]]), 36 + 400);
        assert_eq!(u16::from_le_bytes([h[22], h[23]]), 2);
        assert_eq!(u32::from_le_bytes([h[24], h[25], h[26], h[27]]), 12_000);
        assert_eq!(u32::from_le_bytes([h[28], h[29], h[30], h[31]]), 48_000);
        assert_eq!(u16::from_le_bytes([h[34], h[35]]), 16);
        assert_eq!(u32::from_le_bytes([h[40], h[41], h[42], h[43]]), 400);
    }

    // Tests [@ANCHOR: bench_mcp_rx888_edges]
    // Tests [@ANCHOR: bench_mcp_rx888_mode_arg]
    // Tests [@ANCHOR: bench_mcp_rx888_ssrc]
    // Tests [@ANCHOR: bench_mcp_rx888_out_path]
    #[test]
    fn modes_edges_ssrcs_and_paths() {
        assert_eq!(edges(Mode::Usb, 2400.0), (50.0, 2450.0));
        assert_eq!(edges(Mode::Lsb, 2400.0), (-2450.0, -50.0));
        assert_eq!(edges(Mode::Cw, 500.0), (350.0, 850.0));
        assert_eq!(edges(Mode::Am, 6000.0), (-3000.0, 3000.0));
        assert_eq!(edges(Mode::Sam, 6000.0), (-3000.0, 3000.0));
        assert_eq!(edges(Mode::SamUpper, 3000.0), (-100.0, 2900.0));
        assert_eq!(edges(Mode::SamLower, 3000.0), (-2900.0, 100.0));
        assert_eq!(mode_arg(&json!({}), Mode::Am), Ok(Mode::Am));
        assert_eq!(mode_arg(&json!({"mode": "lsb"}), Mode::Am), Ok(Mode::Lsb));
        assert!(mode_arg(&json!({"mode": "iq"}), Mode::Am).is_err());
        let cfg = Config::default();
        assert_eq!(ssrc(&cfg, 2).map(|s| s.get()), Ok(cfg.ssrc_base + 2));
        let p = out_path("x", "csv");
        assert!(p.starts_with(crate::out_dir()) && p.extension().is_some_and(|e| e == "csv"));
    }
}
