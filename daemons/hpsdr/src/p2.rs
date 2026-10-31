// SPDX-License-Identifier: AGPL-3.0-or-later
//! Protocol 2 wire format, sans-IO. Only the receive-relevant packets and the gated transmit packets.
//!
//! Source (facts only, `verified=unverified`): "openHPSDR Ethernet Protocol" v4.4 (c) 2019 Phil Harman
//! VK6PH, from TAPR/OpenHPSDR-Firmware (Protocol 2/Documentation), cited "P2" with the packet name.
//! All fields are big-endian (revision 1.3 fixed the order; one sentence of the hardware-to-host
//! section still says "little Endian", a typo this crate does not follow).
//!
//! Transmit safety: [`encode_high_priority_rx`] cannot set the PTT bit, drive or a transmit frequency;
//! [`encode_high_priority_tx`] and [`encode_duc_iq`] return [`HpsdrError::GateClosed`] unless the
//! caller's [`TxGate`] is open at that instant. The hardware itself drops to standby and deactivates PTT
//! if no command packet arrives for a second (P2, "Detailed operation"); send a high-priority packet
//! every 100 ms while running.
//!
//! Not implemented yet: a Protocol 2 session and emulator (the packets here are tested by round trip),
//! Erase/Program/Set-IP packets (firmware update is out of scope), wideband, microphone, DDC audio.

use crate::error::HpsdrError;
use crate::gate::TxGate;
use crate::iq::Complex;
use crate::types::{Board, HardwareStatus, Rate};
use std::time::Instant;

/// Fixed discovery/command port (P2, "Discovery Packet").
pub const PORT: u16 = 1024;
/// Length of the large control packets and of DDC/DUC data packets (P2: bytes 0..1443).
pub const BIG_LEN: usize = 1444;
/// Length of the discovery, general and status packets (60 bytes).
pub const SMALL_LEN: usize = 60;
/// DUC I/Q samples per packet at 24 bits (P2, "DUC I&Q Data Packet": 240 samples).
pub const DUC_SAMPLES: usize = 240;
/// DDCs the format can address.
pub const MAX_DDC: usize = 80;

/// The UDP ports of a session (P2, "General Packet to SDR", bytes 5..22). Zero means "use the default".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioPorts {
    /// Port the DDC-specific packet goes to (default 1025).
    pub ddc_specific: u16,
    /// DUC-specific (default 1026).
    pub duc_specific: u16,
    /// High priority from the PC (default 1027).
    pub hp_from_pc: u16,
    /// High priority status from the radio, source port (default 1025).
    pub hp_to_pc: u16,
    /// DDC audio (default 1028).
    pub ddc_audio: u16,
    /// DUC0 I/Q (default 1029).
    pub duc0_iq: u16,
    /// DDC0 I/Q source port; DDC n uses this plus n (default 1035).
    pub ddc0: u16,
    /// Microphone source port (default 1026).
    pub mic: u16,
    /// Wideband ADC0 source port (default 1027).
    pub wideband0: u16,
}

impl Default for RadioPorts {
    // [@ANCHOR: hpsdr_p2_ports_default]
    /// The default port map from the specification.
    fn default() -> Self {
        Self {
            ddc_specific: 1025,
            duc_specific: 1026,
            hp_from_pc: 1027,
            hp_to_pc: 1025,
            ddc_audio: 1028,
            duc0_iq: 1029,
            ddc0: 1035,
            mic: 1026,
            wideband0: 1027,
        }
    }
}

fn put16(p: &mut [u8], at: usize, v: u16) {
    p[at..at + 2].copy_from_slice(&v.to_be_bytes());
}

fn put32(p: &mut [u8], at: usize, v: u32) {
    p[at..at + 4].copy_from_slice(&v.to_be_bytes());
}

// [@ANCHOR: hpsdr_p2_discovery_packet]
/// Discovery request: sequence 0, command 0x02, zeros (P2, "Discovery Packet"). Send to port 1024.
pub fn discovery_packet() -> [u8; SMALL_LEN] {
    let mut p = [0u8; SMALL_LEN];
    p[4] = 0x02;
    p
}

