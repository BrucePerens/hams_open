// SPDX-License-Identifier: AGPL-3.0-or-later
//! Protocol 1 (Metis/USB-framed) wire format, sans-IO: functions turn settings into datagrams and
//! datagrams into values. Nothing here opens a socket.
//!
//! Sources (facts only; every item is `verified=unverified` until checked against a radio):
//! * "Metis - How it works" v1.33 (TAPR/OpenHPSDR-Firmware, Protocol 1/Documentation), cited
//!   "Metis" with its section name: discovery, start/stop and the `0xEFFE` datagram.
//! * "HPSDR - USB Data Protocol" v1.60 (TAPR/OpenHPSDR-SVN, Documentation/USB_protocol_V1.60.doc),
//!   cited "USB": the 512-byte frame, receiver packing and the command-and-control (C&C) bytes.
//!
//! Transmit safety: [`Ep2Builder::build_rx`] has no way to set MOX, drive or transmit I/Q. Only
//! [`Ep2Builder::build_tx`] can, and it returns [`HpsdrError::GateClosed`] unless the caller's
//! [`TxGate`] is open at that instant.

use crate::error::HpsdrError;
use crate::gate::TxGate;
use crate::iq::Complex;
use crate::types::{Board, HardwareStatus, Rate};
use std::time::Instant;

/// UDP port the radio listens on for discovery, start/stop and EP2 data (Metis, "Metis Discover Packet").
pub const PORT: u16 = 1024;
/// Largest receiver count addressable in Protocol 1: Rx1..Rx7 have NCO addresses 2..8 (USB, C0 table).
/// The C4 field could name 8; Rx8 has no frequency address in v1.60 (unverified).
pub const MAX_RX: u8 = 7;
/// Size of one EP2 or EP6 datagram: 8 header bytes and two 512-byte frames (Metis, "PC <> Metis UDP/IP data format").
pub const DATAGRAM_LEN: usize = 8 + 1024;
/// TX I/Q samples carried in one EP2 datagram: 63 per frame, two frames (USB, "Protocol - From PC to HPSDR").
pub const TX_SAMPLES_PER_DATAGRAM: usize = 126;

const MAGIC: [u8; 2] = [0xEF, 0xFE];
const SYNC: [u8; 3] = [0x7F, 0x7F, 0x7F];
const FRAME_LEN: usize = 512;

// [@ANCHOR: hpsdr_p1_discovery_packet]
/// The discovery request: `EF FE 02` and 60 zero bytes (Metis, "Metis Discover Packet format"). Send it
/// to 255.255.255.255:1024; Metis notes that the subnet broadcast is not implemented.
pub fn discovery_packet() -> [u8; 63] {
    let mut p = [0u8; 63];
    p[..2].copy_from_slice(&MAGIC);
    p[2] = 0x02;
    p
}

// [@ANCHOR: hpsdr_p1_start_packet]
/// Start command: `EF FE 04 <cmd>` and 60 zeros; bit 0 starts I/Q plus microphone data, bit 1 the
/// wide bandscope (Metis, "Start Command").
pub fn start_packet(iq: bool, bandscope: bool) -> [u8; 64] {
    let mut p = [0u8; 64];
    p[..2].copy_from_slice(&MAGIC);
    p[2] = 0x04;
    p[3] = u8::from(iq) | (u8::from(bandscope) << 1);
    p
}

// [@ANCHOR: hpsdr_p1_stop_packet]
/// Stop command: the start layout with both bits clear (Metis, "Stop Command").
pub fn stop_packet() -> [u8; 64] {
    start_packet(false, false)
}

/// A parsed Protocol 1 discovery reply (Metis, "Metis Discovery reply format").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct P1Reply {
    /// True when the radio is already sending data (status byte 0x03).
    pub running: bool,
    /// The radio's MAC address.
    pub mac: [u8; 6],
    /// Gateware code version.
    pub code_version: u8,
    /// Raw board identifier.
    pub board_id: u8,
    /// Decoded board.
    pub board: Board,
}

// [@ANCHOR: hpsdr_p1_parse_reply]
/// Parse a discovery reply: `EF FE <status> <MAC 6> <code version> <board id>` then zeros.
pub fn parse_reply(d: &[u8]) -> Result<P1Reply, HpsdrError> {
    if d.len() < 11 {
        return Err(HpsdrError::Truncated);
    }
    if d[..2] != MAGIC || !(d[2] == 2 || d[2] == 3) {
        return Err(HpsdrError::BadMagic);
    }
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&d[3..9]);
    Ok(P1Reply {
        running: d[2] == 3,
        mac,
        code_version: d[9],
        board_id: d[10],
        board: Board::from_p1_id(d[10]),
    })
}

