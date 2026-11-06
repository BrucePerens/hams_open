// SPDX-License-Identifier: AGPL-3.0-or-later
//! A fake `radiod` for the rx888 tests, on loopback multicast with TTL 0 (nothing leaves the host). It
//! answers as the RX-888's `radiod` did on 2026-11-05: a front-end status (SSRC 0) without a command
//! tag; one status per channel command echoing the tag, with the channel's frequency, passband, gain,
//! levels, a rising input sample counter and its stream format; float spectrum bins (DC first, then
//! positive, then negative frequencies) with one tone; and 16-bit big-endian PCM RTP every 20 ms for
//! each audio channel. Every command it receives is recorded.

use radiod_client::tlv::{parse, ty, Kind};
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The baseband power and noise density the fake reports, and the tone's level in the spectrum.
pub const BASEBAND_DB: f32 = -127.1;
pub const NOISE_DENSITY_DB: f32 = -161.4;
pub const TONE_DB: f32 = -90.0;
pub const FLOOR_DB: f32 = -122.7;

/// One command the fake received.
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub ssrc: u32,
    pub lifetime: Option<u64>,
    pub demod: Option<u64>,
}

#[derive(Clone, Debug, Default)]
struct Chan {
    freq: f64,
    lo: f32,
    hi: f32,
    channels: u64,
    agc: bool,
    gain: f32,
    demod: u64,
    bins: u64,
    bin_hz: f32,
}

pub struct FakeRadiod {
    pub status_group: Ipv4Addr,
    pub data_group: Ipv4Addr,
    pub status_port: u16,
    pub data_port: u16,
    pub seen: Arc<Mutex<Vec<Seen>>>,
    stop: Arc<AtomicBool>,
}

impl Drop for FakeRadiod {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn join(group: Ipv4Addr, port: u16) -> UdpSocket {
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).unwrap();
    s.set_reuse_address(true).unwrap();
    s.set_reuse_port(true).unwrap();
    s.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port).into())
        .unwrap();
    s.join_multicast_v4(&group, &Ipv4Addr::LOCALHOST).unwrap();
    s.set_multicast_if_v4(&Ipv4Addr::LOCALHOST).unwrap();
    s.set_multicast_ttl_v4(0).unwrap();
    s.set_multicast_loop_v4(true).unwrap();
    s.set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    s.into()
}

fn tlv(out: &mut Vec<u8>, t: u8, v: &[u8]) {
    out.push(t);
    if v.len() < 128 {
        out.push(v.len() as u8);
    } else {
        out.push(0x84);
        out.extend_from_slice(&(v.len() as u32).to_be_bytes());
    }
    out.extend_from_slice(v);
}

/// Unique groups and ports per fake, so tests in one process and in parallel processes never meet.
fn next_slot() -> (u8, u16) {
    static N: AtomicU16 = AtomicU16::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let port = 40_000 + ((std::process::id() % 2000) as u16) * 8 + (n % 4) * 2;
    ((n % 200) as u8 + 1, port)
}

