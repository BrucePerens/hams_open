// SPDX-License-Identifier: AGPL-3.0-or-later
//! Host side of the FX2LP Station I/O board (`hardware/fx2_station_io/`, protocol 0.2): sans-IO. It parses the board's `GET_INFO` and status
//! records, refuses a board that is not a station I/O board or does not report a hardware time-out, heartbeat and safe state, turns a
//! [`GpioMap`] into the firmware's map requests, and runs the heartbeat. The USB control transfers themselves go through the
//! [`ControlTransport`] trait (the broker's device access in production, a fake in tests), so nothing here touches a device.

use crate::error::{StationError, StationErrorKind};
use crate::gpio_map::{Function, GpioMap, Level};

/// Device type (sub-ID) of the Station I/O image. The bench instrument image has its own and is refused here.
pub const STATION_IO_TYPE: u16 = 0x0001;
/// The protocol major version this driver speaks; another major is refused.
pub const PROTOCOL_MAJOR: u8 = 0;

const REQ_GET_INFO: u8 = 0x01;
const REQ_GET_STATUS: u8 = 0x05;
const REQ_HEARTBEAT: u8 = 0x06;
const REQ_MAP_BEGIN: u8 = 0x10;
const REQ_MAP_PIN: u8 = 0x11;
const REQ_MAP_PARAM: u8 = 0x12;
const REQ_MAP_HEARTBEAT: u8 = 0x13;
const REQ_MAP_COMMIT: u8 = 0x14;
const REQ_KEY: u8 = 0x15;

const FUNC_PTT: u16 = 1;
const FUNC_KEYLINE: u16 = 2;
const FUNC_POWER: u16 = 3;
const PARAM_MAX_ON_MS: u16 = 1;

const FLAG_ACTIVE_HIGH: u16 = 0x04;
const FLAG_SAFE_DEFINED: u16 = 0x08;

/// Capability bits of `GET_INFO`.
pub const CAP_HW_TIMEOUT: u32 = 0x08;
/// The board has a heartbeat.
pub const CAP_HEARTBEAT: u32 = 0x10;
/// The board releases its outputs on a USB reset.
pub const CAP_SAFE_ON_RESET: u32 = 0x20;

fn err(kind: StationErrorKind, m: impl Into<String>) -> StationError {
    StationError::new(kind, m.into())
}

/// One USB control transfer, vendor type, to the board.
pub trait ControlTransport {
    /// A control OUT; `Err` when the transfer failed or the board stalled it.
    fn control_out(&mut self, request: u8, value: u16, index: u16) -> Result<(), StationError>;
    /// A control IN of up to `len` bytes.
    fn control_in(
        &mut self,
        request: u8,
        value: u16,
        index: u16,
        len: usize,
    ) -> Result<Vec<u8>, StationError>;
}

// [@ANCHOR: radioctl_station_fx2_info]
/// What the board says about itself (`GET_INFO`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    /// Protocol (major, minor).
    pub protocol: (u8, u8),
    /// Device type (sub-ID).
    pub device_type: u16,
    /// Firmware (major, minor, patch).
    pub firmware: (u8, u8, u8),
    /// Board map id.
    pub board_id: u8,
    /// Capability flags.
    pub capabilities: u32,
    /// 0: every output released to input; 1: per map entry.
    pub safe_state_kind: u8,
    /// Default maximum on-time, ms.
    pub default_max_on_ms: u16,
    /// Default heartbeat lapse, ms.
    pub default_heartbeat_ms: u16,
}