// [@ANCHOR: hpsdr_p1_encode_reply]
/// Build a discovery reply (used by the emulator): 60 bytes.
pub fn encode_reply(r: &P1Reply) -> [u8; 60] {
    let mut p = [0u8; 60];
    p[..2].copy_from_slice(&MAGIC);
    p[2] = if r.running { 3 } else { 2 };
    p[3..9].copy_from_slice(&r.mac);
    p[9] = r.code_version;
    p[10] = r.board_id;
    p
}

/// Receiver and front-end settings carried by the C&C bytes of EP2 frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct P1Config {
    /// DDC sample rate (48/96/192/384 ksps).
    pub rate: Rate,
    /// Number of receivers, 1..=7.
    pub num_rx: u8,
    /// NCO frequency of each receiver in hertz.
    pub rx_hz: [u32; 7],
    /// Receiver pre-amplifier (C0=0, C3 bit 2).
    pub preamp: bool,
    /// ADC dither (C3 bit 3).
    pub dither: bool,
    /// ADC random (C3 bit 4).
    pub random: bool,
    /// ADC1 step attenuator, 0..=31 dB (C0=10, C4[4:0]).
    pub att_db: u8,
    /// Enable the step attenuator (C4 bit 5); when clear the pre-amp bit applies instead.
    pub att_enable: bool,
    /// Transmit NCO frequency in hertz (C0=1).
    pub tx_hz: u32,
}

impl Default for P1Config {
    // [@ANCHOR: hpsdr_p1_config_default]
    /// One receiver at 48 ksps on 7.1 MHz, no attenuation.
    fn default() -> Self {
        Self {
            rate: Rate::K48,
            num_rx: 1,
            rx_hz: [7_100_000; 7],
            preamp: false,
            dither: false,
            random: false,
            att_db: 0,
            att_enable: false,
            tx_hz: 7_100_000,
        }
    }
}

impl P1Config {
    // [@ANCHOR: hpsdr_p1_config_validate]
    /// Reject settings the protocol cannot express.
    pub fn validate(&self) -> Result<(), HpsdrError> {
        self.rate.p1_code()?;
        if self.num_rx == 0 || self.num_rx > MAX_RX {
            return Err(HpsdrError::Config(format!(
                "receivers {} (1..=7)",
                self.num_rx
            )));
        }
        if self.att_db > 31 {
            return Err(HpsdrError::Config(format!(
                "attenuator {} dB (0..=31)",
                self.att_db
            )));
        }
        Ok(())
    }

    // [@ANCHOR: hpsdr_p1_config_addresses]
    /// The C&C addresses sent in round-robin order: general (0), each receiver NCO (2..), transmit
    /// NCO (1), drive (9) and attenuator (10).
    pub fn addresses(&self) -> Vec<u8> {
        let mut v = vec![0u8];
        v.extend((0..self.num_rx).map(|i| 2 + i));
        v.extend([1u8, 9, 10]);
        v
    }

    // [@ANCHOR: hpsdr_p1_config_encode_cc]
    /// The five C&C bytes for `addr`. `mox` is C0 bit 0; `drive` is the C1 byte of address 9. The
    /// receive builder passes `false` and 0; only the gated transmit builder passes anything else.
    fn encode_cc(&self, addr: u8, mox: bool, drive: u8) -> [u8; 5] {
        let mut cc = [(addr << 1) | u8::from(mox), 0, 0, 0, 0];
        let hz = |f: u32| f.to_be_bytes();
        match addr {
            0 => {
                cc[1] = self.rate.p1_code().unwrap_or(0);
                cc[3] = (u8::from(self.preamp) << 2)
                    | (u8::from(self.dither) << 3)
                    | (u8::from(self.random) << 4);
                cc[4] = (self.num_rx.saturating_sub(1) & 7) << 3;
            }
            1 => cc[1..5].copy_from_slice(&hz(self.tx_hz)),
            2..=8 => {
                let i = usize::from(addr - 2);
                cc[1..5].copy_from_slice(&hz(self.rx_hz[i]));
            }
            9 => cc[1] = drive,
            10 => cc[4] = (self.att_db & 31) | (u8::from(self.att_enable) << 5),
            _ => {}
        }
        cc
    }

