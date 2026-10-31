# 10. Safety

radioctl controls transmitters. This chapter says what it does to keep a transmitter from keying when
nobody decided it should, what the larger safety plan asks for, and, plainly, what is not done. Read
the last section before relying on any of it.

## The rules the design follows

- **Fail closed.** A command that can put RF on the air is refused unless a per-rig gate has been opened
  with a permit. The gate starts closed.
- **Unkeying is always allowed** and takes the fastest path.
- **One transmitter, one owner.** Several clients can read a rig; only one at a time can key it.
- **No hardware safety net is assumed** to exist on the other end of the cable.
- **No `unsafe` and no panics** in library code (workspace lints: `unsafe_code = "forbid"`,
  `unwrap_used`, `expect_used` and `panic` denied). The one exception is the C ABI crate
  `radioctl-capi`, which must hold raw pointers: it denies `unsafe_code` crate-wide, allows it in one
  module (`ffi.rs`), keeps the other denials, and catches every panic at the boundary; every byte parser uses checked access, because reply
  parsing is the attack surface.

## The transmit gate (implemented, unverified on hardware)

`Op::transmits()` is true for `SetPtt(true)` and `SendCw(_)` and for nothing else; `Capability::SetPtt`
and `SendCw` are the transmit-capable capabilities. For such an op:

1. `RigHandle` refuses it with `TransmitGateClosed` before it takes a queue slot (the check in `submit`).
2. The actor checks again when the command runs, so a command queued before someone closed the gate is
   still refused. The test `a_transmit_command_already_queued_is_refused_when_the_gate_closes` pins this.
3. A transmit command is never retried, whatever the description's `retries` says.

The gate is per rig and opened only by a permit that records who decided:

```rust signatures
pub fn open_transmit_gate(&self, permit: TransmitPermit)

pub fn close_transmit_gate(&self)

pub fn new(granted_by: impl Into<String>) -> TransmitPermit
```

There is no boolean shortcut. `radioctld` opens the gate only when its configuration says
`transmit = true`; the splitter and the library never open it by themselves.
`crates/radioctld/examples/transmit_gate.rs` shows the lifecycle on a virtual radio, and the tests run it:

```rust example=transmit_gate
//! The transmit gate on a virtual radio: closed by default, opened with a permit, unkeying
//! always allowed. Nothing here touches a real transmitter.
use std::error::Error;
use std::time::Duration;

use radioctl_core::error::ErrorKind;
use radioctl_core::rig::{spawn_current, Freshness, RigOptions, TransmitPermit};
use radioctl_core::rigfile;
use radioctl_protocols::elecraft::ElecraftEngine;
use radioctl_protocols::io::spawn_virtual_rig;
use radioctl_protocols::virtual_rig::VirtualRig;
use tokio::runtime::Handle;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let description = rigfile::parse(include_str!("../../../rigs/elecraft-kx3.toml"))?.description;
    let radio = VirtualRig::from_description(&description)?;
    let (transport, virtual_control) = spawn_virtual_rig(&Handle::current(), radio, Duration::ZERO);
    let rig = spawn_current(
        ElecraftEngine::new(description)?,
        transport,
        RigOptions::default(),
    )?;

    let refused = rig.set_ptt(true).await.err().map(|e| e.kind);
    println!("gate closed: keying -> {refused:?}");
    assert_eq!(refused, Some(ErrorKind::TransmitGateClosed));
    assert_eq!(rig.transmit_permit(), None);

    // The permit records who decided. There is no boolean shortcut.
    rig.open_transmit_gate(TransmitPermit::new("manual example: virtual radio"));
    rig.set_ptt(true).await?;
    println!(
        "gate open: PTT reads {}",
        rig.ptt(Freshness::Fresh).await?.value
    );
    println!(
        "virtual radio is transmitting: {}",
        virtual_control.state().ptt
    );

    // Unkeying uses the emergency path and does not need the gate.
    rig.close_transmit_gate();
    rig.set_ptt(false).await?;
    println!(
        "gate closed again: unkeyed, virtual radio transmitting: {}",
        virtual_control.state().ptt
    );
    Ok(())
}
```