impl Info {
    // [@ANCHOR: radioctl_station_fx2_info_parse]
    /// Parses a `GET_INFO` record (little endian, byte 0 is its length; bytes past the known ones are ignored).
    pub fn parse(b: &[u8]) -> Result<Info, StationError> {
        let bad = |m: &str| err(StationErrorKind::Driver, format!("GET_INFO: {m}"));
        if b.len() < 20 || usize::from(b[0]) > b.len() || b[0] < 20 {
            return Err(bad("record shorter than the fixed fields"));
        }
        Ok(Info {
            protocol: (b[1], b[2]),
            device_type: u16::from_le_bytes([b[3], b[4]]),
            firmware: (b[5], b[6], b[7]),
            board_id: b[8],
            capabilities: u32::from_le_bytes([b[9], b[10], b[11], b[12]]),
            safe_state_kind: b[13],
            default_max_on_ms: u16::from_le_bytes([b[14], b[15]]),
            default_heartbeat_ms: u16::from_le_bytes([b[16], b[17]]),
        })
    }

    // [@ANCHOR: radioctl_station_fx2_info_require_station_io]
    /// Accepts only a Station I/O board that reports a hardware time-out, a heartbeat and a safe state on reset: a bench board, an unknown
    /// protocol major or a board that does not say how it fails safe is refused, because this driver can key a transmitter.
    pub fn require_station_io(&self) -> Result<(), StationError> {
        let refuse = |m: String| Err(err(StationErrorKind::Refused, m));
        if self.device_type != STATION_IO_TYPE {
            return refuse(format!(
                "device type {:#06x} is not a Station I/O board ({:#06x}); a bench board is never used for the station",
                self.device_type, STATION_IO_TYPE
            ));
        }
        if self.protocol.0 != PROTOCOL_MAJOR {
            return refuse(format!(
                "protocol {}.{} is not a major this driver speaks ({PROTOCOL_MAJOR})",
                self.protocol.0, self.protocol.1
            ));
        }
        for (bit, what) in [
            (CAP_HW_TIMEOUT, "a hardware transmit time-out"),
            (CAP_HEARTBEAT, "a heartbeat"),
            (CAP_SAFE_ON_RESET, "release on USB reset"),
        ] {
            if self.capabilities & bit == 0 {
                return refuse(format!("the board does not report {what}"));
            }
        }
        if self.safe_state_kind > 1 || self.default_max_on_ms == 0 || self.default_heartbeat_ms == 0
        {
            return refuse("the board's safe state or default time-outs are not defined".into());
        }
        Ok(())
    }
}

// [@ANCHOR: radioctl_station_fx2_status]
/// The board's status record (23 bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// A valid map is active.
    pub map_valid: bool,
    /// The heartbeat is armed and alive.
    pub heartbeat_alive: bool,
    /// The host claimed a channel keyed that the board had already released.
    pub out_of_sync: bool,
    /// Bit per channel: keyed now.
    pub keyed_mask: u8,
    /// Bit per channel: line at its active level.
    pub active_mask: u8,
    /// Last release reason (1 timeout, 2 heartbeat lapse, 3 sequence, 4 mismatch, 5 USB reset, 6 suspend, 7 host, 8 map cleared).
    pub last_release_reason: u8,
    /// Releases so far, modulo 256.
    pub release_count: u8,
    /// Last map error code.
    pub map_error: u8,
    /// Last key refusal code.
    pub key_refusal: u8,
    /// Sequence of the last heartbeat the board accepted.
    pub last_sequence: u16,
}

impl Status {
    // [@ANCHOR: radioctl_station_fx2_status_parse]
    /// Parses a status record.
    pub fn parse(b: &[u8]) -> Result<Status, StationError> {
        if b.len() < 23 || b[0] != 23 {
            return Err(err(
                StationErrorKind::Driver,
                format!("status record of {} bytes is not the 23 expected", b.len()),
            ));
        }
        Ok(Status {
            map_valid: b[1] & 1 != 0,
            heartbeat_alive: b[1] & 2 != 0,
            out_of_sync: b[1] & 4 != 0,
            keyed_mask: b[2],
            active_mask: b[3],
            last_release_reason: b[4],
            release_count: b[6],
            map_error: b[7],
            key_refusal: b[8],
            last_sequence: u16::from_le_bytes([b[11], b[12]]),
        })
    }
}

// [@ANCHOR: radioctl_station_fx2_map_step]
/// A control OUT of the map load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapStep {
    /// Request number.
    pub request: u8,
    /// wValue.
    pub value: u16,
    /// wIndex.
    pub index: u16,
}

