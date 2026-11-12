# radioctl function anchors, batch 60

Function anchors for the Hy-Gain DCU rotator engine in `radioctl-station` and the virtual rotator's DCU side. Transmit safety: nothing here keys a transmitter; the engine moves an antenna rotator and refuses a bearing outside the description's travel before anything is written.

## `crates/radioctl-station/src/rotator.rs`

### `pub fn new(desc: RotatorDescription) -> DcuEngine` [@ANCHOR: radioctl:radioctl_station_dcu_new]

Builds the Hy-Gain DCU engine for `desc`, taking ownership of the description (no clone and no borrow is kept by the caller).

| Argument | Type | Meaning |
|---|---|---|
| `desc` | `RotatorDescription` | The rotator's description: protocol `Dcu`, an azimuth-only travel of 0 to 360 degrees, speeds and port settings. Moved in. |

Returns the `DcuEngine`. There is no error path and no I/O; the engine is plain data and is `Send + Sync`, so one engine can serve a task on any thread. It does not validate `desc` itself: `RotatorDescription::engine` calls `validate()` first and then constructs this engine for a description whose protocol is `Dcu` (a DCU description has azimuth only). Constructing it keys nothing and moves no antenna; commands reach a rotator only through `encode`. Test: `dcu_encodes_the_manual_commands_and_reads_nothing` in `rotator.rs`.

### `fn description(&self) -> &RotatorDescription` [@ANCHOR: radioctl:radioctl_station_dcu_description]

Implementation of the trait method `RotatorEngine::description` for the DCU engine.

| Argument | Type | Meaning |
|---|---|---|
| `&self` | `&DcuEngine` | The engine; only borrowed. |

Returns a shared borrow of the `RotatorDescription` the engine was built with, valid as long as the engine lives, so a caller can read the travel limits and protocol without cloning. It cannot fail, does no I/O, takes no lock and has no side effect. The station code uses it to range-check a bearing before encoding and to report the protocol. Test: `dcu_encodes_the_manual_commands_and_reads_nothing`, which asserts the protocol is `Dcu`.

### `fn encode(&self, op: &RotOp) -> Result<Encoded, StationError>` [@ANCHOR: radioctl:radioctl_station_dcu_encode]

Encodes one command. `SetPosition` and `Park` become `AP1nnn;AM1;` in one write (the bearing rounded, three digits, 360 written as 000), no reply expected; `Stop` becomes `AS1;` (Hamlib's DCU stop, not in Hy-Gain's DCU-3 manual); `GetPosition` is `Refused` because a DCU cannot report its bearing. The position is checked by `target` first, so an out-of-travel bearing or a missing park position is `Refused` and nothing is encoded. Pure, no I/O, no lock.

### `fn parse(&self, expect: RotExpect, _buf: &[u8]) -> Result<Option<(RotReply, usize)>, StationError>` [@ANCHOR: radioctl:radioctl_station_dcu_parse]

Parses a DCU reply: `Nothing` is done with 0 bytes used, anything else is a `Driver` error ("a DCU sends nothing back"). The bytes are never looked at. Pure, no I/O.

## `crates/radioctl-station/src/virtual_rotator.rs`

### `fn dcu(&mut self, text: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_station_vrot_dcu]

The virtual rotator's handling of one DCU command (a trailing `;` is removed): `AP1nnn` stores a bearing, `AM1` makes it the target if it is inside the travel, `AS1` freezes the rotator where it is. It never answers, as a DCU-3 sends nothing back. Only the virtual state changes.