/// A parsed Protocol 2 discovery reply (P2, "Discovery Reply Packet", option 1: board type below 254).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct P2Reply {
    /// Already connected to a host (byte 4 = 0x03).
    pub running: bool,
    /// MAC address (bytes 5..10).
    pub mac: [u8; 6],
    /// Board.
    pub board: Board,
    /// Protocol version implemented, decimal/10 (byte 12).
    pub protocol_version: u8,
    /// Firmware code version (byte 13).
    pub firmware: u8,
    /// Independent DDCs implemented (byte 20).
    pub num_ddcs: u8,
    /// Frequencies are sent as phase words (byte 21 = 1) rather than hertz.
    pub phase_word: bool,
    /// Supported endian/data-format bits (byte 22; 0 = big-endian 3-byte only).
    pub data_modes: u8,
}

// [@ANCHOR: hpsdr_p2_parse_reply]
/// Parse a discovery reply. Replies with board type 254/255 (XML or full description) are reported
/// as unsupported rather than guessed at.
pub fn parse_reply(d: &[u8]) -> Result<P2Reply, HpsdrError> {
    if d.len() < 24 {
        return Err(HpsdrError::Truncated);
    }
    if d[..4] != [0, 0, 0, 0] || !(d[4] == 2 || d[4] == 3) {
        return Err(HpsdrError::BadMagic);
    }
    if d[11] >= 254 {
        return Err(HpsdrError::Unsupported(
            "hardware description in the discovery reply".into(),
        ));
    }
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&d[5..11]);
    Ok(P2Reply {
        running: d[4] == 3,
        mac,
        board: Board::from_p2_type(d[11]),
        protocol_version: d[12],
        firmware: d[13],
        num_ddcs: d[20],
        phase_word: d[21] == 1,
        data_modes: d[22],
    })
}

// [@ANCHOR: hpsdr_p2_encode_reply]
/// Build a discovery reply (emulator side). The board type byte is the P2 number of `board_type`.
pub fn encode_reply(r: &P2Reply, board_type: u8) -> [u8; SMALL_LEN] {
    let mut p = [0u8; SMALL_LEN];
    p[4] = if r.running { 3 } else { 2 };
    p[5..11].copy_from_slice(&r.mac);
    p[11] = board_type;
    p[12] = r.protocol_version;
    p[13] = r.firmware;
    p[20] = r.num_ddcs;
    p[21] = u8::from(r.phase_word);
    p[22] = r.data_modes;
    p
}

// [@ANCHOR: hpsdr_p2_encode_general]
/// The General packet (command 0): the port map, the hardware reset timer enabled (byte 38 bit 0, so
/// a lost host puts the radio in standby), phase-word flag, big-endian 3-byte I/Q (byte 39 = 0). The PA
/// enable (byte 58) and Alex enables (byte 59) stay zero. Send it to port 1024 after discovery.
pub fn encode_general(seq: u32, ports: &RadioPorts, phase_words: bool) -> [u8; SMALL_LEN] {
    let mut p = [0u8; SMALL_LEN];
    put32(&mut p, 0, seq);
    put16(&mut p, 5, ports.ddc_specific);
    put16(&mut p, 7, ports.duc_specific);
    put16(&mut p, 9, ports.hp_from_pc);
    put16(&mut p, 11, ports.hp_to_pc);
    put16(&mut p, 13, ports.ddc_audio);
    put16(&mut p, 15, ports.duc0_iq);
    put16(&mut p, 17, ports.ddc0);
    put16(&mut p, 19, ports.mic);
    put16(&mut p, 21, ports.wideband0);
    p[37] = u8::from(phase_words) << 3;
    p[38] = 1;
    p
}

/// One DDC's settings (P2, "DDC Specific Packet").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DdcSetting {
    /// Send this DDC's data.
    pub enabled: bool,
    /// ADC index it listens to.
    pub adc: u8,
    /// Sample rate.
    pub rate: Rate,
}

