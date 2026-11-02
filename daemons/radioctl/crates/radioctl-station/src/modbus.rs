// SPDX-License-Identifier: AGPL-3.0-or-later
//! A Modbus coil relay driver: ASCII and RTU framing, slave address, coil map, baud and timing as
//! description data. Written from the public Modbus serial-line framing rules and from the
//! decode of the SainSmart 16-channel module in proposal section 4.1a; no vendor code or table is
//! used (the vendor's table has wrong checksums, so every frame here is computed).
//!
//! The framing is sans-IO (plain functions over bytes). The driver runs it over any async byte
//! stream (a radioctl serial link). Anything the driver does not recognise is an error: a reply
//! with a bad checksum, the wrong slave, the wrong function, an exception code or a wrong length
//! fails closed and the channel is reported as faulted, never as confirmed.

use crate::channel::{Assurance, BoxFuture, ChannelDriver, Recorded};
use crate::error::{StationError, StationErrorKind};
use serde::{Deserialize, Serialize};
use std::sync::Mutex as StdMutex;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout, Instant};

const FN_READ_COILS: u8 = 0x01;
const FN_WRITE_COIL: u8 = 0x05;
const FN_WRITE_COILS: u8 = 0x0F;
const MAX_FRAME: usize = 520;

/// Serial-line framing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Framing {
    /// `:` hex-ASCII bytes, LRC, CR LF.
    Ascii,
    /// Binary bytes, CRC-16, frames separated by idle time.
    Rtu,
}

/// A coil relay board, as data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModbusBoard {
    /// Model name.
    pub name: String,
    /// ASCII or RTU.
    pub framing: Framing,
    /// Slave address.
    pub slave: u8,
    /// Baud rate, 8 data bits, no parity, 1 stop bit unless the board says otherwise.
    pub baud: u32,
    /// Number of coils (relays).
    pub coils: u16,
    /// Coil address of line 0.
    #[serde(default)]
    pub coil_base: u16,
    /// How long to wait for a reply.
    pub reply_timeout_ms: u64,
    /// RTU: idle time that ends a frame. Also the pause kept before each request.
    pub inter_frame_ms: u64,
    /// Whether the board's coil read (function 1) is trusted to report the relay states (Bruce,
    /// 2026-11-01, decision 23). False: function 1 is NEVER sent, the driver keeps its own record
    /// of acknowledged writes and the channel assurance is [`Assurance::Recorded`]. Even when
    /// true, the coil bits are the commanded states, not a measurement of the contacts.
    #[serde(default = "yes", alias = "read_back")]
    pub status_readable: bool,
    /// The level of each coil the driver applies, in ONE function 15 frame, when the link opens
    /// and after every reconnect, and in the emergency all-off. Empty means every coil off;
    /// otherwise one entry per coil (a board may need some channels on).
    #[serde(default)]
    pub safe_state: Vec<bool>,
    /// Set to true once checked on a real module.
    #[serde(default)]
    pub verified: bool,
}

fn yes() -> bool {
    true
}

impl ModbusBoard {
    // [@ANCHOR: radioctl_station_modbus_safe_bits]
    /// The safe level of every coil, in coil order: `safe_state`, or all off when it is empty.
    pub fn safe_bits(&self) -> Vec<bool> {
        if self.safe_state.is_empty() {
            vec![false; usize::from(self.coils)]
        } else {
            self.safe_state.clone()
        }
    }

    // [@ANCHOR: radioctl_station_modbus_sainsmart]
    /// The SainSmart 16-channel 9-36 V USB relay module (sku 101-70-208): Modbus ASCII, 9600
    /// baud, slave 254, 16 coils from 0. Unverified on hardware. Its status is assumed unreadable
    /// (Bruce, 2026-11-01): `status_readable` is false and the safe state is every relay off.
    pub fn sainsmart_16ch() -> ModbusBoard {
        ModbusBoard {
            name: "sainsmart-16ch-modbus".into(),
            framing: Framing::Ascii,
            slave: 0xFE,
            baud: 9600,
            coils: 16,
            coil_base: 0,
            reply_timeout_ms: 1000,
            inter_frame_ms: 20,
            status_readable: false,
            safe_state: vec![false; 16],
            verified: false,
        }
    }

    // [@ANCHOR: radioctl_station_modbus_from_toml]
    /// Parses a board description.
    pub fn from_toml(text: &str) -> Result<ModbusBoard, StationError> {
        let b: ModbusBoard = toml::from_str(text).map_err(|e| {
            StationError::new(StationErrorKind::Config, format!("modbus board: {e}"))
        })?;
        if b.coils == 0
            || b.coils > 2000
            || b.slave == 0
            || b.slave > 247 && b.slave != 0xFE && b.slave != 0xFF
        {
            return Err(StationError::new(
                StationErrorKind::Config,
                "modbus board: bad slave or coil count",
            ));
        }
        if !b.safe_state.is_empty() && b.safe_state.len() != usize::from(b.coils) {
            return Err(StationError::new(
                StationErrorKind::Config,
                "modbus board: safe_state needs one entry per coil",
            ));
        }
        Ok(b)
    }
}

// [@ANCHOR: radioctl_station_modbus_lrc]
/// The Modbus ASCII checksum: the two's complement of the byte sum, low byte.
pub fn lrc(data: &[u8]) -> u8 {
    data.iter()
        .fold(0u8, |a, b| a.wrapping_add(*b))
        .wrapping_neg()
}

