// SPDX-License-Identifier: AGPL-3.0-or-later
//! Protocol decoders for captured waveforms: asynchronous serial (UART), I2C and SPI. They work on the change list, not on samples, so
//! they are exact to the capture's timestamps. No dependency. Entries: `docs/GLASGOW_LOGIC_TOOLS.md`.

use super::wave::Wave;
use serde_json::{json, Value};

/// The most frames, transactions or words one decode returns (the answer says when it cut the list).
pub const MAX_OUT: usize = 4096;

/// `bit!(value, pin)`: the level of one pin in a pin-state word.
macro_rules! bit {
    ($v:expr, $pin:expr) => {
        (($v) >> ($pin)) & 1 == 1
    };
}

/// Async serial on one pin, idle high (or low with `invert`), start bit, `bits` data bits LSB first, optional parity, one or more stop
/// bits (only the first is checked). The start edge is checked again half a bit later, so a glitch shorter than half a bit is not a
/// frame. Returns `{frames, errors, ...}`; each frame has the time, the byte, and framing and parity error flags.
pub fn uart(
    w: &Wave,
    pin: usize,
    baud: f64,
    bits: u32,
    parity: &str,
    invert: bool,
) -> Result<Value, String> {
    // [@ANCHOR: gla_decode_uart]
    if !(50.0..=3_000_000.0).contains(&baud) {
        return Err(format!("baud {baud} must be 50 to 3000000"));
    }
    if !(5..=8).contains(&bits) {
        return Err(format!("bits {bits} must be 5 to 8"));
    }
    if !matches!(parity, "none" | "even" | "odd") {
        return Err(format!("parity {parity:?} must be none, even or odd"));
    }
    let bit_ns = 1e9 / baud;
    let lvl = |t: f64| bit!(w.value_at(t.round() as u64), pin) != invert;
    let nbits = bits as usize + usize::from(parity != "none");
    let (mut frames, mut framing, mut parity_errs, mut false_starts) = (vec![], 0u32, 0u32, 0u32);
    let mut scan = 0u64;
    let mut truncated = false;
    for (i, e) in w.edges.iter().enumerate() {
        if e.t_ns < scan {
            continue;
        }
        if !(bit!(w.value_before(i), pin) != invert && bit!(e.value, pin) == invert) {
            continue;
        }
        let start = e.t_ns as f64;
        if start + (1.5 + nbits as f64) * bit_ns > w.end_ns as f64 {
            truncated = true;
            break;
        }
        if lvl(start + 0.5 * bit_ns) {
            false_starts += 1;
            continue;
        }
        let mut byte = 0u8;
        let mut ones = 0u32;
        for k in 0..bits as usize {
            if lvl(start + (1.5 + k as f64) * bit_ns) {
                byte |= 1 << k;
                ones += 1;
            }
        }
        let mut perr = false;
        if parity != "none" {
            let p = lvl(start + (1.5 + bits as f64) * bit_ns) as u32;
            perr = (ones + p) % 2 == u32::from(parity == "even");
        }
        let ferr = !lvl(start + (1.5 + nbits as f64) * bit_ns);
        framing += u32::from(ferr);
        parity_errs += u32::from(perr);
        if frames.len() < MAX_OUT {
            frames.push(json!({"t_ns": e.t_ns + w.origin_ns, "byte": byte, "hex": format!("{byte:02x}"), "framing_error": ferr, "parity_error": perr}));
        }
        scan = (start + (0.5 + 1.0 + nbits as f64) * bit_ns) as u64;
    }
    let text: String = frames
        .iter()
        .filter_map(|f| f["byte"].as_u64())
        .map(|b| {
            if (32..127).contains(&b) {
                b as u8 as char
            } else {
                '.'
            }
        })
        .collect();
    Ok(
        json!({"protocol": "uart", "baud": baud, "frames": frames, "count": frames.len(), "framing_errors": framing, "parity_errors": parity_errs,
        "false_starts": false_starts, "text": text, "last_frame_cut_by_capture_end": truncated}),
    )
}

