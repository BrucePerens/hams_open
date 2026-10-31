# 2. Quick start

## Build

The toolchain is pinned by `rust-toolchain.toml` (Rust 1.98.1 with rustfmt and clippy). On the
development machine follow the repository's rule of a shared target directory and `nice`; AGENTS.md
explains why.

```sh
cd daemons/radioctl
export CARGO_TARGET_DIR=$HOME/.cache/hams-cargo-target-radioctl
nice -n 10 cargo build --release -p radioctld
```

The serial port code needs no `libudev`: Linux USB serial ports are found by reading sysfs.

## Run radioctld with a virtual radio

A virtual radio is an in-memory stand-in generated from a rig description. It needs no hardware, so
it is the safe way to try everything. Save this as `virtual.toml`; the `description` path is relative
to the configuration file.

```toml radioctld
[[rig]]
name = "kx3"
description = "rigs/elecraft-kx3.toml"
listen = "127.0.0.1:4532"
[rig.transport]
kind = "virtual"
```

```sh
radioctld --config virtual.toml --check     # validates the file and every description it names
radioctld --config virtual.toml
```

`--check` prints `radioctld: configuration ok, 1 rig(s)` and exits without listening. The daemon logs
`radioctld: kx3 listening on 127.0.0.1:4532` and stops on Ctrl-C. Chapter [9](09-radioctld.md) has the
configuration reference.

## Connect with rigctl

Hamlib's `rigctl` talks to any `rigctld` server as model 2 ("NET rigctl"):

```sh
rigctl -m 2 -r 127.0.0.1:4532 f              # 14060000
rigctl -m 2 -r 127.0.0.1:4532 F 14074000     # set VFO A
rigctl -m 2 -r 127.0.0.1:4532 m              # CW, then the passband 500
rigctl -m 2 -r 127.0.0.1:4532 T 1            # Command rejected by the rig
```

The last command is refused on purpose: the configuration has not set `transmit = true`, so the rig's
transmit gate is closed ([chapter 10](10-safety.md)). The same happens to any program that asks.

## Connect WSJT-X, fldigi or a logger

Program setting: radio "Hamlib NET rigctl" (model 2), network server `127.0.0.1:4532`. To key the
transmitter through the daemon, the configuration must say `transmit = true` for that rig, and
that is a decision about a real transmitter that this manual does not make for you.

**Unverified:** radioctld's protocol subset was compared with Hamlib 4.6.2's own `rigctl` client
(the oracle tests, chapter [11](11-testing.md)). It has not been run against WSJT-X, fldigi or any
logger in this repository.

## A real radio

Replace the transport with a serial port or a USB id, and pick the description of your radio:

```toml radioctld
[[rig]]
name = "ic7300"
description = "rigs/icom-ic-7300.toml"
[rig.transport]
kind = "serial"
usb = { vid = 0x10c4, pid = 0xea60 }
baud = 19200
```

The ids here are the common CP210x bridge, shown as an illustration; read yours with `lsusb`. Every
description is unverified, so start by reading only. Chapter [5](05-transports.md) covers ports and
unplugging.

## Rehearse a real-radio session with no radio

The example `virtual_radio_pty` puts the virtual radio of a description on a pseudo-terminal, so
`radioctld`'s serial path, Hamlib's `rigctl` and your own tools see a serial port that answers like the
radio. Nothing opens a real device. Run the programs from the directory that holds `rigs/` (or set
`RADIOCTL_RIGS`); the front panel and the cable are typed on its standard input:

```sh
mkfifo ctl; exec 3<>ctl                       # keeps standard input open
virtual_radio_pty icom-ic-7300 /tmp/ic7300-pty wire.log <&3 &
echo "freq 14250000" >&3                      # a hand on the tuning knob
echo "unplug" >&3; echo "plug" >&3            # the cable pulled and pushed back
```

```toml radioctld
[[rig]]
name = "ic7300"
description = "rigs/icom-ic-7300.toml"
listen = "127.0.0.1:4532"
hamlib_model = 3073
[rig.transport]
kind = "serial"
path = "/tmp/ic7300-pty"
baud = 19200
lines = "unmanaged"
```