// [@ANCHOR: hpsdr_p2_encode_ddc_specific]
/// The DDC Specific packet (to the DDC-specific port): number of ADCs, dither, random, per-DDC enable
/// bits (bytes 7..16), and per DDC from byte 17 in 6-byte groups: ADC, rate (ksps, 16 bits), two
/// reserved CIC bytes, sample size 24. Synchronous-DDC bytes stay zero (every DDC independent).
pub fn encode_ddc_specific(
    seq: u32,
    num_adc: u8,
    dither: u8,
    random: u8,
    ddcs: &[DdcSetting],
) -> Result<Vec<u8>, HpsdrError> {
    if ddcs.len() > MAX_DDC || num_adc == 0 || num_adc > 8 {
        return Err(HpsdrError::Config("DDC or ADC count".into()));
    }
    let mut p = vec![0u8; BIG_LEN];
    put32(&mut p, 0, seq);
    p[4] = num_adc;
    p[5] = dither;
    p[6] = random;
    for (n, d) in ddcs.iter().enumerate() {
        if d.enabled {
            p[7 + n / 8] |= 1 << (n % 8);
        }
        let at = 17 + 6 * n;
        p[at] = d.adc;
        put16(&mut p, at + 1, (d.rate.hz() / 1000) as u16);
        p[at + 5] = 24;
    }
    Ok(p)
}

// [@ANCHOR: hpsdr_p2_phase_word]
/// `2^32 * f / clock`, rounded (P2, "General Packet" byte 37 text).
pub fn phase_word(freq_hz: u32, clock_hz: f64) -> u32 {
    (4_294_967_296.0 * f64::from(freq_hz) / clock_hz).round() as u32
}

/// What the receive high-priority packet carries.
#[derive(Debug, Clone, PartialEq)]
pub struct HpRx {
    /// Set the run bit (byte 4 bit 0).
    pub run: bool,
    /// DDC frequencies in hertz (DDC 0 first).
    pub ddc_hz: Vec<u32>,
    /// Step attenuator before ADC0, 0..=31 dB (byte 1443).
    pub atten_adc0: u8,
    /// Step attenuator before ADC1, 0..=31 dB (byte 1442).
    pub atten_adc1: u8,
    /// `Some(clock_hz)` to send phase words (the radio's discovery reply said so), `None` for hertz.
    pub phase_clock_hz: Option<f64>,
}

fn hp_base(seq: u32, hp: &HpRx) -> Result<Vec<u8>, HpsdrError> {
    if hp.ddc_hz.len() > MAX_DDC || hp.atten_adc0 > 31 || hp.atten_adc1 > 31 {
        return Err(HpsdrError::Config("high priority settings".into()));
    }
    let mut p = vec![0u8; BIG_LEN];
    put32(&mut p, 0, seq);
    p[4] = u8::from(hp.run);
    for (n, f) in hp.ddc_hz.iter().enumerate() {
        put32(
            &mut p,
            9 + 4 * n,
            hp.phase_clock_hz.map_or(*f, |c| phase_word(*f, c)),
        );
    }
    p[1442] = hp.atten_adc1;
    p[1443] = hp.atten_adc0;
    Ok(p)
}

// [@ANCHOR: hpsdr_p2_encode_hp_rx]
/// A receive-only High Priority packet: run bit and DDC frequencies and attenuators. PTT (byte 4 bit
/// 1), the DUC frequency and the drive level are structurally zero; this function has no transmit
/// parameters.
pub fn encode_high_priority_rx(seq: u32, hp: &HpRx) -> Result<Vec<u8>, HpsdrError> {
    hp_base(seq, hp)
}

/// Transmit part of a High Priority packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TxState {
    /// Key the transmitter (byte 4 bit 1, "PTT0").
    pub ptt: bool,
    /// DUC0 frequency in hertz.
    pub duc_hz: u32,
    /// DUC0 drive level 0..=255 (byte 345).
    pub drive: u8,
}