    // [@ANCHOR: hpsdr_p1_config_apply_cc]
    /// Fold one received C&C quintuple into the settings (used by the emulator, which plays the radio).
    /// Returns `(mox, drive)` as seen in this quintuple so the emulator can count transmit requests.
    pub fn apply_cc(&mut self, cc: &[u8; 5]) -> (bool, Option<u8>) {
        let addr = cc[0] >> 1;
        let mox = cc[0] & 1 == 1;
        let word = u32::from_be_bytes([cc[1], cc[2], cc[3], cc[4]]);
        let mut drive = None;
        match addr {
            0 => {
                self.rate = Rate::from_p1_code(cc[1]);
                self.preamp = cc[3] & 4 != 0;
                self.dither = cc[3] & 8 != 0;
                self.random = cc[3] & 16 != 0;
                self.num_rx = ((cc[4] >> 3) & 7) + 1;
            }
            1 => self.tx_hz = word,
            2..=8 => self.rx_hz[usize::from(addr - 2)] = word,
            9 => drive = Some(cc[1]),
            10 => {
                self.att_db = cc[4] & 31;
                self.att_enable = cc[4] & 32 != 0;
            }
            _ => {}
        }
        (mox, drive)
    }
}

/// Transmit request for [`Ep2Builder::build_tx`].
#[derive(Debug, Clone, Copy)]
pub struct TxRequest<'a> {
    /// Key the transmitter (MOX).
    pub ptt: bool,
    /// Drive level 0..=255 (C0=9, C1).
    pub drive: u8,
    /// Transmit I/Q as 16-bit (I, Q) pairs at 48 ksps; at most [`TX_SAMPLES_PER_DATAGRAM`]; a short
    /// slice is padded with zeros (silence) and a sample is never repeated.
    pub iq: &'a [(i16, i16)],
}

/// Builds EP2 datagrams (PC to radio) with the C&C round-robin and a sequence counter.
#[derive(Debug, Clone, Default)]
pub struct Ep2Builder {
    seq: u32,
    cursor: usize,
}

impl Ep2Builder {
    // [@ANCHOR: hpsdr_p1_ep2_new]
    /// A builder with sequence 0.
    pub fn new() -> Self {
        Self::default()
    }

    // [@ANCHOR: hpsdr_p1_ep2_reset]
    /// Restart the sequence at zero, as the PC must on every Start (Metis, "PC <> Metis UDP/IP data format").
    pub fn reset(&mut self) {
        self.seq = 0;
        self.cursor = 0;
    }

    // [@ANCHOR: hpsdr_p1_ep2_assemble]
    /// Common assembly: header, then two frames each with the next C&C address.
    fn assemble(
        &mut self,
        cfg: &P1Config,
        mox: bool,
        drive: u8,
        iq: &[(i16, i16)],
    ) -> Result<Vec<u8>, HpsdrError> {
        cfg.validate()?;
        let addrs = cfg.addresses();
        let mut out = Vec::with_capacity(DATAGRAM_LEN);
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&[0x01, 0x02]);
        out.extend_from_slice(&self.seq.to_be_bytes());
        for f in 0..2 {
            let addr = addrs[self.cursor % addrs.len()];
            self.cursor = (self.cursor + 1) % addrs.len();
            out.extend_from_slice(&SYNC);
            out.extend_from_slice(&cfg.encode_cc(addr, mox, drive));
            for k in 0..63 {
                let (i, q) = iq.get(f * 63 + k).copied().unwrap_or((0, 0));
                // L and R audio stay zero. The USB spec says I and Q are swapped relative to receive
                // and the PC must swap them before sending (USB, note after the EP2 frame).
                out.extend_from_slice(&[0, 0, 0, 0]);
                out.extend_from_slice(&q.to_be_bytes());
                out.extend_from_slice(&i.to_be_bytes());
            }
        }
        self.seq = self.seq.wrapping_add(1);
        Ok(out)
    }

    // [@ANCHOR: hpsdr_p1_ep2_build_rx]
    /// A receive-only datagram: MOX clear, drive zero, transmit I/Q zero, for every C&C address.
    /// This function has no transmit parameters.
    pub fn build_rx(&mut self, cfg: &P1Config) -> Result<Vec<u8>, HpsdrError> {
        self.assemble(cfg, false, 0, &[])
    }

    // [@ANCHOR: hpsdr_p1_ep2_build_tx]
    /// A datagram that may key the transmitter. Returns [`HpsdrError::GateClosed`] and builds nothing
    /// unless `gate` is open at `now`. With the gate open and `req.ptt` false the datagram is the
    /// receive one.
    pub fn build_tx(
        &mut self,
        cfg: &P1Config,
        gate: &TxGate,
        now: Instant,
        req: &TxRequest<'_>,
    ) -> Result<Vec<u8>, HpsdrError> {
        if !gate.is_open(now) {
            return Err(HpsdrError::GateClosed);
        }
        if req.ptt {
            let n = req.iq.len().min(TX_SAMPLES_PER_DATAGRAM);
            self.assemble(cfg, true, req.drive, &req.iq[..n])
        } else {
            self.assemble(cfg, false, 0, &[])
        }
    }
}

