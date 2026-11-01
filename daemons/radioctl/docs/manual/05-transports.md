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
| TCP, bounded connect only (`connect_tcp`) | **Implemented** | no |
| Network link, TCP or UDP (`net::open_link`: network serial server, a radio's own CAT port, a FlexRadio's SmartSDR port 4992 (unverified), `rigctld`, framed or login radios through the `Framing` hooks) | **Implemented**, tested against loopback fakes, unverified on a radio | yes, with backoff, and the actor unkeys on every reconnect |
| Pseudo-terminal | works as a serial path with `LineControl::Unmanaged` | as serial |
| USB devices that speak control transfers or HID reports (Si570 kits, FiFi-SDR, FUNcube, Digital World Traveller): `usb_link::open_usb` over a `UsbBackend` | **Implemented**, tested against virtual twins on a fake bus, unverified on a board; the direct libusb backend is the cargo feature `usb` (off by default) | yes, with backoff, and the actor unkeys on every reopen |
| Kenwood TS-890S LAN (`kenwood_ts890_lan::Ts890LanFraming`: `##CN`, `##ID` login, keep-alive) | **Implemented** from Kenwood's public guide, tested against a fake radio, unverified on a radio | yes: the link logs in again after a reconnect |
| Icom network control (UDP with its own login and audio streams) | **Planned**: no public manual was used; only the generic `Framing` hooks exist | |

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
//! needs hardware. It reads the radio and never keys it: the transmit gate stays closed (the one transmitter command it sends is the start-up unkey).
//!
//! Usage: `serial_rig PORT BAUD` (a path such as `/dev/ttyUSB0`), or `serial_rig usb VID PID
//! [SERIAL]` (hexadecimal ids, found again after every re-plug).
use std::error::Error;
use std::path::PathBuf;