// [@ANCHOR: hpsdr_p2_encode_hp_tx]
/// A High Priority packet that may key the transmitter. Returns [`HpsdrError::GateClosed`] and builds
/// nothing unless `gate` is open at `now`. With the gate open and `tx.ptt` false it equals the receive packet.
pub fn encode_high_priority_tx(
    seq: u32,
    hp: &HpRx,
    gate: &TxGate,
    now: Instant,
    tx: &TxState,
) -> Result<Vec<u8>, HpsdrError> {
    if !gate.is_open(now) {
        return Err(HpsdrError::GateClosed);
    }
    let mut p = hp_base(seq, hp)?;
    if tx.ptt {
        p[4] |= 2;
        put32(
            &mut p,
            329,
            hp.phase_clock_hz
                .map_or(tx.duc_hz, |c| phase_word(tx.duc_hz, c)),
        );
        p[345] = tx.drive;
    }
    Ok(p)
}

// [@ANCHOR: hpsdr_p2_encode_duc_iq]
/// A DUC I/Q packet (to the DUC0 I/Q port): 240 samples of 24-bit I then Q, big-endian (P2, "DUC I&Q
/// Data Packet"). Returns [`HpsdrError::GateClosed`] unless `gate` is open at `now`. A short slice is
/// padded with zeros (silence); a sample is never repeated. Samples are signed 24-bit values.
pub fn encode_duc_iq(
    seq: u32,
    iq: &[(i32, i32)],
    gate: &TxGate,
    now: Instant,
) -> Result<Vec<u8>, HpsdrError> {
    if !gate.is_open(now) {
        return Err(HpsdrError::GateClosed);
    }
    if iq.len() > DUC_SAMPLES {
        return Err(HpsdrError::Config("more than 240 samples".into()));
    }
    let mut p = vec![0u8; BIG_LEN];
    put32(&mut p, 0, seq);
    for (k, (i, q)) in iq.iter().enumerate() {
        p[4 + 6 * k..7 + 6 * k].copy_from_slice(&i.to_be_bytes()[1..4]);
        p[7 + 6 * k..10 + 6 * k].copy_from_slice(&q.to_be_bytes()[1..4]);
    }
    Ok(p)
}

// [@ANCHOR: hpsdr_p2_parse_hp_status]
/// Parse a High Priority Status packet (60 bytes, from the radio's status port): PTT/dot/dash and PLL
/// bits (byte 4), ADC overload bits (5), exciter (6-7), PA forward (14-15) and reverse (22-23) power,
/// ADC peak magnitudes (39-42) and supply (49-50). Returns the status and the sequence number.
pub fn parse_hp_status(d: &[u8]) -> Result<(u32, HardwareStatus), HpsdrError> {
    if d.len() < SMALL_LEN {
        return Err(HpsdrError::Truncated);
    }
    let w = |at: usize| u16::from_be_bytes([d[at], d[at + 1]]);
    let mut st = HardwareStatus {
        ptt_input: d[4] & 1 != 0,
        dot_input: d[4] & 2 != 0,
        dash_input: d[4] & 4 != 0,
        pll_locked: Some(d[4] & 0x10 != 0),
        exciter_raw: w(6),
        forward_raw: w(14),
        reverse_raw: w(22),
        supply_raw: w(49),
        adc_max_magnitude: [w(39) & 0x7fff, w(41) & 0x7fff],
        ..Default::default()
    };
    for (i, o) in st.adc_overload.iter_mut().enumerate() {
        *o = d[5] & (1 << i) != 0;
    }
    Ok((u32::from_be_bytes([d[0], d[1], d[2], d[3]]), st))
}

// [@ANCHOR: hpsdr_p2_encode_hp_status]
/// Build a High Priority Status packet (emulator side).
pub fn encode_hp_status(seq: u32, st: &HardwareStatus) -> [u8; SMALL_LEN] {
    let mut p = [0u8; SMALL_LEN];
    put32(&mut p, 0, seq);
    p[4] = u8::from(st.ptt_input)
        | (u8::from(st.dot_input) << 1)
        | (u8::from(st.dash_input) << 2)
        | (u8::from(st.pll_locked == Some(true)) << 4);
    for (i, o) in st.adc_overload.iter().enumerate() {
        p[5] |= u8::from(*o) << i;
    }
    put16(&mut p, 6, st.exciter_raw);
    put16(&mut p, 14, st.forward_raw);
    put16(&mut p, 22, st.reverse_raw);
    put16(&mut p, 39, st.adc_max_magnitude[0]);
    put16(&mut p, 41, st.adc_max_magnitude[1]);
    put16(&mut p, 49, st.supply_raw);
    p
}