/// The pin id of a board pin name: port A is 0, B 1, D 2 in the high nibble, the bit in the low.
// [@ANCHOR: radioctl_station_fx2_pin_id]
pub fn pin_id(name: &str) -> Result<u8, StationError> {
    let b = name.as_bytes();
    let port = match b.get(1) {
        Some(b'A') => 0u8,
        Some(b'B') => 1,
        Some(b'D') => 2,
        _ => 255,
    };
    match (b.first(), b.get(2), b.len()) {
        (Some(b'P'), Some(bit @ b'0'..=b'7'), 3) if port != 255 => Ok((port << 4) | (bit - b'0')),
        _ => Err(err(
            StationErrorKind::Config,
            format!("pin {name:?} is not PA0 to PA7, PB0 to PB7 or PD0 to PD7"),
        )),
    }
}

/// The requests that load `map` into the board, ending with the commit. Only ptt, keyline and power lines exist in this firmware; a line with
/// another function that is an output is refused, and inputs (sensors) are left alone, since every unlisted pin is an input. A ptt line
/// carries its `max_on_ms`. The active level is the opposite of the safe level.
// [@ANCHOR: radioctl_station_fx2_map_steps]
pub fn map_steps(map: &GpioMap, heartbeat_lapse_ms: u16) -> Result<Vec<MapStep>, StationError> {
    if heartbeat_lapse_ms == 0 {
        return Err(err(
            StationErrorKind::Config,
            "the heartbeat lapse must be above zero",
        ));
    }
    let mut steps = vec![MapStep {
        request: REQ_MAP_BEGIN,
        value: 0,
        index: 0,
    }];
    for l in &map.lines {
        let func = match l.function {
            Function::Ptt => FUNC_PTT,
            Function::Keyline => FUNC_KEYLINE,
            Function::Power => FUNC_POWER,
            Function::Sensor | Function::Spare => continue,
            f => {
                return Err(err(
                    StationErrorKind::Config,
                    format!(
                        "pin {}: the Station I/O firmware has no {f:?} function",
                        l.pin
                    ),
                ))
            }
        };
        let pin = u16::from(pin_id(&l.pin)?);
        let safe = l.safe.ok_or_else(|| {
            err(
                StationErrorKind::Config,
                format!("pin {}: no safe state", l.pin),
            )
        })?;
        // Safe low means active high and the reverse; the firmware refuses a safe state equal to the active level.
        let flags = FLAG_SAFE_DEFINED
            | match safe {
                Level::Low => FLAG_ACTIVE_HIGH,
                Level::High => 1,
            };
        steps.push(MapStep {
            request: REQ_MAP_PIN,
            value: pin | (func << 8),
            index: flags,
        });
        if func == FUNC_PTT {
            let ms = l
                .max_on_ms
                .filter(|m| (1..=65_535).contains(m))
                .ok_or_else(|| {
                    err(
                        StationErrorKind::Config,
                        format!("pin {}: a ptt line needs max_on_ms from 1 to 65535", l.pin),
                    )
                })?;
            steps.push(MapStep {
                request: REQ_MAP_PARAM,
                value: pin | (PARAM_MAX_ON_MS << 8),
                index: ms as u16,
            });
        }
    }
    steps.push(MapStep {
        request: REQ_MAP_HEARTBEAT,
        value: 0,
        index: heartbeat_lapse_ms,
    });
    steps.push(MapStep {
        request: REQ_MAP_COMMIT,
        value: 0,
        index: 0,
    });
    Ok(steps)
}

// [@ANCHOR: radioctl_station_fx2_board]
/// An open Station I/O board.
pub struct Board<T: ControlTransport> {
    t: T,
    info: Info,
    seq: u16,
}

