# 1. Introduction and concepts

## What radioctl is

radioctl controls radios: it reads and sets frequency, mode and PTT, notices when someone turns a
knob on the front panel, and tells your program. It is a Rust workspace of four crates in
`daemons/radioctl/`:

| Crate | What it holds |
|---|---|
| `radioctl-core` | Typed values (`Hz`, `Mode`, `Vfo`), the capability model, errors, the rig actor and its `RigHandle`, the event stream, polling, the rig description file format, and the transports (serial, USB-serial, TCP). |
| `radioctl-protocols` | One sans-IO protocol engine per radio family (Icom CI-V, Kenwood, Elecraft, Yaesu newcat, Yaesu legacy, FlexRadio SmartSDR, and the control plane of OpenHPSDR/ANAN radios), the virtual rigs that stand in for radios in tests, and the transcript replay. |
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

It is **not** a replacement for Hamlib today. Hamlib 4.6.2 lists <!--count:models-->313<!--/count--> radio models ([appendix A](A-capability-matrix.md));
radioctl has descriptions for <!--count:described-->283<!--/count--> of the <!--count:real_radios-->283<!--/count--> that are real radios (<!--count:not_applicable-->30<!--/count--> more are not radios or have no transport radioctl speaks; [RIG_SUPPORT.md](../RIG_SUPPORT.md) has the list), none verified on hardware yet. It is not a drop-in `libhamlib`; a C ABI exists as a sketch (`radioctl-capi`, [chapter 14](14-station.md#the-c-abi))
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
  a status event stream (the only status-change API; there are no callbacks), a derived
  `active_tx_frequency` (Hamlib issue 138), Icom scope operations with a sweep stream, a transmit
  watchdog in the actor (maximum transmit time, optional caller heartbeat, alarm events), and a
  fail-closed unkey when a transmit command goes unanswered, when the last handle is dropped and when a
  link comes back; engines for Icom CI-V (also used for Xiegu), Kenwood ASCII, Elecraft ASCII, Yaesu
  newcat ASCII, Yaesu legacy five-byte CAT, the FlexRadio SmartSDR control plane, the OpenHPSDR (ANAN)
  control plane, the receiver families, a radiod receive channel, about thirty small vendor engines and
  USB devices; virtual rigs; serial, USB-serial-by-id, TCP/UDP and USB-device transports with reconnect;
  the splitter; `radioctld`; the station layer; the rig description format with provenance and
  verification states; transcript replay; differential tests against Hamlib's own binaries; and the
  self-test a volunteer runs on a radio. The number of radios with a description is in
  [`RIG_SUPPORT.md`](../RIG_SUPPORT.md): <!--count:described-->283<!--/count--> of the
  <!--count:real_radios-->283<!--/count--> real radios among Hamlib 4.6.2's <!--count:models-->313<!--/count--> models.
- **Unverified:** nearly all of the above against real radios. Exactly <!--count:hardware_verified-->1<!--/count-->
  description has `hardware-verified` facts: the Elecraft KX3, bench run on 2026-11-05 including a keyed
  transmission into a dummy load. The Icom files were read from Hamlib's source or from manuals found on
  public mirrors, not from Icom's own downloads ([chapter 12](12-provenance-and-licensing.md) says why),
  and most transcripts are hand-written from a manual or from our own simulator. A passing test means the description, the engine and the virtual rig agree with each
  other, not that they agree with a radio.
- **Planned, not in the code:** ownership modes, scope for non-Icom radios, FlexRadio streams (meters,
  spectrum, audio) and discovery on the wire, the OpenHPSDR data plane (I/Q) and the Kenwood (other than
  the TS-890S session) and Icom LAN radios, the rest of the C ABI, and the remaining items of [chapter 13](13-roadmap.md), which is the
  complete list.

Only the KX3 bench run has keyed a real transmitter. Everything that can transmit fails closed behind
the gate ([chapter 10](10-safety.md)), and no test in the repository keys a transmitter.
