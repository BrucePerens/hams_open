# 1. Introduction and concepts

## What radioctl is

radioctl controls radios: it reads and sets frequency, mode and PTT, notices when someone turns a
knob on the front panel, and tells your program. It is a Rust workspace of four crates in
`daemons/radioctl/`:

| Crate | What it holds |
|---|---|
| `radioctl-core` | Typed values (`Hz`, `Mode`, `Vfo`), the capability model, errors, the rig actor and its `RigHandle`, the event callback and stream, polling, the rig description file format, and the transports (serial, USB-serial, TCP). |
| `radioctl-protocols` | One sans-IO protocol engine per radio family (Icom CI-V, Kenwood, Elecraft, Yaesu newcat, Yaesu legacy, FlexRadio SmartSDR), the virtual rigs that stand in for radios in tests, and the transcript replay. |
| `radioctl-splitter` | Virtual IC-7300 CI-V and TS-2000 CAT endpoints (pseudo-terminals or TCP) so several outside programs can share one radio. |
| `radioctld` | A server for the Hamlib `rigctld` text protocol, as a library plus the `radioctld` binary. |

`radioctl-protocols` depends on `radioctl-core`; the actor in core is generic over a `ProtocolEngine`
trait, so core never depends on the engines. `radioctl-splitter` depends on both; `radioctld`
depends on all three and uses the splitter's PTT arbiter.

## Why it exists, and why not Hamlib

The reasons are in the design proposal (`hams_com/docs/proposals/RADIOCTL_RUST_RIG_CONTROL_LIBRARY.md`,
section 1; that repository is not public, so this paragraph summarises it and the claims about the
relay are the proposal's, not checked here). The hams.com relay links `libhamlib` in its own process
and serialises every call with one global lock, so a slow answer from one radio delays every other
radio, including a PTT release. Hamlib is a C library parsing serial replies inside the relay's
address space, which the relay's zero-privilege design wants out of that process. radioctl is built
so that:

- each radio has its own actor and there is no global lock or global state;
- the API is typed (frequencies are integer hertz, errors say whether to retry) and async-only;
- transmit is refused unless a per-rig gate is opened, and the default is closed;
- a driver and its test double come from the same data file and cannot drift apart;
- every fact about a radio says where it came from and what has been checked.

It is **not** a replacement for Hamlib today. Hamlib 4.6.2 lists 313 radio models ([appendix A](A-capability-matrix.md));
radioctl has descriptions for 21. It is not a drop-in `libhamlib`, and a C ABI is planned but absent
([chapter 13](13-roadmap.md)). The plan is hybrid: radios move to radioctl one at a time, once a
description has been verified on real hardware, and Hamlib stays the fallback and the test oracle.

## Concepts

| Term | Meaning |
|---|---|
| **Rig** | One radio as radioctl sees it: a protocol engine, a transport and a description, run by an actor. |
| **Rig actor** | The task that owns the transport and the engine and runs one command at a time. Rigs share nothing. |
| **`RigHandle`** | A cloneable, `Send + Sync` handle. Anything that talks to a rig does so through a handle. |
| **`Op`** | A command to a rig (`GetFrequency(Vfo)`, `SetPtt(bool)`, ...). |
| **`Observation`** | A value a rig reported, asked for or not. The actor caches the latest of each kind. |
| **`Capability`** | One thing a rig can do. The set comes from its description; operations outside it are refused. |
| **Protocol engine** | A sans-IO state machine: `encode(op)` gives bytes to send, `feed(bytes)` gives decoded events. It never touches a port, a runtime or a clock. |
| **Rig description** | A TOML file per model with identity, timing, capabilities and mode codes, and a provenance and `verified` record for every datum. |
| **Virtual rig** | A simulated radio generated from a description, used by tests and by `radioctld`'s `virtual` transport. |
| **Transcript** | A recorded or hand-written list of wire exchanges, replayed against the engine and the virtual rig. |
| **Transmit gate** | A per-rig switch, closed by default. While it is closed, any command that can put RF on the air is refused. |
| **Splitter** | Presents one radio as an IC-7300 or TS-2000 to several programs at once. |
| **`radioctld`** | The `rigctld`-compatible network server. |

A [glossary](B-glossary.md) covers the ham-radio terms (VFO, split, RIT, CI-V and so on).

## Status

Phase 0/1 foundation, as the crate README says. What is implemented:

- **Implemented:** the rig actor with a priority queue, deadlines, cache with age, adaptive polling,
  status callback and event stream, a derived `active_tx_frequency` (Hamlib issue 138), and a fail-closed
  unkey when a transmit command goes unanswered or the last handle is dropped; engines for Icom CI-V (also used for Xiegu), Kenwood ASCII, Elecraft ASCII,
  Yaesu newcat ASCII, Yaesu legacy five-byte CAT and the FlexRadio SmartSDR control plane; virtual rigs; serial, USB-serial-by-id and TCP transports
  with reconnect for serial; the splitter; `radioctld`; the rig description format with provenance;
  transcript replay; differential tests against Hamlib's own binaries.
- **Unverified:** all of the above against real radios. No radio description has a single
  `hardware-verified` fact. The Icom files were read from Hamlib's source, not Icom's manuals
  ([chapter 12](12-provenance-and-licensing.md) says why), and every transcript is hand-written from
  a manual or from our own simulator. A passing test means the description, the engine and the virtual
  rig agree with each other, not that they agree with a radio.
- **Planned, not in the code:** a dead-man timer, a maximum transmit duration, unkey on link loss,
  scope operations through the API, the Kenwood TS-890S dialect,
  FlexRadio streams (meters, spectrum, audio) and discovery on the wire, HPSDR/ANAN and the Kenwood and
  Icom LAN radios as radioctl rigs, a C ABI, rotators and amplifiers, and external switches. [Chapter 13](13-roadmap.md) is the complete list.

There is no transmit path to real hardware that has been exercised. Everything that can transmit
fails closed behind the gate ([chapter 10](10-safety.md)), and no test in the repository keys a
transmitter.