/// One DDC data packet (P2, "DDC Packet", single DDC, 24 bits).
#[derive(Debug, Clone, PartialEq)]
pub struct DdcPacket {
    /// Sequence number (per DDC port).
    pub seq: u32,
    /// 64-bit sample-count timestamp.
    pub timestamp: u64,
    /// Samples.
    pub samples: Vec<Complex>,
}

// [@ANCHOR: hpsdr_p2_parse_ddc]
/// Parse a DDC packet: seq, 64-bit timestamp, bits per sample (must be 24), sample count, then I/Q.
pub fn parse_ddc_packet(d: &[u8]) -> Result<DdcPacket, HpsdrError> {
    if d.len() < 16 {
        return Err(HpsdrError::Truncated);
    }
    let bits = u16::from_be_bytes([d[12], d[13]]);
    if bits != 24 {
        return Err(HpsdrError::Unsupported(format!("{bits} bits per sample")));
    }
    let n = usize::from(u16::from_be_bytes([d[14], d[15]]));
    if d.len() < 16 + 6 * n {
        return Err(HpsdrError::Truncated);
    }
    let s24 = |b: &[u8]| (i32::from_be_bytes([b[0], b[1], b[2], 0]) >> 8) as f32 / 8_388_608.0;
    let samples = (0..n)
        .map(|k| Complex {
            re: s24(&d[16 + 6 * k..]),
            im: s24(&d[19 + 6 * k..]),
        })
        .collect();
    Ok(DdcPacket {
        seq: u32::from_be_bytes([d[0], d[1], d[2], d[3]]),
        timestamp: u64::from_be_bytes([d[4], d[5], d[6], d[7], d[8], d[9], d[10], d[11]]),
        samples,
    })
}

