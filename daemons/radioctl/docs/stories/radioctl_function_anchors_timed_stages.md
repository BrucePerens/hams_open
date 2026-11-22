# radioctl function anchors, timed stages

Function anchors for the timed multi-stage sequence in `radioctl-core` (`ProtocolEngine::encode_stages`): a rig engine says "write A, wait N ms, write B, wait M ms, write C" and the actor carries it out. Each anchor is exercised by `tests/stages.rs`, which cites it with `Tests [@ANCHOR: ...]`. The transmit gate is checked by `run_command` before the first key-on stage (a gate-closed rig gets nothing on the wire); an unkey needs no gate and always runs every stage.

## `crates/radioctl-core/src/engine.rs`

### `Stage` [@ANCHOR: radioctl:radioctl_core_engine_stage]

**Purpose and location.** One stage of a timed byte sequence, defined in `engine.rs` as a pub `struct` (Debug, Clone, PartialEq, Eq).

| Field | Type | Meaning |
|---|---|---|
| `bytes` | `Vec<u8>` | Bytes to write to the transport; no reply is expected. |
| `wait` | `Duration` | How long to wait after writing them, before the next stage. |

### `fn encode_stages(&mut self, op: &Op) -> Result<Option<Vec<Stage>>, RigError>` [@ANCHOR: radioctl:radioctl_core_engine_box_encode_stages]

Forwards the timed-sequence hook to the boxed engine (the trait default returns `Ok(None)`, an ordinary single request). `Ok(Some(stages))` means the actor writes each stage's bytes in order and waits as the stage says; `encode` is not called and no reply is read.

## `crates/radioctl-core/src/rig.rs`

### `async fn write_stages(&mut self, stages: &[Stage], deadline: Instant, keep_going: bool) -> Option<RigError>` [@ANCHOR: radioctl:radioctl_core_actor_write_stages]

Writes the stages in order, sleeping each stage's wait after its write. With `keep_going` false (a key-on or any other op) it stops at the first failed write or when `deadline` has passed (`Timeout`; a failed write is `LinkDown` or `Timeout`). With `keep_going` true (an unkey) it ignores the deadline, gives each write the rig's own timeout and runs every stage whatever happens. Returns the first error, or `None`.

### `async fn run_stages(&mut self, op: &Op, stages: &[Stage], deadline: Instant, source: Source) -> Reply` [@ANCHOR: radioctl:radioctl_core_actor_run_stages]

Runs a timed sequence for `op` through `write_stages`. The transmit gate was already checked by `run_command`. On success the cache and watchdog are updated (`apply_set`). If a transmitting sequence fails part way the relays may have moved, so the actor unkeys fail-closed (`unkey_fail_closed`, which also runs the engine's timed unkey sequence, without needing the gate) and returns the first error.
