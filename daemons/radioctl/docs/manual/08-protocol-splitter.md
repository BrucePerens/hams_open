# 8. The protocol splitter

Most ham software wants to own a serial port and speak one radio's protocol. The splitter lets several
such programs use one radio at the same time. It presents a virtual serial port (a pseudo-terminal) or
a TCP endpoint that speaks **IC-7300 CI-V** or **TS-2000 CAT** to outside software, while every request
becomes an operation on the one rig actor that owns the real radio, whatever its make. A logger that
only knows the TS-2000 can control a Yaesu or an Elecraft, with its neighbour fldigi on a second port.

Status: **implemented** as a library crate, `radioctl-splitter`, and **unverified** against real radios
and real programs. Its tests run external-program-style clients on real pseudo-terminals and TCP
sockets, not WSJT-X or fldigi.

**There is no splitter command.** `radioctld` does not start one. The crate is a library; the
shipped programs that use it are examples, and the one for ordinary use is
`crates/radioctld/examples/splitter_pty.rs` (below). A daemon that runs both, sharing one actor, is
**planned**; `radioctld` exposes each rig's handle (`RigService::rig()`), so a program embedding both
can attach a splitter to the daemon's actor.

## Personalities

A personality is a sans-IO protocol spoken to one client (`Personality`): it parses a client's bytes into
requests, renders replies, and renders the unsolicited bytes that client expects. It never touches the
rig, a socket or a clock. Two exist:

| Personality | Selected by | Commands answered |
|---|---|---|
| IC-7300 CI-V | `Protocol::Civ { address, echo }` (address `0x94` for the IC-7300) | `03` read frequency, `05` set frequency, `25 00`/`25 01` read or set the selected or unselected VFO, `04` and `26 00` read mode, `06` and `26 00` set mode, `1C 00` read or set PTT, `19 00` read ID, `15 02` S-meter, `15 11` power meter, `17` Morse text. Anything else is NG. |
| TS-2000 CAT | `Protocol::Ts2000` | `FA`, `FB` read or set a VFO (11 digits), `MD` read or set mode, `TX`, `RX` key and unkey, `SM0` S-meter, `ID` (019), `AI` auto-information for this client, `PS` (power is on). A refusal or an unknown command is `?;`. |

Requests reach the rig as one of: read or set a VFO's frequency, read or set the mode, read or set
PTT, read signal, read power, send CW. Split, RIT, XIT, filter and levels are **not** presented; a
program that needs them will not work through the splitter yet.

Limits, all from the code:

- The mode table is the IC-7300's (or the TS-2000's) regardless of the real radio. Data modes map to
  the sideband they sit on (`Data` to USB, `DataR` to LSB). Modes the emulated radio lacks are refused.
- The TS-2000 personality answers `IF;` with `?;`, although the Kenwood engine can decode `IF`. Software
  that needs transmit state from `IF` will not work against this endpoint.
- S-meter values pass through in the real radio's units, not the emulated radio's scale.
- It does not report the transmit frequency while keyed in split mode (the proposal's issue-138
  requirement for the personalities; chapter [4](04-api.md) says what exists).

## Echo and unsolicited traffic

Each client has its own state.

- **CI-V echo.** With `echo = true`, each complete frame a client sends is first echoed back to that
  client, as a single-wire CI-V interface does; with `false`, not. The echo is per client, never to
  another.
- **Unsolicited frames.** Every client is sent the radio's changes as that protocol's unsolicited
  traffic: CI-V `00` (frequency) and `01` (mode) to the broadcast address when VFO A or the mode
  changes; for TS-2000, `FA`, `FB` and `MD` only to clients that sent `AI1`. A field that a request read
  for the first time is a reading, not a change, and is not pushed; a value the radio reported on its
  own always is.
- **Reads come from the cache** when it is no older than `SplitterConfig::max_age` (default 5 s),
  otherwise from the radio. Several programs polling cost the radio almost nothing and never contend
  for the port. A set updates the cache at once, so the next read in any client sees it.

## PTT ownership

One transmitter, one owner (`arbiter::PttOwner`), shared with `radioctld`.

- The first client to key owns the transmitter. Another client's keying request, or its Morse text, is
  refused (CI-V NG, TS-2000 `?;`) until the owner unkeys or goes away. The refusal is counted as a
  `ptt_conflict`. Morse puts RF on the air, so a client that sends Morse claims the transmitter exactly as
  a key does (the claim is given back if the rig refuses), and a Morse sender that goes away unkeys the
  rig.
- Unkeying is open to every client and clears the ownership.
- A client that disconnects while owning the transmitter unkeys the rig. For a pty, a program closing it
  counts as disconnecting; the endpoint stays for the next program.
- The claim is made before the rig is asked and given back if the rig refuses.
- The rig's transmit gate still applies to every keying request. With the gate closed (the default),
  every program is refused, and nothing is written to the radio ([chapter 10](10-safety.md)).

