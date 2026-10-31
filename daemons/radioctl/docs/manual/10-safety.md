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
  `unwrap_used`, `expect_used` and `panic` denied); every byte parser uses checked access, because reply
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

## Unkeying and preemption

`Op::SetPtt(false)` is the only emergency command. It goes through a separate channel that the actor
reads first, wakes the actor, and abandons a command that is waiting for a reply (that command ends in
`Cancelled`). It never interrupts a write already in progress. It needs no gate. The tests
`ptt_off_abandons_a_slow_read_on_the_same_rig` and `a_slow_rig_does_not_delay_another_rigs_ptt_off`
cover the same-rig and cross-rig cases; a blocked or panicking application callback cannot delay it
because callbacks run on their own thread.

Unkeying is repeated on a timeout like any non-transmit command (up to the description's `retries`
more times, within the deadline). It is not a guarantee that the radio stopped: it is a command, and a
radio can ignore a command.

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

Each is written to the transcript as `FAIL CLOSED unkey: <why>`. All are best effort: they do nothing if
the link is down or the radio cannot be unkeyed by command, and nobody is told if the unkey itself fails.
They do not cover a process that is killed or hangs (below).

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
| 2 | Per-rig dead-man timer independent of the caller; dead actor, dropped link or crashed process forces unkey where supported | **Partly.** No timer. The actor unkeys after an unanswered key and when its last handle is dropped; `radioctld` and the splitter unkey for a disconnecting owner and at shutdown. A killed or hung process is not covered. |
| 3 | Documented, tested carrier drop after link loss | **Not implemented.** On a failed transport the actor reports `LinkDown` and sends nothing; an unkey cannot be written to a link that is down. |
| 4 | One transmitter per radio, maximum transmit duration, ownership modes | **Partly**: one owner per radio in the splitter and `radioctld`. No maximum duration; no ownership modes (shared receive, exclusive, transmit-locked). |
| 5 | Frequency, mode, power and privilege limits per operator; identification timers | **Not in radioctl.** It is the relay's job; the gate is the hook it uses. |
| 6 | Drive and signal limits, telemetry-based derating, SWR inhibit | **Not in radioctl.** SWR and power are readable on some radios; nothing acts on them. |
| 7 | Attestation and audit log | **Not in radioctl.** The permit's `granted_by` string is the only record. |
| 8 | No passwords, codes or approval steps in the emergency path | **Consistent**: unkeying has none. |
| 9 | Stop paths for operator and owner; independent hardware stops recommended | **Partly**: any client can unkey. Hardware stops are outside software. |
| 10 | Operating procedure and legal review | Not a code matter. |

An explicit, observable mismatch event between intended and actual transmit state is also in the
proposal. Today the poller reads the PTT state and updates the cache, so a radio that unkeys or keys
itself shows up as a change event with `Source::Poll` or `Transceive`, but nothing compares it with what
a caller intended.

## What radioctl does and does not guarantee

It does:

- refuse transmit-capable commands while a rig's gate is closed, at two points, and in `radioctld`
  before anything is queued;
- put unkeying ahead of everything and keep a slow callback or a slow other rig from delaying it;
- unkey a radio that may be keyed when a transmit command goes unanswered or the last handle is dropped;
- bound every command by a deadline and answer it with a typed error;
- never repeat a transmit command;
- let only one client at a time key a rig behind the splitter or `radioctld`, and unkey for an owner that
  goes away.

It does **not**:

- stop a transmission when the program that keyed it is killed or hangs. `kill -9` of a process using the
  library leaves the radio keyed; a CAT radio stays keyed until its own time-out, if it has one. A program that
  merely returns or panics is covered (the last handle drops, above); a hung one holding a handle is not.
  There is no dead-man timer.
- limit how long a transmission lasts.
- unkey on link loss, or tell the application that a serial link is reconnecting (chapter
  [5](05-transports.md)). During a reconnect, unkey commands are dropped with the rest of the bytes and
  end in a timeout.
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