// [@ANCHOR: radioctl_station_modbus_crc16]
/// The Modbus RTU CRC-16 (polynomial 0xA001 reflected, start 0xFFFF).
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for byte in data {
        crc ^= u16::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

fn driver_err(m: impl Into<String>) -> StationError {
    StationError::new(StationErrorKind::Driver, m)
}

// [@ANCHOR: radioctl_station_modbus_hex_val]
/// One hex digit's value.
fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'A'..=b'F' => Some(c - b'A' + 10),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

// [@ANCHOR: radioctl_station_modbus_encode_ascii]
/// An ASCII frame: `:`, slave and PDU as upper-case hex, the LRC, CR LF.
pub fn encode_ascii(slave: u8, pdu: &[u8]) -> Vec<u8> {
    let mut raw = vec![slave];
    raw.extend_from_slice(pdu);
    raw.push(lrc(&raw));
    let mut out = vec![b':'];
    for b in raw {
        out.extend_from_slice(format!("{b:02X}").as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    out
}

// [@ANCHOR: radioctl_station_modbus_decode_ascii]
/// Checks an ASCII frame (colon, even hex digits, CR LF, LRC) and returns slave and PDU.
pub fn decode_ascii(frame: &[u8]) -> Result<(u8, Vec<u8>), StationError> {
    let body = frame
        .strip_prefix(b":")
        .and_then(|f| f.strip_suffix(b"\r\n"))
        .ok_or_else(|| driver_err("modbus: not an ASCII frame"))?;
    if body.len() % 2 != 0 || body.len() < 6 {
        return Err(driver_err("modbus: ASCII frame has a bad length"));
    }
    let mut raw = Vec::with_capacity(body.len() / 2);
    for pair in body.chunks(2) {
        let (Some(h), Some(l)) = (hex_val(pair[0]), hex_val(pair[1])) else {
            return Err(driver_err("modbus: ASCII frame has a non-hex digit"));
        };
        raw.push(h << 4 | l);
    }
    let sum = raw.pop().unwrap_or(0);
    if lrc(&raw) != sum {
        return Err(driver_err("modbus: ASCII checksum mismatch"));
    }
    let slave = raw[0];
    Ok((slave, raw[1..].to_vec()))
}

// [@ANCHOR: radioctl_station_modbus_encode_rtu]
/// An RTU frame: slave, PDU, CRC-16 low byte first.
pub fn encode_rtu(slave: u8, pdu: &[u8]) -> Vec<u8> {
    let mut out = vec![slave];
    out.extend_from_slice(pdu);
    let crc = crc16(&out);
    out.extend_from_slice(&crc.to_le_bytes());
    out
}

// [@ANCHOR: radioctl_station_modbus_decode_rtu]
/// Checks an RTU frame's CRC and returns slave and PDU.
pub fn decode_rtu(frame: &[u8]) -> Result<(u8, Vec<u8>), StationError> {
    if frame.len() < 4 {
        return Err(driver_err("modbus: RTU frame too short"));
    }
    let (body, crc) = frame.split_at(frame.len() - 2);
    if crc16(body).to_le_bytes() != [crc[0], crc[1]] {
        return Err(driver_err("modbus: RTU checksum mismatch"));
    }
    Ok((body[0], body[1..].to_vec()))
}

// [@ANCHOR: radioctl_station_modbus_pdu_write_coil]
/// Function 5: write one coil (`FF00` on, `0000` off).
pub fn pdu_write_coil(coil: u16, on: bool) -> Vec<u8> {
    let mut p = vec![FN_WRITE_COIL];
    p.extend_from_slice(&coil.to_be_bytes());
    p.extend_from_slice(if on { &[0xFF, 0x00] } else { &[0x00, 0x00] });
    p
}

// [@ANCHOR: radioctl_station_modbus_pdu_write_coils]
/// Function 15: write `bits.len()` coils from `start`, bit 0 of the first data byte first.
pub fn pdu_write_coils(start: u16, bits: &[bool]) -> Vec<u8> {
    let mut p = vec![FN_WRITE_COILS];
    p.extend_from_slice(&start.to_be_bytes());
    p.extend_from_slice(&(bits.len() as u16).to_be_bytes());
    let mut data = vec![0u8; bits.len().div_ceil(8)];
    for (i, bit) in bits.iter().enumerate() {
        if *bit {
            data[i / 8] |= 1 << (i % 8);
        }
    }
    p.push(data.len() as u8);
    p.extend_from_slice(&data);
    p
}

// [@ANCHOR: radioctl_station_modbus_pdu_read_coils]
/// Function 1: read `count` coils from `start`.
pub fn pdu_read_coils(start: u16, count: u16) -> Vec<u8> {
    let mut p = vec![FN_READ_COILS];
    p.extend_from_slice(&start.to_be_bytes());
    p.extend_from_slice(&count.to_be_bytes());
    p
}

/// A validated reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// A write was echoed as the protocol requires.
    Written,
    /// The coils that were read, in order.
    Coils(Vec<bool>),
}

// [@ANCHOR: radioctl_station_modbus_parse_reply]
/// Checks a reply PDU against the request PDU it answers. Fails closed: an exception code, a
/// different function, a write that is not an exact echo (function 5) or does not echo start and
/// quantity (function 15), or a read with the wrong byte count are all errors.
pub fn parse_reply(request: &[u8], reply: &[u8]) -> Result<Reply, StationError> {
    let (Some(&req_fn), Some(&rep_fn)) = (request.first(), reply.first()) else {
        return Err(driver_err("modbus: empty PDU"));
    };
    if rep_fn == req_fn | 0x80 {
        return Err(driver_err(format!(
            "modbus: exception code {:?} from the board",
            reply.get(1)
        )));
    }
    if rep_fn != req_fn {
        return Err(driver_err("modbus: reply to a different function"));
    }
    match req_fn {
        FN_WRITE_COIL if reply == request => Ok(Reply::Written),
        FN_WRITE_COILS
            if reply.len() == 5 && request.len() >= 5 && reply[1..5] == request[1..5] =>
        {
            Ok(Reply::Written)
        }
        FN_READ_COILS if request.len() == 5 && reply.len() >= 2 => {
            let count = usize::from(u16::from_be_bytes([request[3], request[4]]));
            let bytes = count.div_ceil(8);
            if usize::from(reply[1]) != bytes || reply.len() != 2 + bytes {
                return Err(driver_err("modbus: coil read has the wrong byte count"));
            }
            let bits = (0..count)
                .map(|i| reply[2 + i / 8] >> (i % 8) & 1 == 1)
                .collect();
            Ok(Reply::Coils(bits))
        }
        _ => Err(driver_err("modbus: reply does not match the request")),
    }
}

/// The driver's record of the board: one entry per coil plus the safe-state gate.
#[derive(Debug)]
struct Shadow {
    /// Per coil: the level the board last acknowledged and when (`None`: unknown).
    coils: Vec<Option<(bool, Instant)>>,
    /// Whether the safe state has been applied on the current link.
    safe_done: bool,
    /// Consecutive failed safe-state writes.
    failures: u32,
    /// No safe-state attempt before this time.
    retry_at: Option<Instant>,
}

/// The delay before the next safe-state attempt after `failures` failures in a row.
const BACKOFF_BASE_MS: u64 = 250;
const BACKOFF_MAX_MS: u64 = 8000;

/// A coil relay board on an async byte stream.
///
/// Nothing but a function 5 or 15 write is ever sent for a board with `status_readable = false`,
/// and no command goes out on a link until the board's safe state has been written and
/// acknowledged on it. The driver's record of the coils is updated only by acknowledged writes.
pub struct ModbusRelay<T> {
    board: ModbusBoard,
    io: Mutex<T>,
    shadow: StdMutex<Shadow>,
}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> ModbusRelay<T> {
    // [@ANCHOR: radioctl_station_modbus_relay_new]
    /// A driver for `board` over `io`. Nothing is sent; every coil is unknown, and the first
    /// command (or [`ModbusRelay::ensure_safe_state`]) writes the safe state first.
    pub fn new(board: ModbusBoard, io: T) -> ModbusRelay<T> {
        let n = usize::from(board.coils);
        ModbusRelay {
            board,
            io: Mutex::new(io),
            shadow: StdMutex::new(Shadow {
                coils: vec![None; n],
                safe_done: false,
                failures: 0,
                retry_at: None,
            }),
        }
    }

    fn shadow(&self) -> std::sync::MutexGuard<'_, Shadow> {
        self.shadow.lock().unwrap_or_else(|p| p.into_inner())
    }

    // [@ANCHOR: radioctl_station_modbus_relay_reopen]
    /// Replaces the byte stream after a reconnect or reopen (a USB drop, a power loss, a restart
    /// of the port). The board may be in any relay state and has no watchdog, so every coil
    /// becomes unknown and the safe state is required again before any other command. Nothing is
    /// sent here.
    pub async fn reopen(&self, io: T) {
        let mut g = self.io.lock().await;
        *g = io;
        let mut sh = self.shadow();
        sh.coils.iter_mut().for_each(|c| *c = None);
        sh.safe_done = false;
        sh.failures = 0;
        sh.retry_at = None;
    }

    // [@ANCHOR: radioctl_station_modbus_relay_coil]
    /// The coil address of a line, bounds-checked.
    fn coil(&self, line: u32) -> Result<u16, StationError> {
        if line >= u32::from(self.board.coils) {
            return Err(StationError::new(
                StationErrorKind::Config,
                format!("line {line} is past the board's {} coils", self.board.coils),
            ));
        }
        Ok(self.board.coil_base + line as u16)
    }

    // [@ANCHOR: radioctl_station_modbus_relay_read_frame]
    /// Reads one reply frame: to CR LF (ASCII) or to an idle gap (RTU), bounded in size and time.
    async fn read_frame(&self, io: &mut T) -> Result<Vec<u8>, StationError> {
        let reply_wait = Duration::from_millis(self.board.reply_timeout_ms);
        let gap = Duration::from_millis(self.board.inter_frame_ms.max(1));
        let mut frame = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            let wait = if frame.is_empty() || self.board.framing == Framing::Ascii {
                reply_wait
            } else {
                gap
            };
            match timeout(wait, io.read(&mut byte)).await {
                Ok(Ok(1)) => {}
                Ok(Ok(_)) => return Err(driver_err("modbus: the port closed")),
                Ok(Err(e)) => return Err(driver_err(format!("modbus: read failed: {e}"))),
                Err(_) if self.board.framing == Framing::Rtu && !frame.is_empty() => {
                    return Ok(frame)
                }
                Err(_) => {
                    return Err(StationError::new(
                        StationErrorKind::Timeout,
                        "modbus: no reply",
                    ))
                }
            }
            match self.board.framing {
                Framing::Ascii => {
                    if byte[0] == b':' {
                        frame.clear();
                    }
                    if !frame.is_empty() || byte[0] == b':' {
                        frame.push(byte[0]);
                    }
                    if frame.ends_with(b"\r\n") {
                        return Ok(frame);
                    }
                }
                Framing::Rtu => frame.push(byte[0]),
            }
            if frame.len() > MAX_FRAME {
                return Err(driver_err("modbus: reply too long"));
            }
        }
    }

    // [@ANCHOR: radioctl_station_modbus_relay_exchange]
    /// Sends one request PDU on the locked stream and returns the validated reply. A read coils
    /// request is refused here, below everything else, for a board whose status is not readable:
    /// function 1 is never put on the wire for it.
    async fn exchange(&self, io: &mut T, request: &[u8]) -> Result<Reply, StationError> {
        if !self.board.status_readable && request.first() == Some(&FN_READ_COILS) {
            return Err(driver_err(
                "modbus: this board's status is not readable; read coils is never sent",
            ));
        }
        let frame = match self.board.framing {
            Framing::Ascii => encode_ascii(self.board.slave, request),
            Framing::Rtu => encode_rtu(self.board.slave, request),
        };
        sleep(Duration::from_millis(self.board.inter_frame_ms)).await; // lock-scope-ignore: a serial bus is half duplex; the whole request/reply exchange must be serialized
        io.write_all(&frame)
            .await // lock-scope-ignore: a serial bus is half duplex; the whole request/reply exchange must be serialized
            .map_err(|e| driver_err(format!("modbus: write failed: {e}")))?;
        io.flush()
            .await // lock-scope-ignore: a serial bus is half duplex; the whole request/reply exchange must be serialized
            .map_err(|e| driver_err(format!("modbus: flush failed: {e}")))?;
        let raw = self.read_frame(io).await?; // lock-scope-ignore: a serial bus is half duplex; the whole request/reply exchange must be serialized
        let (slave, pdu) = match self.board.framing {
            Framing::Ascii => decode_ascii(&raw)?,
            Framing::Rtu => decode_rtu(&raw)?,
        };
        if slave != self.board.slave {
            return Err(driver_err("modbus: reply from a different slave"));
        }
        parse_reply(request, &pdu)
    }

    // [@ANCHOR: radioctl_station_modbus_relay_apply_safe]
    /// Writes the board's safe state in ONE function 15 frame over every coil and records it on
    /// an acknowledgement. `respect_backoff` false (the emergency path) ignores the retry delay.
    /// A failed or unacknowledged write leaves every coil unknown, extends the delay and keeps the
    /// gate shut.
    async fn apply_safe(&self, io: &mut T, respect_backoff: bool) -> Result<(), StationError> {
        if respect_backoff {
            if let Some(t) = self.shadow().retry_at {
                if Instant::now() < t {
                    return Err(driver_err(
                        "modbus: the safe state is not applied yet; retrying with backoff",
                    ));
                }
            }
        }
        let bits = self.board.safe_bits();
        let request = pdu_write_coils(self.board.coil_base, &bits);
        let result = self.exchange(io, &request).await;
        let mut sh = self.shadow();
        match result {
            Ok(Reply::Written) => {
                let now = Instant::now();
                for (c, b) in sh.coils.iter_mut().zip(&bits) {
                    *c = Some((*b, now));
                }
                sh.safe_done = true;
                sh.failures = 0;
                sh.retry_at = None;
                Ok(())
            }
            other => {
                sh.coils.iter_mut().for_each(|c| *c = None);
                sh.safe_done = false;
                sh.failures = sh.failures.saturating_add(1);
                let ms = (BACKOFF_BASE_MS << sh.failures.min(6)).min(BACKOFF_MAX_MS);
                sh.retry_at = Some(Instant::now() + Duration::from_millis(ms));
                Err(other.err().unwrap_or_else(|| {
                    driver_err("modbus: the safe-state write was not acknowledged")
                }))
            }
        }
    }

    // [@ANCHOR: radioctl_station_modbus_relay_ensure_safe_state]
    /// Applies the safe state if it has not been applied on this link. A caller that opens the
    /// link calls this first; every command calls it too, so nothing else goes out before it
    /// succeeds. After a failure it returns an error without sending until the backoff passes.
    pub async fn ensure_safe_state(&self) -> Result<(), StationError> {
        let mut io = self.io.lock().await;
        if self.shadow().safe_done {
            return Ok(());
        }
        self.apply_safe(&mut io, true).await // lock-scope-ignore: the safe state must go out before any other exchange on this serial bus
    }

    // [@ANCHOR: radioctl_station_modbus_relay_write_gated]
    /// The one path for a command write: safe state first (when not yet applied), then `request`;
    /// the coils in `affected` (coil index, level) are recorded on an acknowledgement and become
    /// unknown on a timeout, a garbled reply, an exception or any other failure. A lost port also
    /// requires the safe state again.
    async fn write_gated(
        &self,
        request: &[u8],
        affected: &[(usize, bool)],
    ) -> Result<(), StationError> {
        let mut io = self.io.lock().await;
        if !self.shadow().safe_done {
            self.apply_safe(&mut io, true).await?; // lock-scope-ignore: the safe state must go out before any other exchange on this serial bus
        }
        let result = self.exchange(&mut io, request).await; // lock-scope-ignore: a serial bus is half duplex; the whole request/reply exchange must be serialized
        let mut sh = self.shadow();
        match result {
            Ok(Reply::Written) => {
                let now = Instant::now();
                for (i, v) in affected {
                    sh.coils[*i] = Some((*v, now));
                }
                Ok(())
            }
            Ok(_) => {
                for (i, _) in affected {
                    sh.coils[*i] = None;
                }
                Err(driver_err("modbus: write was not acknowledged"))
            }
            Err(e) => {
                for (i, _) in affected {
                    sh.coils[*i] = None;
                }
                if e.detail.contains("port closed") || e.detail.contains("failed:") {
                    sh.safe_done = false;
                }
                Err(e)
            }
        }
    }

    // [@ANCHOR: radioctl_station_modbus_relay_write_all]
    /// Function 15 over every coil: all on or all off in one frame, behind the safe-state gate.
    pub async fn write_all(&self, on: bool) -> Result<(), StationError> {
        let n = usize::from(self.board.coils);
        let bits = vec![on; n];
        let affected: Vec<(usize, bool)> = (0..n).map(|i| (i, on)).collect();
        self.write_gated(&pdu_write_coils(self.board.coil_base, &bits), &affected)
            .await
    }

    // [@ANCHOR: radioctl_station_modbus_relay_record]
    /// The driver's record of every coil in order: the level the board last acknowledged and its
    /// age, or unknown. Recorded, not confirmed.
    pub fn record(&self) -> Vec<Recorded> {
        self.shadow()
            .coils
            .iter()
            .map(|c| Recorded {
                value: c.map(|(v, _)| v),
                age: c.map(|(_, t)| t.elapsed()),
            })
            .collect()
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> ChannelDriver for ModbusRelay<T> {
    // [@ANCHOR: radioctl_station_modbus_relay_assurance]
    /// [`Assurance::Recorded`] for a board whose status is not readable, otherwise
    /// [`Assurance::HostSupervised`].
    fn assurance(&self) -> Assurance {
        if self.board.status_readable {
            Assurance::HostSupervised
        } else {
            Assurance::Recorded
        }
    }

    // [@ANCHOR: radioctl_station_modbus_relay_write_line]
    /// Function 5 on the line's coil, behind the safe-state gate.
    fn write_line(&self, line: u32, on: bool) -> BoxFuture<'_, Result<(), StationError>> {
        Box::pin(async move {
            let coil = self.coil(line)?;
            self.write_gated(&pdu_write_coil(coil, on), &[(line as usize, on)])
                .await
        })
    }

    // [@ANCHOR: radioctl_station_modbus_relay_read_line]
    /// Without readable status: the driver's record of the line, with no I/O (an error while it is
    /// unknown). With readable status: function 1 on the line's coil.
    fn read_line(&self, line: u32) -> BoxFuture<'_, Result<bool, StationError>> {
        Box::pin(async move {
            let coil = self.coil(line)?;
            if !self.board.status_readable {
                return self
                    .recorded_line(line)
                    .and_then(|r| r.value)
                    .ok_or_else(|| {
                        driver_err("modbus: the recorded state of this line is unknown")
                    });
            }
            let mut io = self.io.lock().await;
            if !self.shadow().safe_done {
                self.apply_safe(&mut io, true).await?; // lock-scope-ignore: the safe state must go out before any other exchange on this serial bus
            }
            match self.exchange(&mut io, &pdu_read_coils(coil, 1)).await? {
                // lock-scope-ignore: a serial bus is half duplex; the whole request/reply exchange must be serialized
                Reply::Coils(bits) if bits.len() == 1 => Ok(bits[0]),
                _ => Err(driver_err("modbus: unexpected read reply")),
            }
        })
    }

    // [@ANCHOR: radioctl_station_modbus_relay_can_read_back]
    /// Whether the board's status is readable (never for a recorded-only board).
    fn can_read_back(&self) -> bool {
        self.board.status_readable
    }

    // [@ANCHOR: radioctl_station_modbus_relay_safe_line]
    /// The board's safe level for a line (`safe_state`, off when empty).
    fn safe_line(&self, line: u32) -> bool {
        self.board
            .safe_bits()
            .get(line as usize)
            .copied()
            .unwrap_or(false)
    }

    // [@ANCHOR: radioctl_station_modbus_relay_force_safe]
    /// The emergency path: the board's whole safe state in one frame, ignoring the retry delay
    /// and the minimum times. Never blocked.
    fn force_safe(&self, line: u32) -> BoxFuture<'_, Result<(), StationError>> {
        Box::pin(async move {
            self.coil(line)?;
            let mut io = self.io.lock().await;
            self.apply_safe(&mut io, false).await // lock-scope-ignore: the emergency safe-state write owns the serial bus
        })
    }

    // [@ANCHOR: radioctl_station_modbus_relay_recorded_line]
    /// The record of one line: recorded, not confirmed.
    fn recorded_line(&self, line: u32) -> Option<Recorded> {
        self.record().get(line as usize).copied()
    }
}

