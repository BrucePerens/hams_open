# 9. radioctld

`radioctld` serves the Hamlib `rigctld` text protocol for any radioctl rig, real or virtual, so
`rigctl -m 2`, loggers and digital-mode programs that already know how to talk to a `rigctld` can use
a radioctl rig without change. It is a library (`radioctld`, with the protocol, the server and the
service for one rig) plus a thin binary.

Status: **implemented**, **unverified** against real radios. The protocol was observed from Hamlib
4.6.2's `rigctld` and checked with Hamlib's own `rigctl` client as an oracle; no other client program has
been tried (chapter [11](11-testing.md)).

## Running it

```sh
radioctld --config FILE [--check]
```

`--help` (or `-h`) prints the usage line and exits 0. `--check` validates the configuration and every rig description it names, prints
`radioctld: configuration ok, N rig(s)`, and exits without listening. Without it the daemon brings up
every rig, prints one `radioctld: NAME listening on ADDRESS` line each, and runs until Ctrl-C. On exit
it stops the listeners and clients and always sends each rig an unkey, whoever is recorded as holding the
transmitter (a rig keyed by Morse, or one whose owner record was lost, has no owner). The binary builds one
multi-thread runtime (two workers) for all rigs and hands its `Handle` to the library, which creates
none.

Before it listens, the daemon sends each rig an unkey (the emergency path: no gate, never refused), so a
radio left keyed by a killed predecessor is released at once instead of at the watchdog's limit. A radio that
does not answer it is not served: `radioctld: rig NAME: the startup unkey failed, the radio may be keyed`
is usually a wrong baud rate or CI-V address.