impl<T: ControlTransport> Board<T> {
    // [@ANCHOR: radioctl_station_fx2_board_open]
    /// Reads `GET_INFO` and refuses a board that [`Info::require_station_io`] refuses. Nothing is switched.
    pub fn open(mut t: T) -> Result<Board<T>, StationError> {
        let info = Info::parse(&t.control_in(REQ_GET_INFO, 0, 0, 64)?)?;
        info.require_station_io()?;
        Ok(Board { t, info, seq: 0 })
    }

    /// What the board reported.
    pub fn info(&self) -> &Info {
        &self.info
    }

    // [@ANCHOR: radioctl_station_fx2_board_status]
    /// Reads the status record.
    pub fn status(&mut self) -> Result<Status, StationError> {
        Status::parse(&self.t.control_in(REQ_GET_STATUS, 0, 0, 64)?)
    }

    // [@ANCHOR: radioctl_station_fx2_board_load_map]
    /// Sends the map and commits it. If the firmware refuses a step, the error carries its map error code, and the board has released
    /// everything.
    pub fn load_map(&mut self, map: &GpioMap, heartbeat_lapse_ms: u16) -> Result<(), StationError> {
        for s in map_steps(map, heartbeat_lapse_ms)? {
            if let Err(e) = self.t.control_out(s.request, s.value, s.index) {
                let code = self.status().map(|s| s.map_error).ok();
                return Err(err(
                    StationErrorKind::Driver,
                    format!(
                        "the board refused map request {:#04x}: {} (map error {code:?})",
                        s.request, e.detail
                    ),
                ));
            }
        }
        Ok(())
    }

    // [@ANCHOR: radioctl_station_fx2_board_heartbeat]
    /// One heartbeat with the commanded state (bit per channel). The sequence changes every time. The answer must echo that sequence and
    /// the board's keyed state must equal the commanded one, or the call is an error; the board has released everything by then.
    pub fn heartbeat(&mut self, commanded: u8) -> Result<Status, StationError> {
        self.seq = self.seq.wrapping_add(1);
        let st = Status::parse(&self.t.control_in(
            REQ_HEARTBEAT,
            self.seq,
            u16::from(commanded),
            64,
        )?)?;
        if st.last_sequence != self.seq {
            return Err(err(
                StationErrorKind::NotConfirmed,
                format!(
                    "the board answered heartbeat {} with sequence {}",
                    self.seq, st.last_sequence
                ),
            ));
        }
        if st.keyed_mask != commanded || st.out_of_sync || !st.heartbeat_alive {
            return Err(err(
                StationErrorKind::NotConfirmed,
                format!(
                    "heartbeat: commanded {commanded:#04x}, board keyed {:#04x}, alive {}, out of sync {} (last release reason {})",
                    st.keyed_mask, st.heartbeat_alive, st.out_of_sync, st.last_release_reason
                ),
            ));
        }
        Ok(st)
    }