/// A virtual Modbus ASCII module on an in-memory stream, for tests: no hardware, no serial port.
#[cfg(test)]
pub(crate) mod test_peer {
    use super::*;
    use std::sync::Arc;
    use tokio::io::{duplex, DuplexStream};

    /// How the virtual module answers.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Mode {
        Good,
        Silent,
        Garbage,
        Exception,
    }

    /// The module's observable side.
    #[derive(Clone)]
    pub(crate) struct Module {
        pub(crate) mode: Arc<StdMutex<Mode>>,
        pub(crate) coils: Arc<StdMutex<Vec<bool>>>,
        /// Every request frame received, as text without the CR LF.
        pub(crate) log: Arc<StdMutex<Vec<String>>>,
    }

    impl Module {
        pub(crate) fn frames(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
        pub(crate) fn set(&self, m: Mode) {
            *self.mode.lock().unwrap() = m;
        }
    }

    /// Starts a virtual 16-coil module; returns the host's end of the stream.
    pub(crate) fn spawn(coils_at_start: Vec<bool>) -> (DuplexStream, Module) {
        let (host, mut peer) = duplex(4096);
        let m = Module {
            mode: Arc::new(StdMutex::new(Mode::Good)),
            coils: Arc::new(StdMutex::new(coils_at_start)),
            log: Arc::new(StdMutex::new(Vec::new())),
        };
        let mm = m.clone();
        tokio::spawn(async move {
            let mut buf = Vec::new();
            let mut byte = [0u8; 1];
            while peer.read(&mut byte).await.map(|n| n == 1).unwrap_or(false) {
                buf.push(byte[0]);
                if !buf.ends_with(b"\r\n") {
                    continue;
                }
                let frame = std::mem::take(&mut buf);
                mm.log
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&frame[..frame.len() - 2]).into());
                let Ok((slave, pdu)) = decode_ascii(&frame) else {
                    continue;
                };
                let mode = *mm.mode.lock().unwrap();
                if mode == Mode::Silent {
                    continue;
                }
                let reply = match (mode, pdu[0]) {
                    (Mode::Garbage, _) => {
                        let _ = peer.write_all(b"hello\r\n").await;
                        continue;
                    }
                    (Mode::Exception, f) => vec![f | 0x80, 2],
                    (_, FN_WRITE_COIL) => {
                        let on = pdu[3] == 0xFF;
                        mm.coils.lock().unwrap()[usize::from(pdu[2])] = on;
                        pdu.clone()
                    }
                    (_, FN_WRITE_COILS) => {
                        let n = usize::from(pdu[4]);
                        let mut c = mm.coils.lock().unwrap();
                        for i in 0..n {
                            c[usize::from(pdu[2]) + i] = pdu[6 + i / 8] >> (i % 8) & 1 == 1;
                        }
                        pdu[..5].to_vec()
                    }
                    (_, FN_READ_COILS) => {
                        let n = usize::from(pdu[4]);
                        let c = mm.coils.lock().unwrap();
                        let mut r = vec![FN_READ_COILS, n.div_ceil(8) as u8];
                        let mut data = vec![0u8; n.div_ceil(8)];
                        for i in 0..n {
                            if c[usize::from(pdu[2]) + i] {
                                data[i / 8] |= 1 << (i % 8);
                            }
                        }
                        r.extend(data);
                        r
                    }
                    _ => vec![pdu[0] | 0x80, 1],
                };
                let _ = peer.write_all(&encode_ascii(slave, &reply)).await;
            }
        });
        (host, m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Frames from the proposal's decode of the SainSmart example, with the LRC recomputed. The
    // vendor table's channel 15 off (FF), channel 16 off (FE) and on (FF) are wrong; these are
    // the correct values, worked out by hand from the byte sums.
    const CH1_ON: &[u8] = b":FE050000FF00FE\r\n";
    const CH1_OFF: &[u8] = b":FE0500000000FD\r\n";
    const CH15_ON: &[u8] = b":FE05000EFF00F0\r\n";
    const CH15_OFF: &[u8] = b":FE05000E0000EF\r\n";
    const CH16_ON: &[u8] = b":FE05000FFF00EF\r\n";
    const CH16_OFF: &[u8] = b":FE05000F0000EE\r\n";
    const ALL_OFF: &[u8] = b":FE0F00000010020000E1\r\n";
    const ALL_ON: &[u8] = b":FE0F0000001002FFFFE3\r\n";
    const READ_ALL: &[u8] = b":FE0100000010F1\r\n";

    // Tests [@ANCHOR: radioctl_station_modbus_lrc]
    // Tests [@ANCHOR: radioctl_station_modbus_encode_ascii]
    // Tests [@ANCHOR: radioctl_station_modbus_pdu_write_coil]
    // Tests [@ANCHOR: radioctl_station_modbus_pdu_write_coils]
    // Tests [@ANCHOR: radioctl_station_modbus_pdu_read_coils]
    #[test]
    fn the_sainsmart_frames_are_computed_not_copied() {
        let enc = |pdu: Vec<u8>| encode_ascii(0xFE, &pdu);
        assert_eq!(enc(pdu_write_coil(0, true)), CH1_ON);
        assert_eq!(enc(pdu_write_coil(0, false)), CH1_OFF);
        assert_eq!(enc(pdu_write_coil(14, true)), CH15_ON);
        assert_eq!(enc(pdu_write_coil(14, false)), CH15_OFF);
        assert_eq!(enc(pdu_write_coil(15, true)), CH16_ON);
        assert_eq!(enc(pdu_write_coil(15, false)), CH16_OFF);
        assert_eq!(enc(pdu_write_coils(0, &[false; 16])), ALL_OFF);
        assert_eq!(enc(pdu_write_coils(0, &[true; 16])), ALL_ON);
        assert_eq!(enc(pdu_read_coils(0, 16)), READ_ALL);
        // the vendor's wrong entries do not decode
        assert!(decode_ascii(b":FE05000EFF00FF\r\n").is_err());
        assert!(decode_ascii(b":FE0100200000FF\r\n").is_err());
        assert_eq!(lrc(&[0xFE, 0x01, 0x00, 0x00, 0x00, 0x10]), 0xF1);
        // 9 coils pack into two bytes, LSB first
        let p = pdu_write_coils(
            2,
            &[true, false, false, false, false, false, false, false, true],
        );
        assert_eq!(p, vec![0x0F, 0, 2, 0, 9, 2, 0x01, 0x01]);
    }

    // Tests [@ANCHOR: radioctl_station_modbus_decode_ascii]
    // Tests [@ANCHOR: radioctl_station_modbus_hex_val]
    #[test]
    fn ascii_frames_are_checked() {
        assert_eq!(
            decode_ascii(CH1_ON).unwrap(),
            (0xFE, vec![5, 0, 0, 0xFF, 0])
        );
        assert_eq!(decode_ascii(b":fe050000ff00fe\r\n").unwrap().0, 0xFE);
        for bad in [
            &b"FE050000FF00FE\r\n"[..],
            b":FE050000FF00FE",
            b":FE050000FF00FEX\r\n",
            b":FE050000FF00FF\r\n",
            b":FE05G000FF00FE\r\n",
            b":FE\r\n",
            b"",
        ] {
            assert!(decode_ascii(bad).is_err(), "{bad:?}");
        }
    }

    // Tests [@ANCHOR: radioctl_station_modbus_crc16]
    // Tests [@ANCHOR: radioctl_station_modbus_encode_rtu]
    // Tests [@ANCHOR: radioctl_station_modbus_decode_rtu]
    #[test]
    fn rtu_frames_carry_a_crc() {
        // Function 5, slave 1, coil 10 on: the CRC bytes are AC 38 on the wire.
        let frame = encode_rtu(1, &pdu_write_coil(10, true));
        assert_eq!(frame, vec![0x01, 0x05, 0x00, 0x0A, 0xFF, 0x00, 0xAC, 0x38]);
        assert_eq!(decode_rtu(&frame).unwrap(), (1, pdu_write_coil(10, true)));
        let mut bad = frame.clone();
        bad[6] ^= 1;
        assert!(decode_rtu(&bad).is_err());
        assert!(decode_rtu(&[1, 2]).is_err());
        assert_eq!(crc16(&[0x01, 0x01, 0x00, 0x00, 0x00, 0x10]), 0xC63D);
    }

    // Tests [@ANCHOR: radioctl_station_modbus_parse_reply]
    #[test]
    fn unknown_replies_fail_closed() {
        let w = pdu_write_coil(3, true);
        assert_eq!(parse_reply(&w, &w).unwrap(), Reply::Written);
        assert!(parse_reply(&w, &pdu_write_coil(3, false)).is_err());
        assert!(parse_reply(&w, &[0x85, 0x02])
            .unwrap_err()
            .detail
            .contains("exception"));
        assert!(parse_reply(&w, &[0x06, 0, 3, 0xFF, 0]).is_err());
        assert!(parse_reply(&w, &[]).is_err());
        let wm = pdu_write_coils(0, &[true; 16]);
        assert_eq!(
            parse_reply(&wm, &[0x0F, 0, 0, 0, 16]).unwrap(),
            Reply::Written
        );
        assert!(parse_reply(&wm, &[0x0F, 0, 0, 0, 8]).is_err());
        let r = pdu_read_coils(0, 10);
        assert_eq!(
            parse_reply(&r, &[0x01, 2, 0b0000_0101, 0b10]).unwrap(),
            Reply::Coils(vec![
                true, false, true, false, false, false, false, false, false, true
            ])
        );
        assert!(parse_reply(&r, &[0x01, 1, 5]).is_err());
        assert!(parse_reply(&r, &[0x01, 3, 5, 0]).is_err());
        assert!(parse_reply(&[0x2B], &[0x2B]).is_err());
    }

    // Tests [@ANCHOR: radioctl_station_modbus_sainsmart]
    // Tests [@ANCHOR: radioctl_station_modbus_from_toml]
    #[test]
    fn boards_are_description_data() {
        let b = ModbusBoard::sainsmart_16ch();
        let text = toml::to_string(&b).unwrap();
        assert_eq!(ModbusBoard::from_toml(&text).unwrap(), b);
        assert!(!b.verified);
        assert!(ModbusBoard::from_toml(&text.replace("coils = 16", "coils = 0")).is_err());
        assert!(ModbusBoard::from_toml("name = 1").is_err());
        let rtu = text
            .replace("framing = \"ascii\"", "framing = \"rtu\"")
            .replace("slave = 254", "slave = 7");
        let rtu = ModbusBoard::from_toml(&rtu).unwrap();
        assert_eq!((rtu.framing, rtu.slave), (Framing::Rtu, 7));
    }

    use super::test_peer::{spawn, Mode};
    use crate::channel::Assurance;
    use crate::gpio_map::BoardCatalog;

    fn fast_board() -> ModbusBoard {
        let mut b = ModbusBoard::sainsmart_16ch();
        b.reply_timeout_ms = 50;
        b.inter_frame_ms = 1;
        b
    }

    // Tests [@ANCHOR: radioctl_station_modbus_relay_ensure_safe_state]
    // Tests [@ANCHOR: radioctl_station_modbus_relay_apply_safe]
    // Tests [@ANCHOR: radioctl_station_modbus_relay_write_gated]
    // Tests [@ANCHOR: radioctl_station_modbus_relay_exchange]
    // Tests [@ANCHOR: radioctl_station_modbus_safe_bits]
    #[tokio::test(start_paused = true)]
    async fn the_safe_state_is_the_first_frame_and_nothing_goes_before_it() {
        // the board powers up with relays 1 and 16 pulled in
        let mut start = vec![false; 16];
        start[0] = true;
        start[15] = true;
        let (io, module) = spawn(start);
        let relay = ModbusRelay::new(fast_board(), io);
        assert!(module.frames().is_empty(), "new sends nothing");
        assert!(relay.record().iter().all(|r| r.value.is_none()));
        relay.write_line(3, true).await.unwrap();
        let f = module.frames();
        assert_eq!(f[0], ":FE0F00000010020000E1", "safe state first, one frame");
        assert_eq!(f[1], ":FE05000 3FF00FB".replace(' ', ""));
        assert_eq!(f.len(), 2);
        assert!(!module.coils.lock().unwrap()[0]);
        assert!(!module.coils.lock().unwrap()[15]);
        // once applied it is not repeated
        relay.write_line(4, true).await.unwrap();
        assert_eq!(module.frames().len(), 3);
        // a custom safe state (some channels on) is one frame too
        let mut b = fast_board();
        b.safe_state = vec![false; 16];
        b.safe_state[5] = true;
        let (io, module) = spawn(vec![false; 16]);
        let relay = ModbusRelay::new(b, io);
        relay.ensure_safe_state().await.unwrap();
        relay.ensure_safe_state().await.unwrap();
        assert_eq!(module.frames(), vec![":FE0F00000010022000C1".to_string()]);
        assert_eq!(relay.record()[5].value, Some(true));
        assert!(!relay.record()[0].value.unwrap());
        assert!(relay.record()[0].age.is_some());
    }

    // Tests [@ANCHOR: radioctl_station_modbus_relay_reopen]
    #[tokio::test(start_paused = true)]
    async fn every_reopen_applies_the_safe_state_again_before_anything_else() {
        let (io, module) = spawn(vec![false; 16]);
        let relay = ModbusRelay::new(fast_board(), io);
        relay.write_line(0, true).await.unwrap();
        assert_eq!(relay.record()[0].value, Some(true));
        // USB drop and power loss: new stream, new module with any relay state
        let (io2, module2) = spawn(vec![true; 16]);
        relay.reopen(io2).await;
        assert!(module2.frames().is_empty());
        assert!(relay.record().iter().all(|r| r.value.is_none()));
        relay.write_line(7, true).await.unwrap();
        let f = module2.frames();
        assert_eq!(f[0], ":FE0F00000010020000E1");
        assert_eq!(f.len(), 2);
        assert!(!module2.coils.lock().unwrap()[0]);
        assert_eq!(module.frames().len(), 2);
    }

    // Tests [@ANCHOR: radioctl_station_modbus_relay_apply_safe]
    #[tokio::test(start_paused = true)]
    async fn a_failed_safe_state_leaves_unknown_and_blocks_everything_with_backoff() {
        let (io, module) = spawn(vec![false; 16]);
        let relay = ModbusRelay::new(fast_board(), io);
        relay.write_line(0, true).await.unwrap();
        let (io2, module2) = spawn(vec![true; 16]);
        relay.reopen(io2).await;
        module2.set(Mode::Silent);
        assert!(relay.write_line(1, true).await.is_err());
        assert_eq!(module2.frames().len(), 1, "only the safe state was tried");
        assert!(relay.record().iter().all(|r| r.value.is_none()));
        // inside the backoff nothing is sent at all
        module2.set(Mode::Good);
        let e = relay.write_line(1, true).await.unwrap_err();
        assert!(e.detail.contains("backoff"), "{e}");
        assert!(relay.ensure_safe_state().await.is_err());
        assert_eq!(module2.frames().len(), 1);
        assert!(relay.read_line(1).await.is_err());
        // after the backoff the safe state goes first, then the command
        sleep(Duration::from_secs(9)).await;
        relay.write_line(1, true).await.unwrap();
        let f = module2.frames();
        assert_eq!(f.len(), 3);
        assert_eq!(f[1], ":FE0F00000010020000E1");
        assert_eq!(f[2], ":FE05000 1FF00FD".replace(' ', ""));
        // a garbled acknowledgement of the safe state is a failure too
        let (io3, module3) = spawn(vec![false; 16]);
        relay.reopen(io3).await;
        module3.set(Mode::Garbage);
        assert!(relay.ensure_safe_state().await.is_err());
        assert!(relay.record().iter().all(|r| r.value.is_none()));
        drop(module);
    }

    // Tests [@ANCHOR: radioctl_station_modbus_relay_read_line]
    // Tests [@ANCHOR: radioctl_station_modbus_relay_can_read_back]
    // Tests [@ANCHOR: radioctl_station_modbus_relay_assurance]
    // Tests [@ANCHOR: radioctl_station_modbus_relay_recorded_line]
    // Tests [@ANCHOR: radioctl_station_modbus_relay_record]
    #[tokio::test(start_paused = true)]
    async fn an_unreadable_board_is_recorded_and_read_coils_is_never_sent() {
        let (io, module) = spawn(vec![false; 16]);
        let relay = ModbusRelay::new(fast_board(), io);
        assert_eq!(relay.assurance(), Assurance::Recorded);
        assert!(!relay.can_read_back());
        // unknown before the first acknowledged write, and no I/O to find out
        assert!(relay.read_line(2).await.is_err());
        assert!(module.frames().is_empty());
        relay.write_line(2, true).await.unwrap();
        let n = module.frames().len();
        assert!(relay.read_line(2).await.unwrap());
        assert!(!relay.read_line(3).await.unwrap());
        assert_eq!(module.frames().len(), n, "read_line did no I/O");
        let r = relay.recorded_line(2).unwrap();
        assert_eq!(r.value, Some(true));
        assert!(r.age.is_some());
        // even asked directly, function 1 is refused below everything and never on the wire
        let mut io = relay.io.lock().await;
        assert!(relay
            .exchange(&mut io, &pdu_read_coils(0, 16))
            .await
            .is_err());
        drop(io);
        assert!(module.frames().iter().all(|f| &f[3..5] != "01"));
        // a readable board does read, and says HostSupervised
        let mut b = fast_board();
        b.status_readable = true;
        let (io, module) = spawn(vec![false; 16]);
        let relay = ModbusRelay::new(b, io);
        assert_eq!(relay.assurance(), Assurance::HostSupervised);
        assert!(relay.can_read_back());
        relay.write_line(2, true).await.unwrap();
        assert!(relay.read_line(2).await.unwrap());
        assert!(module.frames().iter().any(|f| &f[3..5] == "01"));
    }

    // Tests [@ANCHOR: radioctl_station_modbus_relay_write_line]
    // Tests [@ANCHOR: radioctl_station_modbus_relay_write_all]
    #[tokio::test(start_paused = true)]
    async fn the_record_changes_only_on_acknowledged_writes_and_a_failure_makes_it_unknown() {
        let (io, module) = spawn(vec![false; 16]);
        let relay = ModbusRelay::new(fast_board(), io);
        relay.write_line(2, true).await.unwrap();
        relay.write_line(3, true).await.unwrap();
        assert_eq!(relay.record()[2].value, Some(true));
        // no reply: the coil is unknown, the others keep their record
        module.set(Mode::Silent);
        let e = relay.write_line(2, false).await.unwrap_err();
        assert_eq!(e.kind, StationErrorKind::Timeout);
        assert_eq!(relay.record()[2].value, None);
        assert_eq!(relay.record()[2].age, None);
        assert_eq!(relay.record()[3].value, Some(true));
        assert_eq!(relay.record()[4].value, Some(false));
        // garbled reply, then an exception: same
        for m in [Mode::Garbage, Mode::Exception] {
            module.set(Mode::Good);
            relay.write_line(3, true).await.unwrap();
            assert_eq!(relay.record()[3].value, Some(true));
            module.set(m);
            assert!(relay.write_line(3, false).await.is_err());
            assert_eq!(relay.record()[3].value, None, "{m:?}");
        }
        // an out-of-range line sends nothing and changes nothing
        let n = module.frames().len();
        module.set(Mode::Good);
        assert!(relay.write_line(16, true).await.is_err());
        assert_eq!(module.frames().len(), n);
        // a failed write_all makes every coil unknown, a good one records all
        module.set(Mode::Silent);
        assert!(relay.write_all(true).await.is_err());
        assert!(relay.record().iter().all(|r| r.value.is_none()));
        module.set(Mode::Good);
        relay.write_all(true).await.unwrap();
        assert!(relay.record().iter().all(|r| r.value == Some(true)));
    }

    // Tests [@ANCHOR: radioctl_station_modbus_relay_safe_line]
    // Tests [@ANCHOR: radioctl_station_modbus_relay_force_safe]
    #[tokio::test(start_paused = true)]
    async fn the_emergency_path_uses_the_safe_state_and_ignores_the_backoff() {
        let mut b = fast_board();
        b.safe_state = vec![false; 16];
        b.safe_state[9] = true;
        let (io, module) = spawn(vec![true; 16]);
        let relay = ModbusRelay::new(b, io);
        assert!(relay.safe_line(9));
        assert!(!relay.safe_line(0));
        module.set(Mode::Silent);
        assert!(relay.ensure_safe_state().await.is_err()); // backoff now running
        module.set(Mode::Good);
        relay.force_safe(0).await.unwrap(); // not held back
        let c = module.coils.lock().unwrap().clone();
        assert!(c[9] && !c[0] && !c[15]);
        assert_eq!(relay.record()[9].value, Some(true));
        assert!(relay.force_safe(16).await.is_err());
    }

    // Tests [@ANCHOR: radioctl_station_catalog_builtin]
    // Tests [@ANCHOR: radioctl_station_catalog_get]
    // Tests [@ANCHOR: radioctl_station_catalog_from_toml]
    #[test]
    fn boards_toml_says_the_sainsmart_status_is_not_readable() {
        let cat = BoardCatalog::builtin();
        let sm = cat.get("sainsmart-16ch-modbus").expect("listed");
        let b = ModbusBoard::sainsmart_16ch();
        assert!(!sm.status_readable);
        assert_eq!(sm.safe_state, b.safe_state);
        assert_eq!(sm.pins.len(), usize::from(b.coils));
        assert!(!b.status_readable);
        // other boards keep the default
        assert!(cat.get("shelly-gen2-1ch").unwrap().status_readable);
        assert!(cat.get("shelly-gen2-1ch").unwrap().safe_state.is_empty());
        // a Modbus description: default readable, all off; the old key still parses
        let text = toml::to_string(&b).unwrap();
        let plain = text
            .lines()
            .filter(|l| !l.starts_with("status_readable") && !l.starts_with("safe_state"))
            .collect::<Vec<_>>()
            .join("\n");
        let d = ModbusBoard::from_toml(&plain).unwrap();
        assert!(d.status_readable);
        assert_eq!(d.safe_bits(), vec![false; 16]);
        let old = format!("{plain}\nread_back = false");
        assert!(!ModbusBoard::from_toml(&old).unwrap().status_readable);
        let bad = text.replace("safe_state = [", "safe_state = [true, ");
        assert!(ModbusBoard::from_toml(&bad).is_err());
    }
}