/// An EP2 datagram as the radio sees it (the emulator uses this to count transmit requests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ep2 {
    /// Datagram sequence number.
    pub seq: u32,
    /// The two C&C quintuples.
    pub cc: [[u8; 5]; 2],
    /// True if any transmit I/Q byte (I or Q slots) is non-zero.
    pub tx_iq_nonzero: bool,
}

// [@ANCHOR: hpsdr_p1_parse_ep2]
/// Parse a PC-to-radio datagram.
pub fn parse_ep2(d: &[u8]) -> Result<Ep2, HpsdrError> {
    if d.len() < DATAGRAM_LEN {
        return Err(HpsdrError::Truncated);
    }
    if d[..4] != [0xEF, 0xFE, 0x01, 0x02] {
        return Err(HpsdrError::BadMagic);
    }
    let seq = u32::from_be_bytes([d[4], d[5], d[6], d[7]]);
    let mut cc = [[0u8; 5]; 2];
    let mut nonzero = false;
    for (f, c) in cc.iter_mut().enumerate() {
        let fr = &d[8 + f * FRAME_LEN..8 + (f + 1) * FRAME_LEN];
        if fr[..3] != SYNC {
            return Err(HpsdrError::BadMagic);
        }
        c.copy_from_slice(&fr[3..8]);
        nonzero |= fr[8..]
            .as_chunks::<8>()
            .0
            .iter()
            .any(|g| g[4..8].iter().any(|b| *b != 0));
    }
    Ok(Ep2 {
        seq,
        cc,
        tx_iq_nonzero: nonzero,
    })
}

/// One decoded EP6 datagram (radio to PC).
#[derive(Debug, Clone, PartialEq)]
pub struct Ep6 {
    /// Datagram sequence number (independent per end point).
    pub seq: u32,
    /// The two C&C quintuples (radio-to-PC meaning).
    pub cc: [[u8; 5]; 2],
    /// Samples of each receiver, in time order across both frames.
    pub rx: Vec<Vec<Complex>>,
    /// Microphone samples (16-bit, 48 ksps).
    pub mic: Vec<i16>,
}

// [@ANCHOR: hpsdr_p1_samples_per_frame]
/// Samples per receiver in one 512-byte frame: `504 / (6 n + 2)` (USB, "NOTE 1" padding table agrees).
pub fn samples_per_frame(num_rx: u8) -> usize {
    504 / (6 * usize::from(num_rx) + 2)
}

