# 5. Transports

A transport is anything that moves bytes to and from a radio. The actor needs only this:

```rust signatures
pub trait Transport: AsyncRead + AsyncWrite + Send + Unpin + 'static {}
```

Any tokio `AsyncRead + AsyncWrite` qualifies, and reads must be cancel-safe (tokio's are). The
repository provides serial and USB-serial (`serial`, `usb`), TCP (`tcp`), and the in-memory stream of
a virtual rig (`radioctl_protocols::io`). The `transport.rs` module comment still says no real
transport exists; that comment is out of date.

| Transport | Status | Reconnects |
|---|---|---|
| Serial port by path | **Implemented**, unverified on a radio | yes, with backoff |
| USB serial by vendor id, product id and serial number | **Implemented** (Linux sysfs; other systems through the `serialport` crate) | yes, finds the port again |
| TCP (network serial server, a radio's own CAT port, `rigctld`) | **Implemented** | no: bounded connect only |
| Pseudo-terminal | works as a serial path with `LineControl::Unmanaged` | as serial |
| Native USB (`nusb`), UDP, Ethernet radios (Kenwood LAN, Icom LAN, SmartSDR, HPSDR) | **Planned** | |

## Serial

```rust signatures
pub async fn open_serial(runtime: &Handle, config: SerialConfig) -> Result<SerialLink, RigError>
```

```rust signatures
pub struct SerialConfig {
    pub target: PortTarget,
    pub baud: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub flow: Flow,
    pub lines: LineControl,
    pub idle_dtr: bool,
    pub idle_rts: bool,
    pub backoff: Backoff,
    pub first_open_attempts: u32,
}
```

`SerialLink` has two public fields, the stream for the actor and the control lines:

```rust signatures
pub transport: DuplexStream,

pub control: SerialControl,
```

`SerialConfig::new(target, baud)` gives 8N1, no flow control, both modem lines inactive after open,
the default backoff (250 ms doubling to 8 s) and one open attempt. `open_serial` returns once the first open
has succeeded, or with the last attempt's error after `first_open_attempts`. `link.transport` goes to
`rig::spawn`; dropping it (with the rig) closes the port. `link.status()` and `link.watch()` report
the physical link, `link.stats()` the bytes in, out and dropped and the reconnect count, and
`link.control` gives immediate access to RTS and DTR.

Threads: one supervisor thread opens the port, writes to it and reconnects; one reader thread per
connection reads it. They talk to your runtime through bounded channels and an in-memory duplex
stream. No runtime worker ever blocks on the port.

The example `crates/radioctld/examples/serial_rig.rs` is built by the tests and never run by them
(it needs a radio). It reads a KX3-family radio's frequency and never opens the transmit gate:

```rust example=serial_rig
//! A real radio on a serial or USB-serial port. Compiled by the tests, never run by them: it
//! needs hardware. It reads the radio and never keys it: the transmit gate stays closed.
//!
//! Usage: `serial_rig PORT BAUD` (a path such as `/dev/ttyUSB0`), or `serial_rig usb VID PID
//! [SERIAL]` (hexadecimal ids, found again after every re-plug).
use std::error::Error;
use std::path::PathBuf;

use radioctl_core::rig::{spawn_current, Freshness, RigOptions};
use radioctl_core::rigfile;
use radioctl_core::serial::{open_serial, PortTarget, SerialConfig};
use radioctl_core::units::Vfo;
use radioctl_core::usb::UsbMatcher;
use radioctl_protocols::elecraft::ElecraftEngine;
use tokio::runtime::Handle;

fn target_and_baud(args: &[String]) -> Result<(PortTarget, u32), Box<dyn Error>> {
    match args {
        [path, baud] => Ok((PortTarget::Path(PathBuf::from(path)), baud.parse()?)),
        [usb, vid, pid, rest @ ..] if usb == "usb" => {
            let matcher = UsbMatcher {
                vid: u16::from_str_radix(vid, 16)?,
                pid: u16::from_str_radix(pid, 16)?,
                serial: rest.first().cloned(),
            };
            Ok((PortTarget::Usb(matcher), 38_400))
        }
        _ => Err("usage: serial_rig PORT BAUD | serial_rig usb VID PID [SERIAL]".into()),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (target, baud) = target_and_baud(&args)?;
    let description = rigfile::parse(include_str!("../../../rigs/elecraft-kx3.toml"))?.description;

    let mut config = SerialConfig::new(target, baud);
    config.first_open_attempts = 3;
    let link = open_serial(&Handle::current(), config).await?;
    println!("link: {:?}", link.status());

    let rig = spawn_current(
        ElecraftEngine::new(description)?,
        link.transport,
        RigOptions::default(),
    )?;
    let frequency = rig.frequency(Vfo::A, Freshness::Fresh).await?;
    println!("VFO A: {} Hz", frequency.value.0);
    Ok(())
}
```

## Finding a USB radio by what it is

`/dev/ttyUSB0` today is `/dev/ttyUSB1` after the next re-plug. A `PortTarget::Usb(UsbMatcher { vid,
pid, serial })` names the device instead, and is resolved again at every (re)connect.

- Exactly one port must match. No match is `LinkDown`; several matches are `Unsupported` with the
  message "give the serial number", never a guess. Two identical cables need their serial numbers
  (when the chip has one; many bridge chips do not).
- On Linux, ports are listed from sysfs (`ttyUSB*`, `ttyACM*`; the nearest ancestor with
  `idVendor` and `idProduct` is the USB device); elsewhere from the `serialport` crate. There is no
  `libudev` dependency.
- `usb::classify` names the chip family: FTDI (`0403`), Silicon Labs CP210x (`10c4`, `ea60` to
  `ea71`), WCH CH340, CH341 and CH9102 (`1a86`), or CDC ACM when the kernel bound that driver (a
  radio's native USB serial has no vendor in common). `usb::list_ports()` lists what is present.

## Hot-plug and reconnect

When the device vanishes (a read or write fails, or the port hangs up) the link moves to
`LinkStatus::Reconnecting`. The supervisor reopens it with exponential backoff, looking the port up by
USB id again because the device node usually changes, and the status returns to `Connected` with a
higher `generation`. Reconnect attempts after the first open are unlimited.

What the caller sees, and does not see:

- Bytes written while the port is down are **dropped and counted** (`SerialStats::bytes_dropped`).
  The command that wrote them ends in its own typed `Timeout`.
- The byte stream held by the actor stays open throughout. The actor therefore does **not** notice
  the unplug: `RigHandle::link_up()` stays true, and a dead cable looks like timeouts. The place to
  see `Reconnecting` is `SerialLink::status()` or `watch()`.
- `radioctld` drops the `SerialLink` after taking its stream, so it cannot observe the link status at
  all; to its clients an unplugged radio is a run of `RPRT -5` or `-6` errors, and `\get_powerstat`
  keeps answering 1. **Open item:** surface the link status in the daemon and in `RigHandle`.
- Nothing is re-initialised after a reconnect. A radio that lost power comes back in its own default
  state (for example with transceive off), and the cache still holds the old values until polling or
  a read replaces them. The proposal's explicit lifecycle (`Off`, `Booting`, `Ready`, ...) is
  **planned**.
- The supervisor keeps running after you drop the `SerialLink` struct; it stops only when the byte
  stream is dropped.

## TCP

```rust signatures
pub async fn connect_tcp<A: ToSocketAddrs + Copy>(
    addr: A,
    config: &TcpConfig,
) -> Result<TcpStream, RigError>
```

A bounded connect (default: 3 attempts, 3 s each, with the backoff between) so a radio that is
booting or a network that is down ends in a typed `LinkDown`, never a hang. Nagle's algorithm is
switched off. After connecting there is **no reconnect**: if the connection drops, the actor marks
its link down and stays down; restart the rig (for `radioctld`, restart the daemon). This is the
`tcp` transport of `radioctld` as well.

## Keying with RTS or DTR

Some interfaces key the transmitter from a modem control line instead of a CAT command.
`keyer::LineKeyer` does that behind the same transmit gate that guards CAT keying:

```rust signatures
pub fn new(lines: C, line: KeyLine, active_high: bool) -> Result<LineKeyer<C>, RigError>

pub fn key(&self, rig: &RigHandle) -> Result<(), RigError>

pub fn release(&self) -> Result<(), RigError>
```

- `key` is refused with `TransmitGateClosed` unless the rig's gate is open, and nothing reaches the
  line in that case. `release` is always allowed and never waits.
- The line is an **immediate** call on a second handle of the port (`SerialControl::set_line`),
  never queued behind a pending read. That is the real-time rule for PTT.
- `LineKeyer::new` makes the line inactive at once and reports a failure to do so.
- `LineControl::Managed` (the default) takes the modem lines under control; `Unmanaged` is for
  ports that have none (a pty, a CDC ACM gadget): the lines are never touched and `set_line`
  reports them unsupported.
- With `Flow::Hardware`, RTS belongs to flow control; do not also key with RTS.
- `LineKeyer` is a library part only: no engine and neither `radioctld` nor the splitter uses it.
  PTT through `Op::SetPtt` is always a CAT command today.

### The open-time DTR/RTS glitch

Operating systems assert DTR and RTS when a port is opened. radioctl makes both lines inactive (or
leaves them at `idle_dtr` and `idle_rts`) immediately after the open and fails the open if it cannot,
but **a glitch of microseconds happens first**, before the library can act. Hardware that keys on
RTS or DTR needs an interface that tolerates it (a keying interface that ignores lines until the
host has opened the port, or a separate keying line), and a station should be wired so that a
reconnect cannot key the transmitter. This is a property of the operating system; the library cannot
remove it, and it is why DTR/RTS keying should not be assumed safe on a cable that has not been
checked.
