// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Modbus coil relay driver against a virtual SainSmart-style module on a pty. The virtual
//! module is written here independently of the driver: it parses hex itself and checks the LRC
//! with its own arithmetic.
#![allow(clippy::expect_used, clippy::panic)]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use radioctl_core::serial::{open_serial, LineControl, PortTarget, SerialConfig};
use radioctl_station::channel::ChannelDriver;
use radioctl_station::modbus::{ModbusBoard, ModbusRelay};
use serialport::{SerialPort, TTYPort};
use tokio::runtime::Handle;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Good,
    BadLrc,
    Garbage,
    Exception,
}

fn hex(b: &[u8]) -> Vec<u8> {
    b.iter()
        .flat_map(|x| format!("{x:02X}").into_bytes())
        .collect()
}

fn lrc(b: &[u8]) -> u8 {
    (0u8).wrapping_sub(b.iter().fold(0u8, |a, x| a.wrapping_add(*x)))
}

/// Runs the virtual module on the master side; returns the coil state and the request log.
fn module(
    mut master: TTYPort,
    mode: Arc<Mutex<Mode>>,
) -> (Arc<Mutex<u16>>, Arc<Mutex<Vec<String>>>) {
    let coils = Arc::new(Mutex::new(0u16));
    let log = Arc::new(Mutex::new(Vec::new()));
    let (c, l) = (Arc::clone(&coils), Arc::clone(&log));
    master
        .set_timeout(Duration::from_millis(50))
        .expect("timeout");
    std::thread::spawn(move || {
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            match master.read(&mut byte) {
                Ok(1) => line.push(byte[0]),
                _ => continue,
            }
            if !line.ends_with(b"\r\n") {
                continue;
            }
            let text = String::from_utf8_lossy(&line).to_string();
            line.clear();
            l.lock().expect("log").push(text.trim().to_string());
            let digits = text.trim().trim_start_matches(':');
            let raw: Vec<u8> = (0..digits.len() / 2)
                .map(|i| u8::from_str_radix(&digits[2 * i..2 * i + 2], 16).unwrap_or(0))
                .collect();
            let (body, sum) = raw.split_at(raw.len() - 1);
            if lrc(body) != sum[0] || body[0] != 0xFE {
                continue; // a bad request gets no answer
            }
            let mut st = c.lock().expect("coils");
            let pdu = &body[1..];
            let mut reply: Vec<u8> = match pdu[0] {
                0x05 => {
                    let coil = u16::from_be_bytes([pdu[1], pdu[2]]);
                    if pdu[3] == 0xFF {
                        *st |= 1 << coil;
                    } else {
                        *st &= !(1 << coil);
                    }
                    pdu.to_vec()
                }
                0x0F => {
                    *st = u16::from_le_bytes([pdu[6], pdu[7]]);
                    pdu[..5].to_vec()
                }
                0x01 => {
                    let n = u16::from_be_bytes([pdu[3], pdu[4]]);
                    let start = u16::from_be_bytes([pdu[1], pdu[2]]);
                    let bits = (*st >> start) & ((1u32 << n) - 1) as u16;
                    let mut r = vec![0x01, n.div_ceil(8) as u8];
                    r.extend_from_slice(&bits.to_le_bytes()[..usize::from(n.div_ceil(8))]);
                    r
                }
                _ => vec![pdu[0] | 0x80, 1],
            };
            let m = *mode.lock().expect("mode");
            if m == Mode::Exception {
                reply = vec![pdu[0] | 0x80, 2];
            }
            let mut framed = vec![0xFE];
            framed.extend_from_slice(&reply);
            let sum = if m == Mode::BadLrc {
                lrc(&framed).wrapping_add(1)
            } else {
                lrc(&framed)
            };
            framed.push(sum);
            let mut out = vec![b':'];
            out.extend(hex(&framed));
            out.extend_from_slice(b"\r\n");
            if m == Mode::Garbage {
                out = b"hello\r\n".to_vec();
            }
            let _ = master.write_all(&out);
        }
    });
    (coils, log)
}

// Tests [@ANCHOR: radioctl_station_modbus_relay_new]
// Tests [@ANCHOR: radioctl_station_modbus_relay_transact]
// Tests [@ANCHOR: radioctl_station_modbus_relay_write_all]
// Tests [@ANCHOR: radioctl_station_modbus_relay_write_line]
// Tests [@ANCHOR: radioctl_station_modbus_relay_read_line]
// Tests [@ANCHOR: radioctl_station_modbus_relay_can_read_back]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_driver_talks_to_a_virtual_module_over_a_pty() {
    let (master, slave) = TTYPort::pair().expect("pty");
    let path = PathBuf::from(slave.name().expect("name"));
    drop(slave);
    let mode = Arc::new(Mutex::new(Mode::Good));
    let (coils, log) = module(master, Arc::clone(&mode));
    let mut config = SerialConfig::new(PortTarget::Path(path), 9600);
    config.lines = LineControl::Unmanaged;
    let link = open_serial(&Handle::current(), config).await.expect("open");
    let mut board = ModbusBoard::sainsmart_16ch();
    board.reply_timeout_ms = 300;
    let relay = ModbusRelay::new(board, link.transport);
    assert!(relay.can_read_back());

    relay.write_line(0, true).await.expect("ch1 on");
    relay.write_line(14, true).await.expect("ch15 on");
    relay.write_line(15, true).await.expect("ch16 on");
    assert_eq!(*coils.lock().expect("c"), 0b1100_0000_0000_0001);
    assert!(relay.read_line(15).await.expect("read"));
    assert!(!relay.read_line(1).await.expect("read"));
    relay.write_line(14, false).await.expect("ch15 off");
    assert!(!relay.read_line(14).await.expect("read"));
    {
        let log = log.lock().expect("log");
        assert_eq!(log[0], ":FE050000FF00FE");
        assert_eq!(log[1], ":FE05000EFF00F0");
        assert_eq!(log[2], ":FE05000FFF00EF");
        assert!(log.contains(&":FE05000E0000EF".to_string()));
        assert!(log.contains(&":FE01000F0001F1".to_string()));
    }
    relay.write_all(false).await.expect("all off");
    assert_eq!(*coils.lock().expect("c"), 0);
    assert!(relay.write_line(16, true).await.is_err());

    // fail closed on everything else
    for (m, want) in [
        (Mode::BadLrc, "checksum"),
        (Mode::Garbage, ""),
        (Mode::Exception, "exception"),
    ] {
        *mode.lock().expect("m") = m;
        let e = relay.write_line(3, true).await.expect_err("must fail");
        assert!(e.detail.contains(want), "{m:?}: {e}", m = m as u8);
    }
}