/// I2C on SCL and SDA: START (SDA falls while SCL is high), STOP (SDA rises while SCL is high), data sampled at each SCL rising edge,
/// nine clocks per byte (the ninth is the acknowledge, low = ACK). The first byte after a START is the address: 7-bit address and the
/// R/W bit; a first byte of the form 11110xx is flagged `ten_bit` and left undecoded beyond the raw byte.
pub fn i2c(w: &Wave, scl: usize, sda: usize) -> Result<Value, String> {
    // [@ANCHOR: gla_decode_i2c]
    if scl == sda {
        return Err("scl and sda must be different pins".into());
    }
    struct Cur {
        start: u64,
        repeated: bool,
        bytes: Vec<(u8, bool)>,
        nbits: u32,
        acc: u8,
    }
    let mut xfers: Vec<Value> = vec![];
    let mut cur: Option<Cur> = None;
    let finish = |c: Cur, stopped: bool, xfers: &mut Vec<Value>, origin: u64| {
        if xfers.len() >= MAX_OUT {
            return;
        }
        let first = c.bytes.first().copied();
        let ten = first.is_some_and(|(b, _)| b >> 3 == 0b11110);
        let data: Vec<Value> = c
            .bytes
            .iter()
            .skip(1)
            .map(|(b, a)| json!({"hex": format!("{b:02x}"), "ack": a}))
            .collect();
        xfers.push(json!({"start_ns": c.start + origin, "repeated_start": c.repeated, "address": first.map(|(b, _)| b >> 1), "address_hex": first.map(|(b, _)| format!("0x{:02x}", b >> 1)),
            "read": first.map(|(b, _)| b & 1 == 1), "address_ack": first.map(|(_, a)| a), "ten_bit": ten, "data": data, "stop": stopped, "partial_bits": c.nbits}));
    };
    for (i, e) in w.edges.iter().enumerate() {
        let before = w.value_before(i);
        let (scl_b, scl_a, sda_b, sda_a) = (
            bit!(before, scl),
            bit!(e.value, scl),
            bit!(before, sda),
            bit!(e.value, sda),
        );
        if scl_b && scl_a && sda_b != sda_a {
            if let Some(c) = cur.take() {
                finish(c, sda_a, &mut xfers, w.origin_ns);
            }
            if !sda_a {
                cur = Some(Cur {
                    start: e.t_ns,
                    repeated: xfers.last().is_some_and(|x| x["stop"] == json!(false)),
                    bytes: vec![],
                    nbits: 0,
                    acc: 0,
                });
            }
        } else if !scl_b && scl_a {
            if let Some(c) = cur.as_mut() {
                if c.nbits < 8 {
                    c.acc = (c.acc << 1) | u8::from(sda_a);
                    c.nbits += 1;
                } else {
                    c.bytes.push((c.acc, !sda_a));
                    c.nbits = 0;
                    c.acc = 0;
                }
            }
        }
    }
    if let Some(c) = cur.take() {
        finish(c, false, &mut xfers, w.origin_ns);
    }
    Ok(json!({"protocol": "i2c", "transactions": xfers, "count": xfers.len()}))
}