    // [@ANCHOR: radioctl_station_fx2_board_key]
    /// Key a ptt line down or up (pin name). The firmware refuses without a map or a live heartbeat; its reason is in the status.
    pub fn key(&mut self, pin: &str, down: bool) -> Result<(), StationError> {
        let id = pin_id(pin)?;
        self.t
            .control_out(REQ_KEY, u16::from(id), u16::from(down))
            .map_err(|e| {
                let why = self.status().map(|s| s.key_refusal).ok();
                err(
                    StationErrorKind::Refused,
                    format!(
                        "the board refused the key command ({}; refusal {why:?})",
                        e.detail
                    ),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpio_map::{BoardCatalog, GpioMap};

    /// GET_INFO of the simulator in `hardware/fx2_station_io/tools/fx2_simulator.py`.
    const INFO: &str = "1700020100000200013f000000018813d0070703010001";
    /// Its status after a ptt map on PA0 and heartbeat 5.
    const STATUS: &str = "170300000000000000000005000000ffffffffe8032001";

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or(0))
            .collect()
    }

    const MAP: &str = r#"
board = "fx2-lcsoft-mini"
[[line]]
pin = "PA0"
direction = "output"
safe = "low"
function = "ptt"
channel = "tx"
max_on_ms = 2000
[[line]]
pin = "PA1"
direction = "output"
safe = "high"
function = "keyline"
channel = "tx"
[[line]]
pin = "PB3"
direction = "input"
function = "sensor"
channel = "swr"
"#;

    fn map() -> GpioMap {
        GpioMap::from_toml(MAP, &BoardCatalog::builtin()).unwrap()
    }

    /// A board that records the transfers and answers from a script.
    #[derive(Default)]
    struct Fake {
        outs: Vec<(u8, u16, u16)>,
        info: Vec<u8>,
        status: Vec<u8>,
        stall_on: Option<u8>,
        echo_seq: bool,
    }

    impl ControlTransport for Fake {
        fn control_out(&mut self, r: u8, v: u16, i: u16) -> Result<(), StationError> {
            self.outs.push((r, v, i));
            if self.stall_on == Some(r) {
                return Err(err(StationErrorKind::Driver, "stall"));
            }
            Ok(())
        }
        fn control_in(
            &mut self,
            r: u8,
            v: u16,
            i: u16,
            _l: usize,
        ) -> Result<Vec<u8>, StationError> {
            Ok(match r {
                REQ_GET_INFO => self.info.clone(),
                REQ_HEARTBEAT => {
                    let mut s = self.status.clone();
                    if self.echo_seq {
                        s[11..13].copy_from_slice(&v.to_le_bytes());
                    }
                    s[2] = i as u8;
                    s
                }
                _ => self.status.clone(),
            })
        }
    }

    fn fake() -> Fake {
        Fake {
            info: hex(INFO),
            status: hex(STATUS),
            echo_seq: true,
            ..Fake::default()
        }
    }

    // Tests [@ANCHOR: radioctl_station_fx2_info_parse]
    // Tests [@ANCHOR: radioctl_station_fx2_status_parse]
    // Tests [@ANCHOR: radioctl_station_fx2_status]
    // Tests [@ANCHOR: radioctl_station_fx2_info]
    #[test]
    fn the_simulators_records_parse() {
        let i = Info::parse(&hex(INFO)).unwrap();
        assert_eq!(i.protocol, (0, 2));
        assert_eq!((i.device_type, i.firmware, i.board_id), (1, (0, 2, 0), 1));
        assert_eq!(i.capabilities, 0x3F);
        assert_eq!((i.default_max_on_ms, i.default_heartbeat_ms), (5000, 2000));
        let s = Status::parse(&hex(STATUS)).unwrap();
        assert!(s.map_valid && s.heartbeat_alive && !s.out_of_sync);
        assert_eq!(s.last_sequence, 5);
        assert!(Info::parse(&hex(INFO)[..10]).is_err());
        assert!(Status::parse(&hex(STATUS)[..22]).is_err());
    }

    // Tests [@ANCHOR: radioctl_station_fx2_info_require_station_io]
    // Tests [@ANCHOR: radioctl_station_fx2_board_open]
    // Tests [@ANCHOR: radioctl_station_fx2_board]
    #[test]
    fn a_bench_board_or_one_without_timeouts_is_refused() {
        assert!(Board::open(fake()).is_ok());
        for patch in [
            (3usize, 0x02u8), // the bench device type
            (1, 1),           // another protocol major
            (9, 0x07),        // no heartbeat or release-on-reset capability
            (14, 0),          // default maximum on-time 0 (low byte, high byte below)
        ] {
            let mut f = fake();
            f.info[patch.0] = patch.1;
            if patch.0 == 14 {
                f.info[15] = 0;
            }
            let e = Board::open(f).err().unwrap();
            assert_eq!(e.kind, StationErrorKind::Refused, "{patch:?}: {}", e.detail);
        }
        let mut f = fake();
        f.info[13] = 7;
        assert!(Board::open(f).is_err());
    }

    // Tests [@ANCHOR: radioctl_station_fx2_pin_id]
    #[test]
    fn pin_names_become_port_and_bit() {
        assert_eq!(pin_id("PA0").unwrap(), 0x00);
        assert_eq!(pin_id("PB3").unwrap(), 0x13);
        assert_eq!(pin_id("PD7").unwrap(), 0x27);
        for bad in ["PC0", "PA8", "A0", "PA", "PA00", ""] {
            assert!(pin_id(bad).is_err(), "{bad}");
        }
    }

    // Tests [@ANCHOR: radioctl_station_fx2_map_steps]
    // Tests [@ANCHOR: radioctl_station_fx2_board_load_map]
    // Tests [@ANCHOR: radioctl_station_fx2_map_step]
    #[test]
    fn a_map_becomes_the_firmwares_requests_in_order_with_inputs_left_alone() {
        let steps = map_steps(&map(), 1000).unwrap();
        let got: Vec<(u8, u16, u16)> = steps
            .iter()
            .map(|s| (s.request, s.value, s.index))
            .collect();
        assert_eq!(
            got,
            vec![
                (0x10, 0, 0),
                (0x11, 0x0100, 0x04 | 0x08), // PA0 ptt, safe low so active high
                (0x12, 0x0100, 2000),        // its max on-time
                (0x11, 0x0201, 0x01 | 0x08), // PA1 keyline, safe high so active low
                (0x13, 0, 1000),
                (0x14, 0, 0),
            ]
        );
        let mut b = Board::open(fake()).unwrap();
        b.load_map(&map(), 1000).unwrap();
        assert_eq!(b.t.outs.len(), 6);
        assert!(map_steps(&map(), 0).is_err());
        // A function the firmware does not have, as an output, is refused here.
        let sel = GpioMap::from_toml(
            &MAP.replace("function = \"keyline\"", "function = \"selector\"\nbit = 1"),
            &BoardCatalog::builtin(),
        )
        .unwrap();
        assert!(map_steps(&sel, 1000)
            .unwrap_err()
            .detail
            .contains("no Selector"));
        // A refused step reports the firmware's map error code.
        let mut f = fake();
        f.stall_on = Some(0x14);
        let e = Board::open(f).unwrap().load_map(&map(), 1000).unwrap_err();
        assert!(
            e.detail.contains("0x14") && e.detail.contains("map error"),
            "{}",
            e.detail
        );
    }

    // Tests [@ANCHOR: radioctl_station_fx2_board_heartbeat]
    // Tests [@ANCHOR: radioctl_station_fx2_board_status]
    // Tests [@ANCHOR: radioctl_station_fx2_board_key]
    #[test]
    fn the_heartbeat_must_echo_its_sequence_and_the_keyed_state_and_keying_reports_refusal() {
        let mut b = Board::open(fake()).unwrap();
        let s = b.heartbeat(0).unwrap();
        assert_eq!(s.last_sequence, 1);
        assert_eq!(b.heartbeat(0).unwrap().last_sequence, 2);
        // The board keyed something the host did not command: an error, not a quiet success.
        let mut f = fake();
        f.status[2] = 1;
        let mut b = Board::open(f).unwrap();
        let _ = b.t.status.clone();
        assert!(b.heartbeat(1).is_ok());
        // Wrong sequence echoed.
        let mut f = fake();
        f.echo_seq = false;
        let e = Board::open(f).unwrap().heartbeat(0).unwrap_err();
        assert_eq!(e.kind, StationErrorKind::NotConfirmed);
        // Lapsed heartbeat (alive bit clear).
        let mut f = fake();
        f.status[1] = 1;
        assert!(Board::open(f).unwrap().heartbeat(0).is_err());
        // Key commands.
        let mut b = Board::open(fake()).unwrap();
        b.key("PA0", true).unwrap();
        assert_eq!(b.t.outs, vec![(0x15, 0, 1)]);
        let mut f = fake();
        f.stall_on = Some(0x15);
        let e = Board::open(f).unwrap().key("PA0", true).unwrap_err();
        assert_eq!(e.kind, StationErrorKind::Refused);
        assert!(b.key("PC1", true).is_err());
        assert_eq!(b.status().unwrap().last_sequence, 5);
        assert_eq!(b.info().device_type, STATION_IO_TYPE);
    }
}