Its output:

```text
gate closed: keying -> Some(TransmitGateClosed)
gate open: PTT reads true
virtual radio is transmitting: true
gate closed again: unkeyed, virtual radio transmitting: false
```

**Closing the gate does not unkey a radio that is already keyed.** It blocks later transmit commands.
Whoever closes the gate for safety must also send `set_ptt(false)`.

## Unkeying, stopping Morse, and preemption

`Op::SetPtt(false)` and `Op::StopCw` are the emergency commands (`Op::is_emergency()`). They go through a
separate channel that the actor reads first, wake the actor, and abandon a command that is waiting for a
reply (that command ends in `Cancelled`). They never interrupt a write already in progress. They need no
gate and skip inter-command spacing. The tests `ptt_off_abandons_a_slow_read_on_the_same_rig`,
`a_slow_rig_does_not_delay_another_rigs_ptt_off` and
`stop_morse_jumps_the_queue_needs_no_gate_and_leaves_the_key_alone` cover the same-rig, cross-rig and
Morse cases; a blocked or panicking application callback cannot delay them because callbacks run on
their own thread.

`StopCw` clears the radio's CW text buffer and ends the element being sent; it is the stop path for Morse
that `SendCw` started (or that another program or the front panel started). It is not an unkey: a radio
held keyed by PTT stays keyed, the cache is not touched, and the watchdog stays armed until a poll or a
transceive report shows the radio receiving. Only the unkey is coalesced when the emergency queue is
full and only an unkey's failure is `Alarm::UnkeyFailed` (`Op::is_unkey()`). The command per engine, all
unverified on hardware: CI-V `17 FF` (Hamlib's `icom_stop_morse`), Elecraft `KY <EOT>;` (Programmer's
Reference, "KY": EOT "quickly terminates transmission"), FlexRadio `cwx clear`, Kenwood `KY0;` (TS-590
guide, "KY" Set 2; no Kenwood description carries `send-cw`, so it is refused today). The Yaesu CAT guides
have no Morse text command and no stop for one: `Unsupported` there, and unkeying is the stop path. In
`radioctld` it is `\stop_morse`; the splitter's CI-V personality maps `17 FF` to it. Both serve it to any
client, owner or not, gate open or closed.

Unkeying is repeated on a timeout like any non-transmit command (up to the description's `retries`
more times, within the deadline). An unkey is never refused for want of a queue slot: if the emergency
queue is full, the request rides on the unkey already queued and gets its result. It is not a guarantee
that the radio stopped: it is a command, and a radio can ignore a command.

## Late replies

A command that timed out or was abandoned for an emergency may still be answered. The protocols
cannot label the answer: a CI-V `FB`/`FA`, a Kenwood or Elecraft `?;` and an observation look the same
whichever command they answer (FlexRadio numbers its replies, and its engine matches on the number).
So the actor matches by order, which is how every radio answers: it keeps a list of requests written
whose reply has not been seen (for two reply timeouts each), and a reply belongs to the *oldest*
outstanding request it fits (`FB` to a set, a frequency to a frequency read, `?;` to any). A fitting
reply that an abandoned earlier command is still owed is therefore never the waiting command's answer:
it is counted (`RigStats::late_replies`, and `stale_replies` when it would have passed for the answer), an
observation among them still updates the cache as the older reading, and the command keeps waiting for
its own. Before a normal command is written while such a reply is still expected, the actor first reads
input until it arrives or the expectation lapses (`stale_drains`); an emergency command (unkey,
stop-Morse) is written at once and relies on the order rule alone. The case this closes: a key that
times out is followed by the fail-closed unkey, and the key's late `FB` arrived while the unkey waited
for its own; before, the cache said unkeyed on the strength of the wrong `FB`. Tests:
`a_late_ack_for_a_timed_out_key_is_not_taken_for_the_unkeys_answer`,
`a_late_frequency_is_not_taken_for_the_next_reads_answer` and
`a_reply_that_never_comes_stops_being_expected_and_does_not_eat_the_next_answer`.

## The transmit watchdog