One `[[rig]]` per radio, each with its own listen address, as with one `rigctld` per radio. If the
first connection to a rig fails (the port is missing, the TCP connect times out) the daemon does not
start; it does not wait for the radio to appear. A `[[rotator]]` is served the same way in the
`rotctld` protocol, on its own listener (`radioctld: rotator NAME listening on ADDRESS`); see
[Rotators](#rotators) below.

## Configuration reference

The file is validated completely at load time: an unknown key, an unsafe bind or an out-of-range value
is an error, never a silent default. Below, a file with every key set to its default (or a typical
value for a required one).

```toml radioctld
[server]
allow = ["127.0.0.0/8", "::1"]
allow_non_loopback_bind = false
max_clients = 16
idle_timeout_secs = 600
line_timeout_secs = 10
max_line_bytes = 1024
request_timeout_ms = 2000
cache_max_age_ms = 1000

[[rig]]
name = "kx3"
description = "rigs/elecraft-kx3.toml"
listen = "127.0.0.1:4532"
vfo_mode = false
transmit = false
max_transmit_s = 180
rx_low_hz = 30000
rx_high_hz = 60000000
tx_power_mw = 100000
hamlib_model = 1

[rig.transport]
kind = "virtual"
reply_delay_ms = 0
```

| Key | Default | Range | Meaning |
|---|---|---|---|
| `server.allow` | loopback only | list of `addr` or `addr/prefix` | Networks allowed to connect. An empty list allows nobody. |
| `server.allow_non_loopback_bind` | `false` | | Permission to listen beyond this machine. It requires an explicit `allow`. |
| `server.max_clients` | 16 | 1 to 256 | Per rig. A client over the limit is refused. |
| `server.idle_timeout_secs` | 600 | 1 to 86400 | A silent connection is closed. |
| `server.line_timeout_secs` | 10 | 1 to 300 | A request line that has begun must end within this. |
| `server.max_line_bytes` | 1024 | 64 to 65536 | Longest request line. An over-long line gets `RPRT -1` and the connection is closed. |
| `server.request_timeout_ms` | 2000 | 100 to 30000 | Deadline for a request that must go to the rig. |
| `server.cache_max_age_ms` | 1000 | 0 to 60000 | Reads newer than this are answered from the actor's cache. |
| `server.secrets_dir` | none | a directory, relative to the configuration file | One file per secret, a regular file readable by the owner only (mode 0600), one trailing newline dropped. Needed by `framing = "ts890-lan"`. |
| `rig.name` | required | 1 to 32 of `A-Za-z0-9_.-` | Used in logs. |
| `rig.description` | required | | Rig description file, relative to the configuration file. |
| `rig.listen` | `127.0.0.1:` 4532 plus the rig's index | `ip:port` | A non-loopback address needs `allow_non_loopback_bind`. |
| `rig.vfo_mode` | `false` | | Hamlib's `rigctld -o`: `\chk_vfo` answers 1 and response headers echo the VFO. |
| `rig.transmit` | `false` | | `true` opens the rig's transmit gate when the daemon starts. See [chapter 10](10-safety.md). |
| `rig.max_transmit_s` | library default, 180 | 1 to 3600 | The rig actor unkeys a rig that transmits longer than this (the transmit watchdog). Optional. |
| `rig.rx_low_hz`, `rx_high_hz` | 30000, 60000000 | | The coverage `\dump_state` reports. |
| `rig.tx_power_mw` | 100000 | 1 to 10000000 | The maximum power `\dump_state` reports. |
| `rig.hamlib_model` | 1 | | The model number `\dump_state` reports. |
| `transport.kind` | required | `virtual`, `serial`, `tcp` | See below. |
| virtual: `reply_delay_ms` | 0 | 0 to 10000 | How long the virtual rig takes to answer. |
| serial: `path` or `usb`, `baud`, `lines` | `lines = "managed"` | baud 300 to 4000000; `lines` is `managed` or `unmanaged` | Exactly one of `path = "/dev/ttyUSB0"` and `usb = { vid, pid, serial }`. The port is opened 8N1 with the modem lines inactive and one open attempt. `lines = "unmanaged"` is for a port that has no DTR and RTS (a pseudo-terminal such as the one `virtual_radio_pty` makes, a CDC ACM gadget): the lines are never touched. With the default a pty fails the open with `LinkDown: DTR ... Not a typewriter`; a real radio's USB port must keep `managed`. |
| serial: `baud = "auto"` or `baud_probe = [...]` | off (a fixed `baud`) | rates 300 to 4000000, no repeats | The read-only baud probe (chapter [5](05-transports.md#the-baud-probe)). `baud = "auto"` tries 4800, 9600, 19200, 38400, 57600 and 115200 in that order; `baud_probe` tries your list in your order (with `baud` absent or `"auto"`; a fixed `baud` together with a list is an error). At each rate the port is opened as for the real link, the engine's own frequency read (CI-V `03`) is sent once, and the daemon waits one description timeout for an observation; the first rate that answers is used for the real link and printed as `radioctld: rig NAME: baud probe chose RATE (tried ...)`; no answer at any rate stops the daemon with `baud probe: ... no answer ... rates tried`. The probe never sends anything but that read, never touches the transmit gate, and the startup unkey pass still runs afterwards at the chosen rate. On a pseudo-terminal the first rate always answers (a pty ignores the rate). Unverified on a real radio: bench it against the radio's CI-V baud menu item. |
| tcp: `address` | | `host:port` | A network serial server or a radio's own CAT port, on the reconnecting network link (chapter [5](05-transports.md#network-links)): one connection attempt at start (a radio that cannot be reached stops the daemon), then reconnection with backoff; the rig actor marks the link down meanwhile (transmit commands fail closed) and unkeys the rig when it is back. |
| tcp: `framing` | `raw` | `raw`, `ts890-lan` | `ts890-lan` is the Kenwood TS-890S LAN session (`##CN;`, the `##ID` login, an `ID;` keep-alive every 4 s; chapter [5](05-transports.md#the-ts-890s-over-the-lan)) around the CAT stream. |
| tcp: `lan_account`, `lan_administrator`, `lan_password_secret` | `lan_administrator = false` | | For `ts890-lan` only. `lan_account` is the radio's LAN account name (1 to 32 printable ASCII, no `;`), `lan_administrator` whether it is the administrator account, `lan_password_secret` the name of the file in `server.secrets_dir` holding the password. The password is read when the rig is brought up and held only in the session; it is never in this file, a log line or an error. A refused login (`##ID0;`), a denied connection (`##CN0;`), a missing or world-readable secret stops the daemon from starting. |
| tcp: `idle_timeout_ms` | none for `raw`; 12000 for `ts890-lan` | 1000 to 600000 | Silence on the wire after which the link is declared lost and reconnected. The TS-890S hangs up after 10 s without communication; three keep-alive intervals is the default. |

`radioctld.example.toml` in the crate root is the annotated example. Not configurable today: serial data
bits, parity, flow control and the idle level of DTR and RTS; the number of open attempts; polling
intervals (the daemon uses the library defaults); the list of capabilities (that is the description's).

### Rotators

A `[[rotator]]` section serves one rotator in Hamlib's `rotctld` text protocol, on a listener of its
own, through `radioctl-station`'s rotator engines (chapter [14](14-station.md#rotators)). A file may
hold rigs, rotators or both; names are unique within each kind, and listen addresses across both.

```toml radioctld
[[rotator]]
name = "g5500"
model = "yaesu-g-5500-gs232b"
listen = "127.0.0.1:4533"

[rotator.transport]
kind = "virtual"
```

| Key | Default | Range | Meaning |
|---|---|---|---|
| `rotator.name` | required | 1 to 32 of `A-Za-z0-9_.-` | Used in logs. |
| `rotator.model` | required | an `id` from `data/station/rotators.toml` | `generic-rotctld` (any rotator behind a Hamlib `rotctld`), `yaesu-g-5500-gs232b`, `yaesu-g-450-gs232a` (azimuth only). The loader names the known ids when it refuses one. |
| `rotator.listen` | `127.0.0.1:` 4533 plus the rotator's index | `ip:port` | 4533 is Hamlib's `rotctld` default. A second rig would also default to 4533, so a file with both gives one of them a `listen` of its own (the loader refuses the clash). A non-loopback address needs `allow_non_loopback_bind`. |
| `transport.kind` | required | `virtual`, `tcp` | `virtual` serves the model's virtual rotator inside the daemon (on a loopback socket of its own, port chosen by the kernel) and reaches it over the same link a real one uses. `tcp` is `address = "host:port"`: a `rotctld`, or a serial server in front of a GS-232 controller. The link reconnects with backoff after a loss; the first connection is one attempt, and a rotator that cannot be reached stops the daemon from starting, as a rig does. No serial transport for rotators yet. |

What the rotator listener speaks: `P az el`/`\set_pos`, `p`/`\get_pos` (two lines, two decimals;
`Azimuth:`/`Elevation:` in the extended format), `S`/`\stop`, `K`/`\park`, `_`/`\get_info` (the
description's name), `\dump_state` (the shape Hamlib 4.6.2's `rotctld` prints and `rotctl -m 2` reads on
connect: protocol version 1, model, `min_az=` to `max_el=`, `south_zero=0`, `rot_type=AzEl` or `Az`,
`done`), `q`; the `+` extended format. Every other Hamlib rotator command (`M`, `R`, levels, the locator
and distance calculations) answers `RPRT -4`; an unknown word `RPRT -1`; the raw `w` `RPRT -11`. A
position outside the model's travel is **refused** with `RPRT -1` before anything is written to the
rotator (Hamlib's `rot_set_position` answers `RIG_EINVAL` for one outside the limits), never clamped;
`K` on a model with no park position is `RPRT -11`. A rotator error maps as a rig's does: a timeout is
`-5`, a link or controller fault `-6`. A rotator has no owner and no transmit path, so nothing is released
when a client disconnects, and stopping the daemon leaves the rotator where it is (it keeps its own
target, as it would if a `rotctld` stopped). Tested against the virtual rotator only, and with Hamlib's
own `rotctl -m 2` as an oracle under `--features hamlib-oracle`; **nothing is verified against a real
rotator**. Verifying the shipped descriptions (`verified = true` per fact), serial transports, Easycomm,
Rot2Prog and DCU engines, and the relay's `rotor_steer` dispatch are still to do.

## Access control and exposure

- Every listener binds loopback by default, and the allow list defaults to loopback.
- The allow list is applied **on accept, before a byte is read**; a peer outside it is dropped. IPv4-mapped
  IPv6 addresses are normalised.
- Serving beyond loopback needs two things written down: `allow_non_loopback_bind = true` and an explicit
  `allow` list. The loader refuses the first without the second.
- The only secret the daemon handles is a TS-890S LAN password (`framing = "ts890-lan"`): it is read
  from a file in `server.secrets_dir` that only the owner can read, held in the session's
  `LanCredentials` (whose `Debug` hides it), sent in the login frame, and in no message or log.
- There is **no authentication and no encryption** in the `rigctld` protocol. Anyone the allow list admits
  can read and set the radio, and key it if `transmit = true`. Put it behind a network you control.
- **A one-shot `rigctl -m 2 ... T 1` keys the rig and unkeys it again an instant later**: the client
  disconnects, and a disconnecting owner unkeys the rig. Keep the connection open for as long as the
  rig should stay keyed, for example `(echo "T 1"; sleep 4; echo "T 0"; sleep 1) | nc 127.0.0.1 4532`,
  or an interactive `rigctl -m 2 -r 127.0.0.1:4532`.
- Bounds on a client: the client count, the line length, idle time, time to finish a started line, and
  write time (10 s). A client that breaks one is disconnected; a disconnecting client that owned the
  transmitter unkeys the rig.

## What the server speaks

The protocol is the one documented in the `rigctl`/`rigctld` man pages, as observed from Hamlib 4.6.2.
Appendix [C](C-radioctld-command-reference.md) lists every command. In short:

- Requests are one line: an optional `+` (extended response format), a one-letter command or
  `\long_name`, and blank-separated arguments. A get answers its value, one per line; a set answers
  `RPRT 0`; a failure answers `RPRT <negative code>`. Extended format adds a header, labelled values and
  the `RPRT` line.
- **Served:** `f F m M v V t T s S i I x X j J z Z l L u U b`, `1` and `\dump_caps`, `\dump_state`,
  `\chk_vfo`, `\get_powerstat`, `\get_info`, `\get_lock_mode`, `\get_vfo_info`, `\get_vfo_list`,
  `\get_split_freq_mode`, `\set_split_freq_mode`, `\get_active_tx_freq` (radioctld's own), `q`.
- **Known to Hamlib, not implemented** (`RPRT -4`): memories, channels, scan, CTCSS and DCS, repeater
  shift and offset, antenna, tuning step, parameters, DTMF, `vfo_op`, `reset` and the like. Unknown words
  are `RPRT -1`. What the rig cannot do is `RPRT -11`.
- **Never served** (`RPRT -11`): the raw `send_cmd` (`w`) and power control (`\set_powerstat`). There
  is no way to send a client's bytes to the radio.
- **Strict arguments.** A missing, surplus or malformed argument is `RPRT -1`; the parser never waits for
  another line. Hamlib's `rigctld` waits.

### VFOs

Every VFO-taking command accepts the VFO argument or not, in either mode. Tokens (case-insensitive):
`VFOA`, `Main`, `MainA` (VFO A), `VFOB`, `MainB` (VFO B), `currVFO` (the VFO selected by `V`), `RX` (A) and
`TX` (the transmit VFO: B in split on most rigs, else A; what the rig reports where its protocol says). `VFOC`, `SubA`, `SubB`, `Sub`, `MEM`, `Mem` and `None` name VFOs radioctl
does not have: `RPRT -11`. With `vfo_mode = true`, `\chk_vfo` answers 1. An extended-format header echoes
the VFO token the client sent, or `currVFO` when `vfo_mode` is true and none was sent
(`get_freq: VFOA`, `set_freq: VFOA:14074000`); with `vfo_mode = false` and no token it shows none.

The VFO selected by `V` is radioctld's own state, per rig and shared by its clients. The radio's
selected VFO is never changed by it: radioctl addresses VFOs explicitly. Split is "VFO A receives, VFO B
transmits"; `S 1 VFOB` turns it on, and `S` with any other transmit VFO is `RPRT -11`.

### Differences from Hamlib's `rigctld`, and why

| Difference | Why |
|---|---|
| PTT has one owner: another client's `T 1` or `b` is `RPRT -9` until the owner unkeys or disconnects, which unkeys the rig. `b` (Morse) claims the transmitter as `T 1` does. Anyone may unkey, and anyone may `\stop_morse`. | One transmitter, one owner; a vanished client must not leave a carrier on the air; a stop path has no owner check. |
| Transmit commands are refused up front with `RPRT -9` while the rig's transmit gate is closed or its link is down; nothing is queued. `\dump_state` advertises a transmit range only when the gate is open. | Fail closed. |
| `rig.transmit = false` is the default. | The gate is opened by the operator, never implied. |
| The raw `send_cmd` is not served. | An escape hatch is a separate, explicit capability, never on by default. |
| Arguments are checked strictly; no waiting for more lines. | A network server must not hang on malformed input. |
| Loopback bind and allow list by default; the line, client and time bounds. | Safe for every user by default. |
| `X` and `\set_split_freq_mode` accept only the mode the rig is already in. | A transmit mode different from the receive mode is not modelled. |
| Passbands for rigs without filter control are the mode's typical width. | Honest about what is unknown; `M` with a passband of 0 or -1 keeps the rig's width. |
| Levels are scaled to 0 to 1 (or the rig's own integer for keyer speed) by the ranges the engines enforce. `names::level_range` holds them, and `tests/levels.rs` pins them to the engines. | Hamlib's levels are floats; the engines work in the radio's units. |
| Levels served: `AF`, `RF`, `RFPOWER`, `MICGAIN`, `KEYSPD`, `MONITOR_GAIN`, `RAWSTR`, `ALC`, `SWR` (the last two from raw meters through unverified calibration points), `ATT` (decibels) and `PREAMP` (a step number, not decibels), each only where the description has the capability and the engine has a range for it. The CI-V keyer speed (`14 0C`) is served in words per minute, 6 to 48; the legacy Yaesu family has no levels; the Flex levels are percentages. | Only what an engine carries. |
| Functions served: `RIT` and `XIT`. Others Hamlib knows are `RPRT -11`. | Same. |
| `\get_active_tx_freq` exists. `f` keeps Hamlib's meaning (the frequency of the VFO asked for). | Issue 138: a follower needs the transmit frequency without guessing which VFO to ask for (chapter [4](04-api.md)). |
| One listener per rig. | As one `rigctld` per radio. |

The protocol subset was compared with Hamlib 4.6.2 only. `\dump_state` prints `agc_levels=`,
`ctcss_list=` and `dcs_list=` only when non-empty, as Hamlib's `rigctld` does (an empty one was an
"invalid dumpcaps line" to `rigctl -vvv`); no description has any of the three, so none is printed.

## Embedding the daemon in your own program

The same code the binary runs is a library. `crates/radioctld/examples/embed_daemon.rs` starts a
virtual KX3 from a configuration text on the caller's runtime and talks to it as a client would. It
shows the closed gate at the end: `T 1` answers `RPRT -9`.

```rust example=embed_daemon
//! radioctld as a library: the same code the `radioctld` binary runs, started on the caller's
//! runtime from a configuration text, with one rigctld-protocol client talking to it.
use std::error::Error;
use std::path::Path;
use std::time::Duration;

use radioctl_core::rig::RigOptions;
use radioctld::config::Config;
use radioctld::daemon;
use radioctld::rigs::load_description;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::runtime::Handle;
use tokio::time::timeout;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let rigs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs");
    let text = r#"
[[rig]]
name = "kx3"
description = "elecraft-kx3.toml"
listen = "127.0.0.1:0"
[rig.transport]
kind = "virtual"
"#;
    let config = Config::parse(text, &rigs)?;
    let descriptions = config
        .rigs
        .iter()
        .map(|rig| load_description(&rig.description))
        .collect::<Result<Vec<_>, _>>()?;
    let daemon = daemon::start(
        &Handle::current(),
        &config,
        descriptions,
        RigOptions::default,
    )
    .await?;
    let address = daemon.rigs[0].server.local_addr();
    println!("radioctld: kx3 listening on loopback");

    let stream = TcpStream::connect(address).await?;
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    for request in ["F 14074000", "f", "t", "T 1"] {
        writer.write_all(format!("{request}\n").as_bytes()).await?;
        // Plain format: a get answers its value, a set answers `RPRT 0`, a refusal `RPRT <code>`.
        let first = timeout(Duration::from_secs(5), lines.next_line()).await??;
        println!("> {request}\n< {}", first.unwrap_or_default());
    }
    daemon.stop().await;
    Ok(())
}
```

It prints:

```text
radioctld: kx3 listening on loopback
> F 14074000
< RPRT 0
> f
< 14074000
> t
< 0
> T 1
< RPRT -9
```

A program embedding the daemon can reach each rig's `RigHandle` through
`daemon.rigs[i].service.rig()`, for example to subscribe to the event stream of its own (build the rig
with your own `RigOptions` through the `options` closure of `daemon::start`).