/// SPI: words are shifted at the sampling edge of SCK for the mode (0 and 3 sample on the rising edge, 1 and 2 on the falling), only
/// while CS is asserted (`cs` None: always, one transfer). `bits` per word 4 to 32. Returns one entry per CS assertion with the words
/// of COPI (controller out) and CIPO (peripheral out) as numbers and hex.
#[allow(clippy::too_many_arguments)]
pub fn spi(
    w: &Wave,
    cs: Option<usize>,
    sck: usize,
    copi: Option<usize>,
    cipo: Option<usize>,
    mode: u8,
    bits: u32,
    msb_first: bool,
    cs_active_low: bool,
) -> Result<Value, String> {
    // [@ANCHOR: gla_decode_spi]
    if mode > 3 {
        return Err(format!("mode {mode} must be 0 to 3"));
    }
    if !(4..=32).contains(&bits) {
        return Err(format!("bits {bits} must be 4 to 32"));
    }
    if copi.is_none() && cipo.is_none() {
        return Err("give at least one of copi and cipo".into());
    }
    let sample_rising = (mode >> 1) == (mode & 1);
    let active = |v: u32| cs.is_none_or(|c| bit!(v, c) != cs_active_low);
    struct X {
        start: u64,
        o: Vec<u32>,
        i: Vec<u32>,
        ao: u32,
        ai: u32,
        n: u32,
    }
    let new = |t: u64| X {
        start: t,
        o: vec![],
        i: vec![],
        ao: 0,
        ai: 0,
        n: 0,
    };
    let mut cur: Option<X> = if active(w.init) { Some(new(0)) } else { None };
    let mut done: Vec<X> = vec![];
    for (i, e) in w.edges.iter().enumerate() {
        let before = w.value_before(i);
        if cs.is_some() {
            let (a0, a1) = (active(before), active(e.value));
            if !a0 && a1 {
                cur = Some(new(e.t_ns));
            }
            if a0 && !a1 {
                if let Some(x) = cur.take() {
                    done.push(x);
                }
            }
        }
        if bit!(before, sck) != bit!(e.value, sck) && bit!(e.value, sck) == sample_rising {
            if let Some(x) = cur.as_mut() {
                let take = |v: u32, pin: Option<usize>, acc: u32| -> u32 {
                    let b = pin.map_or(0, |p| u32::from(bit!(v, p)));
                    if msb_first {
                        (acc << 1) | b
                    } else {
                        acc | (b << x.n)
                    }
                };
                x.ao = take(before, copi, x.ao);
                x.ai = take(before, cipo, x.ai);
                x.n += 1;
                if x.n == bits {
                    x.o.push(x.ao);
                    x.i.push(x.ai);
                    x.ao = 0;
                    x.ai = 0;
                    x.n = 0;
                }
            }
        }
    }
    if let Some(x) = cur.take() {
        done.push(x);
    }
    let words = |v: &[u32]| -> Value {
        json!(v
            .iter()
            .map(|w| format!("0x{w:0width$x}", width = (bits as usize).div_ceil(4)))
            .collect::<Vec<_>>())
    };
    let xfers: Vec<Value> = done.iter().take(MAX_OUT).map(|x| json!({"start_ns": x.start + w.origin_ns, "words": x.o.len(), "copi": if copi.is_some() { words(&x.o) } else { Value::Null },
        "cipo": if cipo.is_some() { words(&x.i) } else { Value::Null }, "extra_bits": x.n})).collect();
    Ok(
        json!({"protocol": "spi", "mode": mode, "bits": bits, "transfers": xfers, "count": done.len()}),
    )
}

/// Runs the decoder the `decode` argument asks for (`type` uart, i2c or spi; pins by name or position, see `gla_wave_pin_index`).
pub fn run(w: &Wave, spec: &Value) -> Result<Value, String> {
    // [@ANCHOR: gla_decode_run]
    let pin = |k: &str| -> Result<usize, String> {
        w.pin_index(spec.get(k).ok_or_else(|| format!("decode needs \"{k}\""))?)
    };
    let opt = |k: &str| -> Result<Option<usize>, String> {
        spec.get(k)
            .filter(|v| !v.is_null())
            .map(|v| w.pin_index(v))
            .transpose()
    };
    // A value of the wrong JSON type is refused, not replaced by the default (a wrong baud rate would decode garbage).
    let num = |k: &str, d: f64| -> Result<f64, String> {
        match spec.get(k) {
            None | Some(Value::Null) => Ok(d),
            Some(v) => v
                .as_f64()
                .ok_or_else(|| format!("decode {k} must be a number")),
        }
    };
    let flag = |k: &str, d: bool| -> Result<bool, String> {
        match spec.get(k) {
            None | Some(Value::Null) => Ok(d),
            Some(v) => v
                .as_bool()
                .ok_or_else(|| format!("decode {k} must be true or false")),
        }
    };
    let parity = match spec.get("parity") {
        None | Some(Value::Null) => "none",
        Some(p) => p
            .as_str()
            .ok_or("decode parity must be text: none, even or odd")?,
    };
    match spec.get("type").and_then(|t| t.as_str()) {
        Some("uart") => uart(
            w,
            pin("pin")?,
            num("baud", 115200.0)?,
            num("bits", 8.0)? as u32,
            parity,
            flag("invert", false)?,
        ),
        Some("i2c") => i2c(w, pin("scl")?, pin("sda")?),
        Some("spi") => spi(
            w,
            opt("cs")?,
            pin("sck")?,
            opt("copi")?,
            opt("cipo")?,
            num("mode", 0.0)? as u8,
            num("bits", 8.0)? as u32,
            flag("msb_first", true)?,
            flag("cs_active_low", true)?,
        ),
        o => Err(format!("decode type {o:?} must be uart, i2c or spi")),
    }
}