// [@ANCHOR: hpsdr_p1_parse_ep6]
/// Decode an EP6 datagram carrying `num_rx` receivers. An EP4 (bandscope) datagram is reported as
/// [`HpsdrError::Unsupported`].
pub fn parse_ep6(d: &[u8], num_rx: u8) -> Result<Ep6, HpsdrError> {
    if num_rx == 0 || num_rx > 8 {
        return Err(HpsdrError::Config(format!("receivers {num_rx}")));
    }
    if d.len() < DATAGRAM_LEN {
        return Err(HpsdrError::Truncated);
    }
    if d[..3] != [0xEF, 0xFE, 0x01] {
        return Err(HpsdrError::BadMagic);
    }
    if d[3] != 0x06 {
        return Err(HpsdrError::Unsupported(format!("end point {:#04x}", d[3])));
    }
    let n = usize::from(num_rx);
    let per = samples_per_frame(num_rx);
    let seq = u32::from_be_bytes([d[4], d[5], d[6], d[7]]);
    let mut out = Ep6 {
        seq,
        cc: [[0; 5]; 2],
        rx: vec![Vec::with_capacity(per * 2); n],
        mic: Vec::with_capacity(per * 2),
    };
    let s24 = |b: &[u8]| (i32::from_be_bytes([b[0], b[1], b[2], 0]) >> 8) as f32 / 8_388_608.0;
    for f in 0..2 {
        let fr = &d[8 + f * FRAME_LEN..8 + (f + 1) * FRAME_LEN];
        if fr[..3] != SYNC {
            return Err(HpsdrError::BadMagic);
        }
        out.cc[f].copy_from_slice(&fr[3..8]);
        let body = &fr[8..];
        for s in 0..per {
            let g = &body[s * (6 * n + 2)..(s + 1) * (6 * n + 2)];
            for r in 0..n {
                out.rx[r].push(Complex {
                    re: s24(&g[r * 6..r * 6 + 3]),
                    im: s24(&g[r * 6 + 3..r * 6 + 6]),
                });
            }
            out.mic.push(i16::from_be_bytes([g[6 * n], g[6 * n + 1]]));
        }
    }
    Ok(out)
}

// [@ANCHOR: hpsdr_p1_encode_ep6]
/// Build an EP6 datagram (used by the emulator). `rx[r]` holds the 24-bit (I, Q) integers of receiver
/// `r`, exactly `2 * samples_per_frame(n)` of them; `mic` likewise 16-bit.
pub fn encode_ep6(
    seq: u32,
    cc: &[[u8; 5]; 2],
    rx: &[Vec<(i32, i32)>],
    mic: &[i16],
) -> Result<Vec<u8>, HpsdrError> {
    let n = rx.len();
    let per =
        samples_per_frame(u8::try_from(n).map_err(|_| HpsdrError::Config("receivers".into()))?);
    if n == 0 || rx.iter().any(|r| r.len() != 2 * per) || mic.len() != 2 * per {
        return Err(HpsdrError::Config("sample counts".into()));
    }
    let mut out = Vec::with_capacity(DATAGRAM_LEN);
    out.extend_from_slice(&[0xEF, 0xFE, 0x01, 0x06]);
    out.extend_from_slice(&seq.to_be_bytes());
    for f in 0..2 {
        out.extend_from_slice(&SYNC);
        out.extend_from_slice(&cc[f]);
        for s in 0..per {
            for r in rx {
                let (i, q) = r[f * per + s];
                out.extend_from_slice(&i.to_be_bytes()[1..4]);
                out.extend_from_slice(&q.to_be_bytes()[1..4]);
            }
            out.extend_from_slice(&mic[f * per + s].to_be_bytes());
        }
        out.resize(8 + (f + 1) * FRAME_LEN, 0);
    }
    Ok(out)
}

// [@ANCHOR: hpsdr_p1_apply_status_cc]
/// Fold one radio-to-PC C&C quintuple into `st`. C0 bits 7..3 are the address; bits 2..0 are the
/// DOT, DASH and PTT inputs (USB, "Command & Control" under "From HPSDR to PC").
pub fn apply_status_cc(st: &mut HardwareStatus, cc: &[u8; 5]) {
    st.ptt_input = cc[0] & 1 != 0;
    st.dash_input = cc[0] & 2 != 0;
    st.dot_input = cc[0] & 4 != 0;
    let w = |a: u8, b: u8| u16::from_be_bytes([a, b]) & 0x0fff;
    match cc[0] >> 3 {
        0 => {
            st.adc_overload[0] = cc[1] & 1 != 0;
            st.firmware = cc[4];
        }
        1 => {
            st.exciter_raw = w(cc[1], cc[2]);
            st.forward_raw = w(cc[3], cc[4]);
        }
        2 => st.reverse_raw = w(cc[1], cc[2]),
        3 => st.supply_raw = w(cc[3], cc[4]),
        4 => {
            for (i, b) in cc[1..5].iter().enumerate() {
                st.adc_overload[i] = b & 1 != 0;
            }
        }
        _ => {}
    }
}

