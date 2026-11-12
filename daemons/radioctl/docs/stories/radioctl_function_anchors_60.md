# radioctl function anchors, batch 60

Function anchors for the Hy-Gain DCU rotator engine in `radioctl-station` and the virtual rotator's DCU side. Transmit safety: nothing here keys a transmitter; the engine moves an antenna rotator and refuses a bearing outside the description's travel before anything is written.

## `crates/radioctl-station/src/rotator.rs`

### `pub fn new(desc: RotatorDescription) -> DcuEngine` [@ANCHOR: radioctl:radioctl_station_dcu_new]

Builds the DCU engine for `desc`, taking ownership. No I/O, cannot fail; `RotatorDescription::engine` validates first (a DCU description is azimuth only, 0 to 360).

### `fn description(&self) -> &RotatorDescription` [@ANCHOR: radioctl:radioctl_station_dcu_description]

Implementation of `RotatorEngine::description` for the DCU engine: a borrow of the description. Cannot fail.

### `fn encode(&self, op: &RotOp) -> Result<Encoded, StationError>` [@ANCHOR: radioctl:radioctl_station_dcu_encode]

Encodes one command. `SetPosition` and `Park` become `AP1nnn;AM1;` in one write (the bearing rounded, three digits, 360 written as 000), no reply expected; `Stop` becomes `AS1;` (Hamlib's DCU stop, not in Hy-Gain's DCU-3 manual); `GetPosition` is `Refused` because a DCU cannot report its bearing. The position is checked by `target` first, so an out-of-travel bearing or a missing park position is `Refused` and nothing is encoded. Pure, no I/O, no lock.

### `fn parse(&self, expect: RotExpect, _buf: &[u8]) -> Result<Option<(RotReply, usize)>, StationError>` [@ANCHOR: radioctl:radioctl_station_dcu_parse]

Parses a DCU reply: `Nothing` is done with 0 bytes used, anything else is a `Driver` error ("a DCU sends nothing back"). The bytes are never looked at. Pure, no I/O.

## `crates/radioctl-station/src/virtual_rotator.rs`

### `fn dcu(&mut self, text: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_station_vrot_dcu]

The virtual rotator's handling of one DCU command (a trailing `;` is removed): `AP1nnn` stores a bearing, `AM1` makes it the target if it is inside the travel, `AS1` freezes the rotator where it is. It never answers, as a DCU-3 sends nothing back. Only the virtual state changes.