use radioctl_core::serial::{PortTarget, SerialConfig};
use radioctl_core::usb::UsbMatcher;
use radioctl_protocols::prelude::*;
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
    let description = rigfile::load("elecraft-kx3")?.description;

    // Opens the port, watches the link (a reopen after a USB glitch unkeys the rig first), and
    // sends the start-up unkey radioctld sends. The transmit gate stays closed. Keep `link`
    // (control lines, status, counters) alive for the life of the rig.
    let mut config = SerialConfig::new(target, baud);
    config.first_open_attempts = 3;
    let SerialRig { rig, link } = open_serial_rig_with(
        &Handle::current(),
        &description,
        config,
        RigOptions::default(),
    )
    .await?;
    println!("link: {:?}", link.status());
    let frequency = rig.frequency(Vfo::A, Freshness::Fresh).await?;
    println!("VFO A: {} Hz", frequency.value.0);
    Ok(())
}
```

## The baud probe

Off unless you ask for it. When the radio's rate is unknown, `radioctl_core::baud_probe` finds it by
asking, at each candidate rate in turn, the one question that changes nothing:

```rust signatures
pub trait ProbePort: Send {
```

```rust signatures
pub async fn probe<P: ProbePort, E: ProtocolEngine>(
    port: &mut P,
    engine: &mut E,
    rates: &[u32],
) -> Result<ProbeResult, RigError>
```

For each rate in order, `probe` opens the port at that rate, writes the engine's own encoding of
`Op::GetFrequency(Vfo::A)` (CI-V command `03` on an Icom; never a hand-written frame, and the probe refuses to
run at all if the engine's reply kind for that request is anything but the VFO A frequency observation),
waits one description timeout (`Timing::timeout`) for an `Inbound::Observation`, and closes the port. The
first rate that answers wins and the probe stops: no later rate is opened and no second request is sent.
`ProbeResult { baud, tried }` names the winner and every rate opened. No answer at any rate is
`ErrorKind::LinkDown` naming every rate tried; an open error (the device is missing or not permitted) is
not a baud problem and ends the probe with that error. The probe never touches the transmit gate; it runs
before the rig actor exists, so the actor's start-up unkey pass still runs, at the chosen rate.

`ProbePort` is the policy's only contact with a port, so the policy is tested without one
(`crates/radioctl-core/tests/baud_probe.rs`: an in-memory virtual IC-7300 that answers at one rate only
proves the order, one request per rate, one timeout per silent rate, the stop, and that the wire carried
nothing but the read). `SerialProbePort` is the real one: each `open` is `open_serial` at the given rate with
the same `SerialConfig` as the real link (so DTR and RTS are made inactive after every open, or never touched
for `lines = Unmanaged`), and `close` waits for the supervisor to report `Closed`, because the port is held
exclusively and the next open would otherwise find it busy. `DEFAULT_RATES` is 4800, 9600, 19200, 38400,
57600, 115200. radioctld exposes the probe as `baud = "auto"` or `baud_probe = [...]` (chapter
[9](09-radioctld.md#configuration-reference)). A pseudo-terminal ignores the rate, so a pty proves the wiring
only; the rates and the IC-7300's answer are unverified until a real radio is benched.

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
- The byte stream held by the actor stays open throughout, so the actor learns of the unplug only
  through the status: pass `link.watch()` as `RigOptions::link_watch` (the example above does;
  `radioctld` and the splitter example do). The actor then marks the link down while the port is
  `Reconnecting` (`RigHandle::link_up()` is false, every command is refused with `LinkDown`, the
  loss is reported once as `Alarm::LinkDown`) and, on the reopen, marks it up again and **unkeys
  the radio before serving any queued command**, because the radio may have been keyed when the
  port was lost and nothing could unkey it meanwhile; `Alarm::LinkRestored` follows the unkey, and
  an unkey that fails is `Alarm::UnkeyFailed` as always. Without the watch, a dead cable looks like
  timeouts and `link_up()` stays true.
- `SerialLink::into_parts()` gives the byte stream for `rig::spawn` and a `SerialMonitor` (the
  control lines, `status()`, `watch()`, `stats()`) that the owner keeps for the life of the rig.
  `radioctld` keeps it in the served rig; it is never dropped while the rig is served.
- Nothing is re-initialised after a reconnect. A radio that lost power comes back in its own default
  state (for example with transceive off), and the cache still holds the old values until polling or
  a read replaces them. The proposal's explicit lifecycle (`Off`, `Booting`, `Ready`, ...) is
  **planned**.
- The supervisor keeps running after you drop the `SerialLink` struct; it stops only when the byte
  stream is dropped.

## USB devices

Some SDR boards and dongles have no serial port: the Si570 kits (SoftRock, KTH, Peaberry, FA-SDR),
the FiFi-SDR, the FUNcube dongles and the Digital World Traveller are tuned with USB vendor control
transfers or HID reports. They are still a transport: `radioctl_core::usb_link::open_usb` gives the
actor the same byte stream, status watch and counters as a serial link, and the engines (`si570_usb`,
`funcube`, `dwt`) stay sans-IO.

What goes over the stream is a frame, not CAT: the engine writes a request frame
(`usb_wire::UsbTransfer`: control in or out with request type, request, wValue, wIndex and data; bulk
or interrupt read and write; or an interrupt write followed by the read of its answer) and reads a
reply frame (`UsbReply`: status and data). A supervisor thread owns the open device and performs one
transfer at a time. The rules are the serial link's:

- If the device is lost (a transfer reports `NoDevice`) the status is `Reconnecting`, commands are
  refused with `LinkDown`, bytes written meanwhile are dropped and counted, and the device is looked
  up again by vendor id, product id, manufacturer and product strings and serial number with
  exponential backoff, because it usually re-enumerates. The V-USB boards all share 16C0:05DC and are
  told apart by their strings; two identical boards need the serial number.
- After a reopen the actor unkeys the device before it serves anything else (pass `link.watch()` as
  `RigOptions::link_watch`). The transmit gate is unchanged: a board's PTT request is refused while
  the gate is closed and never reaches the device.
- A transfer that times out or stalls does not take the link down; the command ends in the actor's
  typed `Timeout` or a refusal.

Device access is behind two traits in `radioctl_core::usb_device`: `UsbBackend::open(&UsbTarget)` and
`UsbDevice` (`control_in`, `control_out`, `bulk_write`, `bulk_read`, `interrupt_write`,
`interrupt_read`). `FakeUsbBackend` serves a virtual twin that can be unplugged and plugged back
(`unplug`, `plug`, `opens`, `log`) and is what the tests use; `usb_libusb::LibusbBackend` opens the
device directly through libusb and exists only with the cargo feature `usb`
(`cargo build -p radioctld --features usb`, needs libusb-1.0 and a udev rule giving your user access
to the board; the kernel driver is detached while the interface is claimed). The relay's
zero-privilege broker will implement the same traits over a handle it hands over.

```toml radioctld
[[rig]]
name = "softrock"
description = "softrock-si570-avr-usb.toml"   # supplies the ids and strings
[rig.transport]
kind = "usb"
serial = "A12345"                             # only if two identical boards are attached
# vid = 0x16C0                                # optional overrides of the description's ids
# pid = 0x05DC
transfer_timeout_ms = 1000                    # 50 to 10000
```

A build without the `usb` feature refuses this transport at start with a message saying so.
`kind = "virtual"` on a USB description runs the board's twin on the fake bus, for trying
clients. Status: unverified on every board; the Elektor SDR-USB has a description but no engine yet
(it programs a Cypress PLL through an FTDI chip in bit-bang mode).

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
its link down and stays down; restart the rig. `radioctl-capi`'s TCP rigs use it; `radioctld`'s `tcp`
transport is on `net::open_link` below, which reconnects.

## Network links

`net::open_link` is the network counterpart of `open_serial`: a supervisor task owns the socket,
the actor gets an in-memory byte stream that stays open across reconnects, and a `LinkStatus` watch
tells the actor when the network went away and came back.

```rust signatures
pub async fn open_link(
    runtime: &Handle,
    config: NetConfig,
    framing: Box<dyn Framing>,
) -> Result<NetLink, RigError>
```

```rust signatures
pub struct NetConfig {
    pub address: String,
    pub protocol: NetProtocol,
    pub connect_timeout: Duration,
    pub handshake_timeout: Duration,
    pub write_timeout: Duration,
    pub idle_timeout: Option<Duration>,
    pub backoff: Backoff,
    pub first_attempts: u32,
}
```

`NetConfig::new(address)` gives TCP, three-second connect, handshake and write limits, no silence
limit, the default backoff and one first attempt. `open_link` returns once the first connection
(and its handshake) is ready, or with `LinkDown` after `first_attempts`.

- **Every wait is bounded:** connect, handshake, each write, and silence on the wire
  (`idle_timeout`, off by default; set it together with a keep-alive).
- **Reconnect is the serial story again.** On loss the status becomes `Reconnecting` and the actor
  refuses commands with `LinkDown`; the supervisor retries without limit with exponential backoff
  and publishes `Connected` with a higher `generation` once the new session is ready. Pass
  `link.watch()` as `RigOptions::link_watch`: on that reconnect the actor **unkeys the radio before
  serving any other command**, then reports `Alarm::LinkRestored`. Without the watch the actor
  cannot know.
- **Bytes written while the link is down or still handshaking are dropped and counted**
  (`NetStats::bytes_dropped`), never queued and replayed into the radio after the reconnect.
- **UDP** (`NetProtocol::Udp`) sends one datagram per write and delivers one per read. There is no
  connection to lose, so loss means an ICMP error from the stack or silence for `idle_timeout`; the
  link then opens a fresh socket, repeats the greeting and counts a reconnect. A UDP radio needs a
  keep-alive and an idle limit, or the link never notices that the radio is gone.
- **Radios that wrap CAT in a framing or a login** implement `Framing`: a greeting sent at every
  connect, `encode` and `decode` for the wire, `needs_handshake` (the link is up only after the
  radio answers, for example a login), a `keepalive`, and `reset` at every connect. `Decoded` can
  carry bytes to pass up, bytes to send straight back (a login answer) and a failure that forces a
  reconnect. `RawFraming` is the identity and `LengthFraming` is the 2 or 4 byte length-prefixed
  form. The one manufacturer's network session implemented is the Kenwood TS-890S LAN login
  (`kenwood_ts890_lan`, below). Icom network control (a UDP protocol with its own login and audio
  streams) is added the same way once its documentation is in hand; until then the hooks are all
  there is.
- The link supervisor stops, and the status becomes `Closed`, when the byte stream is dropped.
- `radioctld` still uses `connect_tcp` for its `tcp` transport (no reconnect); moving it to
  `open_link` is a small change in `rigs.rs`, listed in the roadmap.

### The TS-890S over the LAN

Kenwood's TS-890S PC Control Command Reference Guide describes the LAN connector: TCP, UTF-8, the PC
sends `##CN;`, the radio answers `##CN1;` (or `##CN0;`, denied: a LAN connection already exists), the
PC sends `##ID<type><len><len><account><password>;`, and `##ID1;` completes the connection (`##ID0;`
is an authorisation failure). **A connection with no communication for 10 seconds is closed by the
radio**, so the session sends the read-only `ID;` every 4 seconds and swallows its answer.
`Ts890LanFraming` is that session as a `Framing`:

```rust signatures
pub fn new(credentials: LanCredentials) -> Ts890LanFraming
```

`LanCredentials::new(administrator, account, password)` checks the account and password (1 to 32
printable ASCII characters, no `;`) and its `Debug` hides the password. Where the password comes from
is the application's business (a secret store, never a configuration file or a log). Pass
`Box::new(Ts890LanFraming::new(credentials))` to `open_link` with `idle_timeout` a little above the
keep-alive interval; after the login the stream is the ordinary TS-890S CAT stream
(`Ts890Engine`). A wrong password or a denied connection fails `open_link` with `LinkDown` and a message
that says which; the bandscope output (`##DD2`, `##DD3`) is decoded by the engine (chapter 7).

`radioctld` wires this session into its `tcp` transport: `framing = "ts890-lan"` with `lan_account`,
`lan_administrator` and `lan_password_secret` (the name of a file in `server.secrets_dir`; chapter
[9](09-radioctld.md#configuration-reference)). The password is read at start from that file, which
must be the owner's alone, and appears in no message. Tested against a fake radio on loopback only
(`crates/radioctld/tests/ts890_lan.rs`: login, CAT, the keep-alive, a second login after the radio
drops the connection, a refused password, a missing secret); **unverified on a radio**. After a
reconnect the actor unkeys the rig but the engine's session state is not re-sent (`AI2;`): chapter
[13](13-roadmap.md).

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
- **Installation note (the open glitch).** Opening a serial port asserts DTR and RTS for a short
  time before `open_port` makes them inactive: the operating system and the USB-serial driver do
  it, not radioctl, and it happens again on **every reconnect** after a USB glitch, when nobody is
  watching. An interface that keys the transmitter from RTS or DTR must tolerate that glitch: use
  one that needs the line held for longer than the open takes (an RC delay or a keying circuit with
  a time constant of tens of milliseconds), or key through CAT instead, or put a hardware stop in
  the PTT line (the FX2 switch board of `RELAY_EXTERNAL_SWITCHES_AND_POWER_CONTROL.md` 4.2). Do not
  wire a bare RTS or DTR line to a transmitter's PTT on a port that reconnects on its own.
- A `LineKeyer` that is dropped (its holder returned or panicked while keyed) makes its line inactive, so
  a lost keyer does not leave RTS or DTR asserted. That covers the keyer only; nothing releases a line
  when the whole process is killed.
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
