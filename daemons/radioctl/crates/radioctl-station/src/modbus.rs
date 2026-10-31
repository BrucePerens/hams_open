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

use crate::channel::{BoxFuture, ChannelDriver};
use crate::error::{StationError, StationErrorKind};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout};

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
    /// Whether function 1 (read coils) is used for read-back. The coil bits are the commanded
    /// relay states, not a measurement of the contacts.
    pub read_back: bool,
    /// Set to true once checked on a real module.
    #[serde(default)]
    pub verified: bool,
}

impl ModbusBoard {
    // [@ANCHOR: radioctl_station_modbus_sainsmart]
    /// The SainSmart 16-channel 9-36 V USB relay module (sku 101-70-208): Modbus ASCII, 9600
    /// baud, slave 254, 16 coils from 0. Unverified on hardware.
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
            read_back: true,
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

/// A coil relay board on an async byte stream.
pub struct ModbusRelay<T> {
    board: ModbusBoard,
    io: Mutex<T>,
}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> ModbusRelay<T> {
    // [@ANCHOR: radioctl_station_modbus_relay_new]
    /// A driver for `board` over `io`. Nothing is sent.
    pub fn new(board: ModbusBoard, io: T) -> ModbusRelay<T> {
        ModbusRelay {
            board,
            io: Mutex::new(io),
        }
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

    // [@ANCHOR: radioctl_station_modbus_relay_transact]
    /// Sends one request PDU and returns the validated reply.
    pub async fn transact(&self, request: &[u8]) -> Result<Reply, StationError> {
        let frame = match self.board.framing {
            Framing::Ascii => encode_ascii(self.board.slave, request),
            Framing::Rtu => encode_rtu(self.board.slave, request),
        };
        let mut io = self.io.lock().await;
        sleep(Duration::from_millis(self.board.inter_frame_ms)).await; // lock-scope-ignore: a serial bus is half duplex; the whole request/reply exchange must be serialized
        io.write_all(&frame)
            .await // lock-scope-ignore: a serial bus is half duplex; the whole request/reply exchange must be serialized
            .map_err(|e| driver_err(format!("modbus: write failed: {e}")))?;
        io.flush()
            .await // lock-scope-ignore: a serial bus is half duplex; the whole request/reply exchange must be serialized
            .map_err(|e| driver_err(format!("modbus: flush failed: {e}")))?;
        let raw = self.read_frame(&mut io).await?; // lock-scope-ignore: a serial bus is half duplex; the whole request/reply exchange must be serialized
        let (slave, pdu) = match self.board.framing {
            Framing::Ascii => decode_ascii(&raw)?,
            Framing::Rtu => decode_rtu(&raw)?,
        };
        if slave != self.board.slave {
            return Err(driver_err("modbus: reply from a different slave"));
        }
        parse_reply(request, &pdu)
    }

    // [@ANCHOR: radioctl_station_modbus_relay_write_all]
    /// Function 15 over every coil: all on or all off in one frame.
    pub async fn write_all(&self, on: bool) -> Result<(), StationError> {
        let bits = vec![on; usize::from(self.board.coils)];
        self.transact(&pdu_write_coils(self.board.coil_base, &bits))
            .await
            .map(|_| ())
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> ChannelDriver for ModbusRelay<T> {
    // [@ANCHOR: radioctl_station_modbus_relay_write_line]
    /// Function 5 on the line's coil.
    fn write_line(&self, line: u32, on: bool) -> BoxFuture<'_, Result<(), StationError>> {
        Box::pin(async move {
            let coil = self.coil(line)?;
            self.transact(&pdu_write_coil(coil, on)).await.map(|_| ())
        })
    }

    // [@ANCHOR: radioctl_station_modbus_relay_read_line]
    /// Function 1 on the line's coil.
    fn read_line(&self, line: u32) -> BoxFuture<'_, Result<bool, StationError>> {
        Box::pin(async move {
            if !self.board.read_back {
                return Err(driver_err("modbus: this board is not read back"));
            }
            let coil = self.coil(line)?;
            match self.transact(&pdu_read_coils(coil, 1)).await? {
                Reply::Coils(bits) if bits.len() == 1 => Ok(bits[0]),
                _ => Err(driver_err("modbus: unexpected read reply")),
            }
        })
    }

    // [@ANCHOR: radioctl_station_modbus_relay_can_read_back]
    /// Whether the board description reads coils back.
    fn can_read_back(&self) -> bool {
        self.board.read_back
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
}