Independent of the caller, each rig's actor runs a dead-man timer while the radio transmits. It starts when
the radio is keyed by command, or seen keyed by polling or transceive, or when Morse is sent.

- **Maximum transmit time.** `RigOptions::max_transmit`, 180 seconds by default (`DEFAULT_MAX_TRANSMIT`).
  Past it the actor unkeys the radio and emits `Alarm::TransmitTimeout`. The owner may raise it, and may
  disable it with `Duration::MAX`; that is a decision for the owner, not a default. `radioctld` takes it
  from `max_transmit_s` (1 to 3600 seconds).
- **Caller heartbeat.** With `RigOptions::heartbeat` set to a period, the caller must call
  `RigHandle::heartbeat()` at least that often while the radio transmits, or the actor unkeys it and emits
  `Alarm::HeartbeatLapsed`. Off by default; enforced only while transmitting. A caller that keys and then
  beats is not tripped by the time before it keyed.
- **How it unkeys.** Through the fail-closed path below, preempting a command that is waiting for a
  reply. A radio still seen keyed afterwards re-arms the timer.

This is the library layer. The relay's own time-out and the FX2 board's hardware time-out are outer
layers. `radioctld` and the splitter do not set a heartbeat: for them the maximum transmit time is the
protection.

## When the outcome is unknown: fail closed

Three cases where the radio may be transmitting and nobody asked it to stop, handled in the actor itself,
below every client:

1. **An unanswered key.** A `SetPtt(true)` or `SendCw` that was written and never answered, or whose write
   timed out part way, is followed at once by an unkey, before the `Timeout` is reported. The unkey is not
   preemptible and waits at most the engine's reply timeout. Without this, `radioctld` and the splitter would
   have released their claim on a rig that was in fact keyed, and no later disconnect would have unkeyed it.
2. **The last handle is dropped.** The actor unkeys a rig whose cache says keyed, or whose gate was open,
   before it ends. A holder that returns, panics or is cancelled does not leave RF on the air.
3. **A dropped `LineKeyer`** makes its RTS or DTR line inactive.

Each is written to the transcript as `FAIL CLOSED unkey: <why>`. An unkey is tried three times with a short
backoff. If it still cannot be done (every attempt unanswered or refused, the link down, or no command for
it) the failure is counted (`RigStats::unkey_failures`) and reported as an `Alarm::UnkeyFailed` event: the
radio may still be transmitting. The paths do not cover a process that is killed (below).

## PTT ownership

In the splitter and in `radioctld`, a `PttOwner` (`radioctl-splitter/src/arbiter.rs`) gives the
transmitter to the first client that keys it. Another client's key or Morse request is refused until the
owner unkeys, or disconnects, which unkeys the rig. Morse claims the transmitter the same way a key does,
so a Morse sender that disconnects unkeys the rig too. A claim is made before the rig is asked and given
back if the rig refuses. Stopping `radioctld` always sends each rig an unkey. These behaviours are
tested (`radioctld/tests/arbitration.rs`, `radioctl-splitter/tests/splitter.rs`), including a refused
transmit command leaving no byte on the rig's transport. The library API itself has no owner concept:
two tasks holding handles can both key; ownership is the service's job.

## The transmit safety baseline, item by item

`RELAY_SDR_RADIOCTL_ROADMAP.md` section 2a lists ten requirements that "every relay, every radio, every
operator" must meet in software before transmission is enabled on any radio class. That proposal is
not public. This table says, for each, what radioctl itself has today. Most of the list belongs to the
relay on top of radioctl; the point of the table is to avoid claiming otherwise.