## Using it

```rust signatures
pub fn new(rig: RigHandle, config: SplitterConfig) -> Splitter

pub fn attach_stream<S: Transport>(&self, runtime: &Handle, stream: S) -> ClientHandle

pub fn serve_tcp(&self, runtime: &Handle, listener: TcpListener) -> JoinHandle<()>

pub fn attach_pty(
    splitter: &Splitter,
    runtime: &Handle,
    link: Option<PathBuf>,
) -> Result<(PtyEndpoint, ClientHandle), RigError>
```

`attach_pty` creates a pseudo-terminal, attaches it as a persistent client, and optionally links a
path to its slave (an existing file at that path is replaced). **Each program that should be a separate
client needs its own endpoint**, because a pty carries one client. A TCP listener makes every
connection its own client. Bind a listener to loopback unless the network is meant to reach the radio:
the splitter has no allow list of its own, unlike `radioctld`.

`splitter_tcp` runs a TS-2000 endpoint over loopback TCP in front of a virtual KX3 and has two
"programs" share it. The tests run it:

```rust example=splitter_tcp
//! The protocol splitter: a Kenwood TS-2000 CAT endpoint on a loopback TCP port in front of
//! one rig actor (a virtual KX3). Two "programs" connect and share the radio.
use std::error::Error;
use std::time::Duration;

use radioctl_core::rig::{spawn_current, RigOptions};
use radioctl_core::rigfile;
use radioctl_protocols::elecraft::ElecraftEngine;
use radioctl_protocols::io::spawn_virtual_rig;
use radioctl_protocols::virtual_rig::VirtualRig;
use radioctl_splitter::session::{Protocol, Splitter, SplitterConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Handle;
use tokio::time::timeout;

/// Sends one CAT command and returns the `;`-terminated answer.
async fn ask(program: &mut TcpStream, command: &str) -> Result<String, Box<dyn Error>> {
    program.write_all(command.as_bytes()).await?;
    let mut answer = Vec::new();
    let mut byte = [0u8; 1];
    while answer.last() != Some(&b';') {
        timeout(Duration::from_secs(5), program.read_exact(&mut byte)).await??;
        answer.push(byte[0]);
    }
    Ok(String::from_utf8(answer)?)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let description = rigfile::parse(include_str!("../../../rigs/elecraft-kx3.toml"))?.description;
    let radio = VirtualRig::from_description(&description)?;
    let (transport, _front_panel) = spawn_virtual_rig(&Handle::current(), radio, Duration::ZERO);
    let rig = spawn_current(
        ElecraftEngine::new(description)?,
        transport,
        RigOptions::default(),
    )?;

    // Every client of the splitter goes through the one actor queue of `rig`.
    let splitter = Splitter::new(rig, SplitterConfig::new(Protocol::Ts2000));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = splitter.serve_tcp(&Handle::current(), listener);

    let mut logger = TcpStream::connect(address).await?;
    let mut digital = TcpStream::connect(address).await?;
    println!("logger sees   {}", ask(&mut logger, "FA;").await?);
    ask(&mut digital, "FA00007074000;MD2;ID;").await.ok();
    println!("digital sees  {}", ask(&mut digital, "FA;").await?);
    println!("logger sees   {}", ask(&mut logger, "FA;").await?);

    // Keying is refused while the transmit gate is closed: the TS-2000 answers `?;`.
    println!("TX; answers   {}", ask(&mut logger, "TX;").await?);
    server.abort();
    Ok(())
}
```

It prints:

```text
logger sees   FA00014060000;
digital sees  FA00007074000;
logger sees   FA00007074000;
TX; answers   ?;
```

To give real programs a port on Linux or macOS, `splitter_pty` makes one symlink per program:

```sh
cargo run -p radioctld --example splitter_pty -- ts2000 elecraft-kx3 virtual /tmp/rig-logger /tmp/rig-fldigi
cargo run -p radioctld --example splitter_pty -- civ elecraft-kx3 /dev/ttyUSB0:38400 /tmp/rig-wsjtx
```

Point each program at its own path, as a serial port, with the protocol's usual settings. The first
line uses the virtual radio; the second drives a radio on a real port and is not run by any test.
`splitter_pty` is checked by hand against a virtual KX3 with a script opening two links; no test runs
it, and no test uses WSJT-X, fldigi or a logger.

## Platforms

- **Linux and macOS:** the pty module (`radioctl_splitter::pty`) is `#[cfg(unix)]`.
- **Windows** has no pty. Use the TCP endpoint, or the `radioctld` network server, or put a virtual COM
  driver (the relay uses `com0com`) between a program and the TCP endpoint. Radioctl provides none of
  those; the combination is **unverified** and no Windows build has been made.