// [@ANCHOR: hpsdr_p1_encode_status_cc]
/// Build the radio-to-PC C&C quintuple for status address `addr` (0..=4) from `st` (emulator side).
pub fn encode_status_cc(st: &HardwareStatus, addr: u8) -> [u8; 5] {
    let c0 = (addr << 3)
        | u8::from(st.ptt_input)
        | (u8::from(st.dash_input) << 1)
        | (u8::from(st.dot_input) << 2);
    let be = |v: u16| v.to_be_bytes();
    let mut cc = [c0, 0, 0, 0, 0];
    match addr {
        0 => {
            cc[1] = u8::from(st.adc_overload[0]);
            cc[4] = st.firmware;
        }
        1 => {
            cc[1..3].copy_from_slice(&be(st.exciter_raw));
            cc[3..5].copy_from_slice(&be(st.forward_raw));
        }
        2 => cc[1..3].copy_from_slice(&be(st.reverse_raw)),
        3 => cc[3..5].copy_from_slice(&be(st.supply_raw)),
        4 => {
            for (i, o) in st.adc_overload.iter().enumerate() {
                cc[1 + i] = u8::from(*o);
            }
        }
        _ => {}
    }
    cc
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn cfg4() -> P1Config {
        P1Config {
            rate: Rate::K192,
            num_rx: 4,
            rx_hz: [3_573_000, 7_074_000, 14_074_000, 21_074_000, 0, 0, 0],
            preamp: true,
            dither: true,
            random: false,
            att_db: 17,
            att_enable: true,
            tx_hz: 7_074_000,
        }
    }

    #[test]
    fn discovery_start_stop_bytes() {
        // Tests [@ANCHOR: hpsdr_p1_discovery_packet]
        // Tests [@ANCHOR: hpsdr_p1_start_packet]
        // Tests [@ANCHOR: hpsdr_p1_stop_packet]
        let d = discovery_packet();
        assert_eq!(&d[..3], &[0xEF, 0xFE, 0x02]);
        assert!(d[3..].iter().all(|b| *b == 0) && d.len() == 63);
        assert_eq!(&start_packet(true, false)[..4], &[0xEF, 0xFE, 0x04, 0x01]);
        assert_eq!(start_packet(true, true)[3], 0x03);
        assert_eq!(stop_packet()[3], 0x00);
        assert_eq!(stop_packet().len(), 64);
    }

    #[test]
    fn reply_round_trips_and_rejects_junk() {
        // Tests [@ANCHOR: hpsdr_p1_parse_reply]
        // Tests [@ANCHOR: hpsdr_p1_encode_reply]
        let r = P1Reply {
            running: true,
            mac: [1, 2, 3, 4, 5, 6],
            code_version: 73,
            board_id: 1,
            board: Board::Hermes,
        };
        let e = encode_reply(&r);
        assert_eq!(e.len(), 60);
        assert_eq!(parse_reply(&e), Ok(r));
        assert_eq!(parse_reply(&e[..10]), Err(HpsdrError::Truncated));
        let mut bad = e;
        bad[0] = 0;
        assert_eq!(parse_reply(&bad), Err(HpsdrError::BadMagic));
        let mut st = e;
        st[2] = 9;
        assert_eq!(parse_reply(&st), Err(HpsdrError::BadMagic));
    }

    #[test]
    fn config_validation_and_cc_round_trip() {
        // Tests [@ANCHOR: hpsdr_p1_config_default]
        // Tests [@ANCHOR: hpsdr_p1_config_validate]
        // Tests [@ANCHOR: hpsdr_p1_config_addresses]
        // Tests [@ANCHOR: hpsdr_p1_config_encode_cc]
        // Tests [@ANCHOR: hpsdr_p1_config_apply_cc]
        assert!(P1Config::default().validate().is_ok());
        let c = cfg4();
        assert!(c.validate().is_ok());
        assert_eq!(c.addresses(), vec![0, 2, 3, 4, 5, 1, 9, 10]);
        let mut back = P1Config {
            num_rx: 1,
            rx_hz: [0; 7],
            ..P1Config::default()
        };
        for a in c.addresses() {
            let cc = c.encode_cc(a, false, 0);
            let (mox, drive) = back.apply_cc(&cc);
            assert!(!mox, "receive C&C never carries MOX");
            assert!(drive.is_none() || drive == Some(0));
        }
        back.rx_hz[4] = 0;
        assert_eq!(back, c);
        for bad in [
            P1Config {
                num_rx: 0,
                ..c.clone()
            },
            P1Config {
                num_rx: 8,
                ..c.clone()
            },
            P1Config {
                att_db: 32,
                ..c.clone()
            },
            P1Config {
                rate: Rate::K768,
                ..c.clone()
            },
        ] {
            assert!(bad.validate().is_err());
        }
    }

    #[test]
    fn receive_datagrams_never_carry_transmit_state() {
        // Tests [@ANCHOR: hpsdr_p1_ep2_new]
        // Tests [@ANCHOR: hpsdr_p1_ep2_assemble]
        // Tests [@ANCHOR: hpsdr_p1_ep2_build_rx]
        // Tests [@ANCHOR: hpsdr_p1_parse_ep2]
        let c = cfg4();
        let mut b = Ep2Builder::new();
        let mut seen_addrs = std::collections::BTreeSet::new();
        for n in 0..40u32 {
            let d = b.build_rx(&c).unwrap_or_default();
            assert_eq!(d.len(), DATAGRAM_LEN);
            let p = parse_ep2(&d).unwrap_or(Ep2 {
                seq: 999,
                cc: [[0; 5]; 2],
                tx_iq_nonzero: true,
            });
            assert_eq!(p.seq, n);
            assert!(!p.tx_iq_nonzero, "transmit I/Q slots are zero");
            for cc in p.cc {
                assert_eq!(cc[0] & 1, 0, "MOX clear in every frame");
                if cc[0] >> 1 == 9 {
                    assert_eq!(cc[1], 0, "drive is zero in receive datagrams");
                }
                seen_addrs.insert(cc[0] >> 1);
            }
        }
        assert_eq!(
            seen_addrs.into_iter().collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4, 5, 9, 10]
        );
        let bad = P1Config { num_rx: 0, ..c };
        assert!(b.build_rx(&bad).is_err());
    }

    #[test]
    fn transmit_builder_refuses_unless_gate_open() {
        // Tests [@ANCHOR: hpsdr_p1_ep2_build_tx]
        // Tests [@ANCHOR: hpsdr_p1_ep2_reset]
        let c = cfg4();
        let now = Instant::now();
        let gate = TxGate::new();
        let iq = [(1000i16, -2000i16); 126];
        let req = TxRequest {
            ptt: true,
            drive: 200,
            iq: &iq,
        };
        let mut b = Ep2Builder::new();
        assert_eq!(
            b.build_tx(&c, &gate, now, &req),
            Err(HpsdrError::GateClosed),
            "default gate is closed"
        );
        assert!(gate.open_until(now + Duration::from_secs(1)));
        assert_eq!(
            b.build_tx(&c, &gate, now + Duration::from_secs(2), &req),
            Err(HpsdrError::GateClosed),
            "expired lease"
        );
        // Open: the datagram carries MOX, drive and the I/Q with I and Q swapped.
        let d = b.build_tx(&c, &gate, now, &req).unwrap_or_default();
        let p = parse_ep2(&d).unwrap_or(Ep2 {
            seq: 999,
            cc: [[0; 5]; 2],
            tx_iq_nonzero: false,
        });
        assert_eq!(p.seq, 0, "refused calls did not consume sequence numbers");
        assert!(p.tx_iq_nonzero);
        assert!(p.cc.iter().all(|cc| cc[0] & 1 == 1));
        assert_eq!(
            &d[8 + 8..8 + 16],
            &[0, 0, 0, 0, 0xF8, 0x30, 0x03, 0xE8],
            "Q (-2000) then I (1000)"
        );
        // Gate open but not keyed: identical to receive.
        let idle = TxRequest {
            ptt: false,
            drive: 255,
            iq: &iq,
        };
        let d2 = b.build_tx(&c, &gate, now, &idle).unwrap_or_default();
        let p2 = parse_ep2(&d2).unwrap_or(Ep2 {
            seq: 999,
            cc: [[0; 5]; 2],
            tx_iq_nonzero: true,
        });
        assert!(!p2.tx_iq_nonzero && p2.cc.iter().all(|cc| cc[0] & 1 == 0));
        // A short slice is padded with silence, not repeated.
        let short = TxRequest {
            ptt: true,
            drive: 1,
            iq: &iq[..1],
        };
        let d3 = b.build_tx(&c, &gate, now, &short).unwrap_or_default();
        assert_eq!(&d3[8 + 8 + 8..8 + 8 + 16], &[0u8; 8]);
        b.reset();
        assert_eq!(
            parse_ep2(&b.build_rx(&c).unwrap_or_default()).map(|p| p.seq),
            Ok(0)
        );
    }

    #[test]
    fn ep6_round_trips_for_one_to_seven_receivers() {
        // Tests [@ANCHOR: hpsdr_p1_samples_per_frame]
        // Tests [@ANCHOR: hpsdr_p1_parse_ep6]
        // Tests [@ANCHOR: hpsdr_p1_encode_ep6]
        // The spec's padding table: 504 - per*(6n+2) = 0,0,4,10,24,10,20,4 for n=1..8.
        for (n, pad) in [
            (1u8, 0usize),
            (2, 0),
            (3, 4),
            (4, 10),
            (5, 24),
            (6, 10),
            (7, 20),
            (8, 4),
        ] {
            assert_eq!(
                504 - samples_per_frame(n) * (6 * usize::from(n) + 2),
                pad,
                "n={n}"
            );
        }
        for n in 1..=7u8 {
            let per = samples_per_frame(n);
            let rx: Vec<Vec<(i32, i32)>> = (0..n)
                .map(|r| {
                    (0..2 * per)
                        .map(|s| (-(s as i32) * 1000 - i32::from(r), 8_388_607 - s as i32))
                        .collect()
                })
                .collect();
            let mic: Vec<i16> = (0..2 * per).map(|s| s as i16 - 5).collect();
            let cc = [[0x08, 1, 2, 3, 4], [0x10, 5, 6, 7, 8]];
            let d = encode_ep6(77, &cc, &rx, &mic).unwrap_or_default();
            assert_eq!(d.len(), DATAGRAM_LEN);
            let p = parse_ep6(&d, n).unwrap_or(Ep6 {
                seq: 0,
                cc: [[0; 5]; 2],
                rx: vec![],
                mic: vec![],
            });
            assert_eq!(p.seq, 77);
            assert_eq!(p.cc, cc);
            assert_eq!(p.mic, mic);
            for (r, got) in p.rx.iter().enumerate() {
                for (s, c) in got.iter().enumerate() {
                    assert_eq!(c.re, rx[r][s].0 as f32 / 8_388_608.0);
                    assert_eq!(c.im, rx[r][s].1 as f32 / 8_388_608.0);
                }
            }
        }
        let good = encode_ep6(
            1,
            &[[0; 5]; 2],
            &[vec![(0, 0); 2 * samples_per_frame(1)]],
            &vec![0; 2 * samples_per_frame(1)],
        )
        .unwrap_or_default();
        assert_eq!(parse_ep6(&good[..100], 1), Err(HpsdrError::Truncated));
        assert_eq!(
            parse_ep6(&good, 0),
            Err(HpsdrError::Config("receivers 0".into()))
        );
        let mut ep4 = good.clone();
        ep4[3] = 4;
        assert!(matches!(
            parse_ep6(&ep4, 1),
            Err(HpsdrError::Unsupported(_))
        ));
        let mut nosync = good.clone();
        nosync[8] = 0;
        assert_eq!(parse_ep6(&nosync, 1), Err(HpsdrError::BadMagic));
        let mut magic = good;
        magic[0] = 0;
        assert_eq!(parse_ep6(&magic, 1), Err(HpsdrError::BadMagic));
        assert!(encode_ep6(0, &[[0; 5]; 2], &[vec![(0, 0); 3]], &[0; 3]).is_err());
    }

    #[test]
    fn status_cc_round_trips_overload_power_and_supply() {
        // Tests [@ANCHOR: hpsdr_p1_apply_status_cc]
        // Tests [@ANCHOR: hpsdr_p1_encode_status_cc]
        let src = HardwareStatus {
            adc_overload: [true, false, true, false],
            ptt_input: true,
            dot_input: false,
            dash_input: true,
            exciter_raw: 0x123,
            forward_raw: 0xabc,
            reverse_raw: 0x0f0,
            supply_raw: 0x9ff,
            firmware: 73,
            ..Default::default()
        };
        let mut st = HardwareStatus::default();
        for a in 0..=5u8 {
            apply_status_cc(&mut st, &encode_status_cc(&src, a));
        }
        assert_eq!(st.adc_overload, src.adc_overload);
        assert_eq!(
            (
                st.exciter_raw,
                st.forward_raw,
                st.reverse_raw,
                st.supply_raw
            ),
            (0x123, 0xabc, 0x0f0, 0x9ff)
        );
        assert_eq!(st.firmware, 73);
        assert!(st.ptt_input && st.dash_input && !st.dot_input);
    }
}