| # | Requirement | In radioctl today |
|---|---|---|
| 1 | Fail closed; a new radio is receive-only | **Implemented**: the gate (handle, actor, `radioctld`). |
| 2 | Per-rig dead-man timer independent of the caller; dead actor, dropped link or crashed process forces unkey where supported | **Implemented in the actor**: the watchdog (maximum transmit time, optional caller heartbeat), the unkey after an unanswered key and when the last handle is dropped, with alarms for failures. `radioctld` and the splitter also unkey for a disconnecting owner and at shutdown. A process that is killed takes its actors with it: nothing unkeys the radio then. |
| 3 | Documented, tested carrier drop after link loss | **Partly.** While a link is down nothing can be written to it: the actor reports `LinkDown` and refuses commands. A serial link that comes back (`RigOptions::link_watch`, chapter [5](05-transports.md)) is unkeyed by the actor before any other command and `Alarm::LinkRestored` reports it; the carrier therefore drops at the reopen, whose time is the supervisor's backoff, not a fixed figure. A CAT radio with no hardware time-out stays keyed until then. |
| 4 | One transmitter per radio, maximum transmit duration, ownership modes | **Partly**: one owner per radio in the splitter and `radioctld`; the maximum transmit duration is the watchdog's. No ownership modes (shared receive, exclusive, transmit-locked). |
| 5 | Frequency, mode, power and privilege limits per operator; identification timers | **Not in radioctl.** It is the relay's job; the gate is the hook it uses. |
| 6 | Drive and signal limits, telemetry-based derating, SWR inhibit | **Not in radioctl.** SWR and power are readable on some radios; nothing acts on them. |
| 7 | Attestation and audit log | **Not in radioctl.** The permit's `granted_by` string is the only record. |
| 8 | No passwords, codes or approval steps in the emergency path | **Consistent**: unkeying has none. |
| 9 | Stop paths for operator and owner; independent hardware stops recommended | **Partly**: any client can unkey and any client can stop Morse (`StopCw`), on every engine that has a command for it. Hardware stops are outside software. |
| 10 | Operating procedure and legal review | Not a code matter. |

An explicit, observable mismatch event between intended and actual transmit state is also in the
proposal. Today the poller reads the PTT state and updates the cache, so a radio that unkeys or keys
itself shows up as a change event with `Source::Poll` or `Transceive` (and keying that the actor did not
ask for arms the watchdog), but nothing compares it with what a caller intended.

## What radioctl does and does not guarantee

It does:

- refuse transmit-capable commands while a rig's gate is closed, at two points, and in `radioctld`
  before anything is queued;
- put unkeying ahead of everything and keep a slow callback or a slow other rig from delaying it;
- unkey a radio that may be keyed when a transmit command goes unanswered, when the last handle is
  dropped, and when it has transmitted past its limit or its caller's heartbeat has lapsed;
- report an unkey that failed, as an alarm, instead of dropping it;
- bound every command by a deadline and answer it with a typed error;
- never repeat a transmit command;
- let only one client at a time key a rig behind the splitter or `radioctld`, and unkey for an owner that
  goes away.

It does **not**:

- stop a transmission when the process that keyed it is killed. `kill -9` takes the actor with it and leaves
  the radio keyed; a CAT radio stays keyed until its own time-out, if it has one. A program that merely
  returns or panics is covered (the last handle drops, above), and one that hangs is covered by the
  watchdog (the maximum transmit time, and the heartbeat if the owner set one).
- limit how long a transmission lasts below the default, or at all if the owner disabled the limit.
- unkey on link loss: nothing can be written to a port that is gone. With `RigOptions::link_watch`
  (chapter [5](05-transports.md)) the application is told (`link_up()`, `Alarm::LinkDown`) and the
  radio is unkeyed as soon as the port is back; without it, unkey commands during a reconnect are
  dropped with the rest of the bytes and end in a timeout.
- guarantee that a description is right. Every description is unverified; a wrong command could key a
  radio or change its state in ways the author did not intend. Read-only trials come first.
- avoid the open-time DTR/RTS glitch, which can key a radio wired to those lines; that is the operating
  system's behaviour (chapter [5](05-transports.md)).
- authenticate or encrypt network clients of `radioctld`; only the address allow list stands between a
  network and a radio with `transmit = true`.
- enforce frequency, power, privilege or identification rules. A caller who opens the gate can send any
  frequency the radio accepts.
- verify that the radio did what it was told; a set that the radio silently ignored (Kenwood-style sets
  have no reply) is cached as if it had worked until polling or a read corrects it.

Treat a radio under radioctl as you would treat one under any CAT program: a dummy load and the owner's
decision first, and independent hardware stops (a time-out, switched power) for anything unattended.