// [@ANCHOR: hpsdr_p2_encode_ddc]
/// Build a DDC packet from 24-bit integers (emulator side).
pub fn encode_ddc_packet(seq: u32, timestamp: u64, iq: &[(i32, i32)]) -> Vec<u8> {
    let mut p = vec![0u8; 16 + 6 * iq.len()];
    put32(&mut p, 0, seq);
    p[4..12].copy_from_slice(&timestamp.to_be_bytes());
    put16(&mut p, 12, 24);
    put16(&mut p, 14, iq.len() as u16);
    for (k, (i, q)) in iq.iter().enumerate() {
        p[16 + 6 * k..19 + 6 * k].copy_from_slice(&i.to_be_bytes()[1..4]);
        p[19 + 6 * k..22 + 6 * k].copy_from_slice(&q.to_be_bytes()[1..4]);
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn hp() -> HpRx {
        HpRx {
            run: true,
            ddc_hz: vec![7_074_000, 14_074_000, 3_573_000],
            atten_adc0: 12,
            atten_adc1: 3,
            phase_clock_hz: None,
        }
    }

    #[test]
    fn discovery_and_reply_round_trip() {
        // Tests [@ANCHOR: hpsdr_p2_discovery_packet]
        // Tests [@ANCHOR: hpsdr_p2_parse_reply]
        // Tests [@ANCHOR: hpsdr_p2_encode_reply]
        let d = discovery_packet();
        assert_eq!((d.len(), d[4]), (60, 2));
        assert!(d[..4].iter().all(|b| *b == 0) && d[5..].iter().all(|b| *b == 0));
        let r = P2Reply {
            running: true,
            mac: [1, 2, 3, 4, 5, 6],
            board: Board::Hermes,
            protocol_version: 39,
            firmware: 21,
            num_ddcs: 4,
            phase_word: true,
            data_modes: 0,
        };
        assert_eq!(parse_reply(&encode_reply(&r, 1)), Ok(r));
        // Tests [@ANCHOR: hpsdr_discovery_parse_reply]
        let from: std::net::SocketAddr = ([192, 168, 1, 30], 1024).into();
        let found = crate::discovery::parse_reply(from, &encode_reply(&r, 1)).unwrap();
        assert_eq!(found.protocol, crate::types::Protocol::P2);
        assert_eq!(
            (found.num_ddcs, found.protocol_version, found.busy),
            (Some(4), Some(39), true)
        );
        assert_eq!(
            parse_reply(&encode_reply(&r, 254)),
            Err(HpsdrError::Unsupported(
                "hardware description in the discovery reply".into()
            ))
        );
        assert_eq!(parse_reply(&[0u8; 10]), Err(HpsdrError::Truncated));
        assert_eq!(parse_reply(&[1u8; 60]), Err(HpsdrError::BadMagic));
    }

    #[test]
    fn general_packet_carries_ports_and_the_hardware_timer() {
        // Tests [@ANCHOR: hpsdr_p2_ports_default]
        // Tests [@ANCHOR: hpsdr_p2_encode_general]
        let p = encode_general(0, &RadioPorts::default(), true);
        assert_eq!(&p[5..7], &1025u16.to_be_bytes());
        assert_eq!(&p[9..11], &1027u16.to_be_bytes());
        assert_eq!(&p[17..19], &1035u16.to_be_bytes());
        assert_eq!(
            (p[37], p[38], p[39], p[58], p[59]),
            (8, 1, 0, 0, 0),
            "phase words on, timer on, PA and Alex off"
        );
        assert_eq!(encode_general(0, &RadioPorts::default(), false)[37], 0);
    }

    #[test]
    fn ddc_specific_sets_enables_rates_and_adcs() {
        // Tests [@ANCHOR: hpsdr_p2_encode_ddc_specific]
        let ddcs = [
            DdcSetting {
                enabled: true,
                adc: 0,
                rate: Rate::K192,
            },
            DdcSetting {
                enabled: false,
                adc: 0,
                rate: Rate::K48,
            },
            DdcSetting {
                enabled: true,
                adc: 1,
                rate: Rate::K1536,
            },
        ];
        let p = encode_ddc_specific(5, 2, 0, 0, &ddcs).unwrap_or_default();
        assert_eq!((p.len(), p[3], p[4], p[7]), (1444, 5, 2, 0b101));
        assert_eq!(&p[17..23], &[0, 0, 192, 0, 0, 24]);
        assert_eq!(&p[29..35], &[1, 0x06, 0x00, 0, 0, 24], "1536 ksps on ADC1");
        assert!(encode_ddc_specific(0, 0, 0, 0, &ddcs).is_err());
        assert!(encode_ddc_specific(0, 1, 0, 0, &[ddcs[0]; 81]).is_err());
    }

    #[test]
    fn receive_high_priority_never_carries_transmit_state() {
        // Tests [@ANCHOR: hpsdr_p2_phase_word]
        // Tests [@ANCHOR: hpsdr_p2_encode_hp_rx]
        let p = encode_high_priority_rx(9, &hp()).unwrap_or_default();
        assert_eq!((p.len(), p[4]), (1444, 1), "run only; the PTT bit is clear");
        assert_eq!(&p[9..13], &7_074_000u32.to_be_bytes());
        assert_eq!(&p[13..17], &14_074_000u32.to_be_bytes());
        assert!(p[329..333].iter().all(|b| *b == 0), "DUC frequency zero");
        assert_eq!(p[345], 0, "drive zero");
        assert_eq!((p[1442], p[1443]), (3, 12));
        let mut phase = hp();
        phase.phase_clock_hz = Some(122_880_000.0);
        let q = encode_high_priority_rx(9, &phase).unwrap_or_default();
        assert_eq!(
            &q[9..13],
            &phase_word(7_074_000, 122_880_000.0).to_be_bytes()
        );
        assert_eq!(phase_word(122_880_000 / 4, 122_880_000.0), 1 << 30);
        for bad in [
            HpRx {
                atten_adc0: 32,
                ..hp()
            },
            HpRx {
                ddc_hz: vec![0; 81],
                ..hp()
            },
        ] {
            assert!(encode_high_priority_rx(0, &bad).is_err());
        }
    }

    #[test]
    fn transmit_packets_are_refused_while_the_gate_is_closed() {
        // Tests [@ANCHOR: hpsdr_p2_encode_hp_tx]
        // Tests [@ANCHOR: hpsdr_p2_encode_duc_iq]
        let now = Instant::now();
        let gate = TxGate::new();
        let tx = TxState {
            ptt: true,
            duc_hz: 7_074_000,
            drive: 200,
        };
        assert_eq!(
            encode_high_priority_tx(0, &hp(), &gate, now, &tx),
            Err(HpsdrError::GateClosed)
        );
        assert_eq!(
            encode_duc_iq(0, &[(1, 1)], &gate, now),
            Err(HpsdrError::GateClosed)
        );
        assert!(gate.open_until(now + Duration::from_secs(1)));
        assert_eq!(
            encode_high_priority_tx(0, &hp(), &gate, now + Duration::from_secs(2), &tx),
            Err(HpsdrError::GateClosed)
        );
        let p = encode_high_priority_tx(0, &hp(), &gate, now, &tx).unwrap_or_default();
        assert_eq!((p[4], p[345]), (3, 200));
        assert_eq!(&p[329..333], &7_074_000u32.to_be_bytes());
        let idle = encode_high_priority_tx(0, &hp(), &gate, now, &TxState { ptt: false, ..tx })
            .unwrap_or_default();
        assert_eq!(
            idle,
            encode_high_priority_rx(0, &hp()).unwrap_or_default(),
            "open gate, not keyed: identical to receive"
        );
        let iq =
            encode_duc_iq(3, &[(-2, 5), (0x7FFFFF, -0x800000)], &gate, now).unwrap_or_default();
        assert_eq!(&iq[4..10], &[0xFF, 0xFF, 0xFE, 0, 0, 5]);
        assert_eq!(&iq[10..16], &[0x7F, 0xFF, 0xFF, 0x80, 0, 0]);
        assert!(
            iq[16..].iter().all(|b| *b == 0),
            "short slices are padded with silence"
        );
        assert!(encode_duc_iq(0, &vec![(0, 0); 241], &gate, now).is_err());
    }

    #[test]
    fn status_and_ddc_packets_round_trip() {
        // Tests [@ANCHOR: hpsdr_p2_parse_hp_status]
        // Tests [@ANCHOR: hpsdr_p2_encode_hp_status]
        // Tests [@ANCHOR: hpsdr_p2_parse_ddc]
        // Tests [@ANCHOR: hpsdr_p2_encode_ddc]
        let st = HardwareStatus {
            adc_overload: [true, false, true, false],
            adc_max_magnitude: [32000, 12],
            ptt_input: true,
            exciter_raw: 10,
            forward_raw: 3000,
            reverse_raw: 20,
            supply_raw: 3900,
            pll_locked: Some(true),
            ..Default::default()
        };
        let (seq, back) =
            parse_hp_status(&encode_hp_status(41, &st)).unwrap_or((0, HardwareStatus::default()));
        assert_eq!((seq, back), (41, st));
        assert_eq!(parse_hp_status(&[0u8; 5]), Err(HpsdrError::Truncated));
        let iq = vec![(-8_388_608, 8_388_607), (1, -1), (0, 0)];
        let p = parse_ddc_packet(&encode_ddc_packet(7, 1 << 40, &iq)).unwrap_or(DdcPacket {
            seq: 0,
            timestamp: 0,
            samples: vec![],
        });
        assert_eq!((p.seq, p.timestamp), (7, 1 << 40));
        assert_eq!(
            p.samples[0],
            Complex {
                re: -1.0,
                im: 8_388_607.0 / 8_388_608.0
            }
        );
        assert_eq!(p.samples.len(), 3);
        let mut wide = encode_ddc_packet(0, 0, &iq);
        wide[13] = 32;
        assert!(matches!(
            parse_ddc_packet(&wide),
            Err(HpsdrError::Unsupported(_))
        ));
        assert_eq!(
            parse_ddc_packet(&encode_ddc_packet(0, 0, &iq)[..20]),
            Err(HpsdrError::Truncated)
        );
        assert_eq!(parse_ddc_packet(&[0u8; 4]), Err(HpsdrError::Truncated));
    }
}