impl FakeRadiod {
    pub fn start() -> FakeRadiod {
        let (g, port) = next_slot();
        let status_group = Ipv4Addr::new(239, 255, 89, g);
        let data_group = Ipv4Addr::new(239, 255, 90, g);
        let (status_port, data_port) = (port, port + 1);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let chans: Arc<Mutex<HashMap<u32, Chan>>> = Arc::new(Mutex::new(HashMap::new()));
        let started = Instant::now();
        {
            let (seen, stop, chans) = (seen.clone(), stop.clone(), chans.clone());
            let sock = join(status_group, status_port);
            let dest = SocketAddrV4::new(status_group, status_port);
            std::thread::spawn(move || {
                let mut buf = vec![0u8; 65_536];
                while !stop.load(Ordering::Relaxed) {
                    let Ok(n) = sock.recv(&mut buf) else { continue };
                    let Ok((Kind::Command, e)) = parse(&buf[..n]) else {
                        continue;
                    };
                    let find = |t: u8| e.iter().find(|x| x.ty == t);
                    let uint = |t: u8| find(t).and_then(|x| x.uint().ok());
                    let ssrc = uint(ty::OUTPUT_SSRC).unwrap_or(0) as u32;
                    let tag = uint(ty::COMMAND_TAG).unwrap_or(0) as u32;
                    let lifetime = uint(ty::LIFETIME);
                    seen.lock().unwrap().push(Seen {
                        ssrc,
                        lifetime,
                        demod: uint(ty::DEMOD_TYPE),
                    });
                    let mut r = vec![0u8];
                    if ssrc == 0 {
                        tlv(&mut r, ty::OUTPUT_SSRC, &[]);
                        tlv(&mut r, ty::DESCRIPTION, b"fake rx888");
                        tlv(&mut r, ty::INPUT_SAMPRATE, &64_800_000u32.to_be_bytes());
                        tlv(&mut r, ty::FE_ISREAL, &[1]);
                        tlv(&mut r, ty::AD_BITS_PER_SAMPLE, &[16]);
                        tlv(&mut r, ty::AD_OVER, &[3]);
                        tlv(&mut r, ty::RF_GAIN, &34.0f32.to_be_bytes());
                        tlv(&mut r, ty::RF_ATTEN, &[]);
                        tlv(&mut r, ty::RF_AGC, &[1]);
                        tlv(&mut r, ty::IF_POWER, &(-52.5f32).to_be_bytes());
                    } else {
                        let mut map = chans.lock().unwrap();
                        if lifetime == Some(1) {
                            map.remove(&ssrc);
                        }
                        let c = map.entry(ssrc).or_insert_with(|| Chan {
                            agc: true,
                            ..Chan::default()
                        });
                        if let Some(f) = find(ty::RADIO_FREQUENCY).and_then(|x| x.f64().ok()) {
                            c.freq = f;
                        }
                        if let Some(v) = find(ty::LOW_EDGE).and_then(|x| x.f32().ok()) {
                            c.lo = v;
                        }
                        if let Some(v) = find(ty::HIGH_EDGE).and_then(|x| x.f32().ok()) {
                            c.hi = v;
                        }
                        if let Some(v) = uint(ty::OUTPUT_CHANNELS) {
                            c.channels = v;
                        }
                        if let Some(v) = uint(ty::DEMOD_TYPE) {
                            c.demod = v;
                        }
                        if let Some(v) = find(ty::AGC_ENABLE).and_then(|x| x.bool().ok()) {
                            c.agc = v;
                        }
                        if let Some(v) = find(ty::GAIN).and_then(|x| x.f32().ok()) {
                            c.gain = v;
                        }
                        if let Some(v) = uint(ty::BIN_COUNT) {
                            c.bins = v;
                        }
                        if let Some(v) = find(ty::RESOLUTION_BW).and_then(|x| x.f32().ok()) {
                            c.bin_hz = v;
                        }
                        let c = c.clone();
                        if lifetime == Some(1) {
                            map.remove(&ssrc);
                        }
                        drop(map);
                        let input =
                            (started.elapsed().as_secs_f64() / 0.02).floor() as u64 * 1_296_000;
                        tlv(&mut r, ty::OUTPUT_SSRC, &ssrc.to_be_bytes());
                        tlv(&mut r, ty::COMMAND_TAG, &tag.to_be_bytes());
                        tlv(&mut r, ty::RADIO_FREQUENCY, &c.freq.to_bits().to_be_bytes());
                        tlv(&mut r, ty::INPUT_SAMPLES, &input.to_be_bytes());
                        tlv(&mut r, ty::DEMOD_TYPE, &[c.demod as u8]);
                        if c.demod == 3 {
                            let n = c.bins as usize;
                            let floor = 10f32.powf(FLOOR_DB / 10.0);
                            let mut bins = vec![floor; n];
                            // A tone 1 MHz above the centre (positive frequencies follow DC).
                            let k = (1.0e6 / f64::from(c.bin_hz)).round() as usize;
                            if k < n / 2 {
                                bins[k] = 10f32.powf(TONE_DB / 10.0);
                            }
                            let mut v = Vec::with_capacity(n * 4);
                            for b in bins {
                                v.extend_from_slice(&b.to_be_bytes());
                            }
                            tlv(&mut r, ty::BIN_COUNT, &(n as u32).to_be_bytes());
                            tlv(&mut r, ty::RESOLUTION_BW, &c.bin_hz.to_be_bytes());
                            tlv(&mut r, ty::NOISE_BW, &(c.bin_hz * 1.5).to_be_bytes());
                            tlv(&mut r, ty::BIN_DATA, &v);
                        } else {
                            tlv(&mut r, ty::LOW_EDGE, &c.lo.to_be_bytes());
                            tlv(&mut r, ty::HIGH_EDGE, &c.hi.to_be_bytes());
                            tlv(&mut r, ty::AGC_ENABLE, &[u8::from(c.agc)]);
                            tlv(
                                &mut r,
                                ty::GAIN,
                                &(if c.agc { 61.2f32 } else { c.gain }).to_be_bytes(),
                            );
                            tlv(
                                &mut r,
                                ty::OUTPUT_LEVEL,
                                &(BASEBAND_DB + if c.agc { 61.2 } else { c.gain }).to_be_bytes(),
                            );
                            tlv(&mut r, ty::BASEBAND_POWER, &BASEBAND_DB.to_be_bytes());
                            tlv(&mut r, ty::NOISE_DENSITY, &NOISE_DENSITY_DB.to_be_bytes());
                            tlv(
                                &mut r,
                                ty::RTP_PT,
                                &[if c.channels == 2 { 123 } else { 122 }],
                            );
                            tlv(&mut r, ty::OUTPUT_ENCODING, &[2]);
                            tlv(&mut r, ty::OUTPUT_CHANNELS, &[c.channels as u8]);
                            tlv(&mut r, ty::OUTPUT_SAMPRATE, &12_000u32.to_be_bytes());
                        }
                    }
                    r.push(ty::EOL);
                    let _ = sock.send_to(&r, dest);
                }
            });
        }
        {
            let (stop, chans) = (stop.clone(), chans.clone());
            let sock = join(data_group, data_port);
            let dest = SocketAddrV4::new(data_group, data_port);
            std::thread::spawn(move || {
                let mut seq: u16 = 0;
                while !stop.load(Ordering::Relaxed) {
                    let list: Vec<(u32, Chan)> = chans
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|(k, v)| (*k, v.clone()))
                        .collect();
                    for (ssrc, c) in list {
                        if c.demod == 3 || c.channels == 0 {
                            continue;
                        }
                        let mut p = vec![0x80, if c.channels == 2 { 123 } else { 122 }];
                        p.extend_from_slice(&seq.to_be_bytes());
                        p.extend_from_slice(&(u32::from(seq) * 240).to_be_bytes());
                        p.extend_from_slice(&ssrc.to_be_bytes());
                        for i in 0..240 * c.channels as usize {
                            let v = (1000.0 * (i as f32 * 0.7).sin()) as i16;
                            p.extend_from_slice(&v.to_be_bytes());
                        }
                        let _ = sock.send_to(&p, dest);
                    }
                    seq = seq.wrapping_add(1);
                    std::thread::sleep(Duration::from_millis(20));
                }
            });
        }
        FakeRadiod {
            status_group,
            data_group,
            status_port,
            data_port,
            seen,
            stop,
        }
    }

    /// The instrument entry pointing at this fake.
    pub fn entry(&self, tier: &str) -> String {
        format!(
            r#""driver":"rx888","tier":"{tier}","status_group":"{}","data_group":"{}","status_port":{},"data_port":{}"#,
            self.status_group, self.data_group, self.status_port, self.data_port
        )
    }
}