`lines = "unmanaged"` is needed because a pty has no DTR or RTS; with a real radio leave it out. The
file `wire.log` records every byte in both directions as hexadecimal. Hamlib's own driver can be
pointed at the same port: `rigctl -m 3073 -r /tmp/ic7300-pty -s 19200 f`.

## The bench console: read and set every field, watch events, record scope sweeps

`radioctld` serves what the `rigctld` protocol has words for. Meters beyond the S-meter, filter width,
transceive, split and RIT state, the scope and the source of each status change are reached through
the example `bench_console`, a line-oriented console on the library API. It is receive-only by
construction: it has no key, Morse or gate command.

```sh
bench_console --log session.txt --wire wire.txt icom-ic-7300 /dev/ttyUSB0 19200   # a real port
bench_console --unmanaged icom-ic-7300 /tmp/ic7300-pty 19200               # a pty (see above)
bench_console icom-ic-7300 virtual                                        # no port at all
```

Type `help`. Every reply, every status change (`event source=Poll Frequency(A): ... -> ...`, with the
source that found it) and every scope sweep is printed with its time and copied to the log file; the
failure of a command prints the last wire lines. `--wire FILE` taps the link itself: one line per read or write, `SECONDS tx|rx HEX`, both directions, which is the raw material of a transcript fixture (scrub serial numbers before keeping one). `frames 20 sweeps.csv` collects twenty sweeps and
prints the frame rate. `radioctld` and the console both hold the port exclusively: run one at a time.

## Use it from Rust in 30 lines

The program below builds the same virtual KX3, spawns a rig actor for it on your runtime, reads and
sets frequency and mode, and shows that keying is refused while the gate is closed. It is the file
`crates/radioctld/examples/quickstart.rs`, built and run by the tests.

```rust example=quickstart
//! Quick start: a virtual Elecraft KX3 behind a rig actor, driven through a `RigHandle`.
use std::error::Error;
use std::time::Duration;

use radioctl_core::rig::{spawn_current, Freshness, RigOptions};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Mode, Vfo};
use radioctl_protocols::elecraft::ElecraftEngine;
use radioctl_protocols::io::spawn_virtual_rig;
use radioctl_protocols::virtual_rig::VirtualRig;
use tokio::runtime::Handle;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    // The rig description is data; the virtual rig and the engine are both built from it.
    let description = rigfile::parse(include_str!("../../../rigs/elecraft-kx3.toml"))?.description;
    let radio = VirtualRig::from_description(&description)?;
    let (transport, _front_panel) = spawn_virtual_rig(&Handle::current(), radio, Duration::ZERO);
    // The caller's runtime runs the actor; the library creates none.
    let rig = spawn_current(
        ElecraftEngine::new(description)?,
        transport,
        RigOptions::default(),
    )?;

    let frequency = rig.frequency(Vfo::A, Freshness::Fresh).await?;
    println!("VFO A: {} Hz", frequency.value.0);
    rig.set_frequency(Vfo::A, Hz(14_074_000)).await?;
    rig.set_mode(Mode::Usb).await?;
    let mode = rig.mode(Freshness::Cached).await?;
    println!("mode: {:?}", mode.value);

    // Transmitting is refused until someone opens the transmit gate.
    match rig.set_ptt(true).await {
        Err(error) => println!("keying refused: {:?}", error.kind),
        Ok(()) => return Err("the closed gate let a transmit command through".into()),
    }
    Ok(())
}
```

It prints:

```text
VFO A: 14060000 Hz
mode: Usb
keying refused: TransmitGateClosed
```

Run it with `cargo run -p radioctld --example quickstart`. A program of your own needs
`radioctl-core`, `radioctl-protocols` and `tokio` (features `rt` and `macros`; the time driver must be
enabled, which `#[tokio::main]` does). Chapter [4](04-api.md) explains each call, and chapter
[5](05-transports.md) replaces the virtual transport with a serial port.
