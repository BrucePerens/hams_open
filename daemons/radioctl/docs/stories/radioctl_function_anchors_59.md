# radioctl function anchors, batch 59

Function anchors for the SPID Rot2Prog rotator engine in `radioctl-station` and the virtual rotator's Rot2Prog side. Transmit safety: nothing here keys a transmitter; the engine moves an antenna rotator and refuses a position outside the description's travel before anything is written.

## `crates/radioctl-station/src/rotator.rs`

### `pub fn new(desc: RotatorDescription) -> Rot2ProgEngine` [@ANCHOR: radioctl:radioctl_station_rot2prog_new]

Builds the Rot2Prog engine for `desc`, taking ownership. No I/O, cannot fail; `RotatorDescription::engine` validates first (Rot2Prog needs `pulses_per_degree` 1 to 10 and four digits of 360 plus the largest angle).

### `fn r2p_frame(command: u8, angles: Option<(f32, f32, u8)>) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_station_rot2prog_frame]

Builds one 13-byte command frame: `0x57`, ten payload bytes, the command byte, `0x20`. With `angles` (azimuth, elevation, pulses per degree) the payload holds, for each axis, four ASCII digits of `(360 + angle) * pulses` rounded and zero padded, then the pulses byte; without it the payload is zeros. Pure, no I/O.

### `fn r2p_angle(digits: &[u8], ppd: u8) -> Result<f32, StationError>` [@ANCHOR: radioctl:radioctl_station_rot2prog_reading]

Turns the four digit bytes and the pulses-per-degree byte of a reply into degrees: the digits (each ASCII `0x30` to `0x39` or a plain 0 to 9) make a number, divided by the pulses and minus 360. A zero pulses byte or a byte that is not a digit is a `Driver` error. Pure, no I/O.

### `fn description(&self) -> &RotatorDescription` [@ANCHOR: radioctl:radioctl_station_rot2prog_description]

Implementation of `RotatorEngine::description` for the SPID Rot2Prog engine; takes `&self` and returns `&RotatorDescription`, a borrow of the description `Rot2ProgEngine::new` took ownership of. The fields that matter to this protocol are `az_min` to `el_max` (the travel that `encode` checks every position against before a frame exists), `has_elevation` (decides whether the elevation is parsed from a reply), `pulses_per_degree` (1 to 10, validated by `RotatorDescription::engine`; it goes into every set frame and is the divisor for reading the reply angle), `park`, and `reply_timeout_ms` (read by the rotator driver as the longest wait for the 12-byte reply). No I/O, no lock, no side effects, cannot fail. Immutable after construction, so any thread may call it (the trait is `Send + Sync`). Test: `rot2prog_encodes_and_parses_the_sheet_frames` in `rotator.rs`.

### `fn encode(&self, op: &RotOp) -> Result<Encoded, StationError>` [@ANCHOR: radioctl:radioctl_station_rot2prog_encode]

Encodes one command. `SetPosition` and `Park` are a set frame (command `0x2F`, no reply expected), `GetPosition` a status frame (`0x1F`, expects a position) and `Stop` a stop frame (`0x0F`; the controller answers with its position, which is ignored). The position is checked by `target` first, so an out-of-travel position or a missing park position is `Refused` and nothing is encoded. Pure, no I/O, no lock.

### `fn parse(&self, expect: RotExpect, buf: &[u8]) -> Result<Option<(RotReply, usize)>, StationError>` [@ANCHOR: radioctl:radioctl_station_rot2prog_parse]

Parses a Rot2Prog reply. `Nothing` is done with 0 bytes used; `Status` is a `Driver` error. `Position` skips bytes before the first `0x57`, waits for 12 bytes (`Ok(None)` until then), requires the last to be `0x20`, and reads the azimuth and (when the description has elevation) the elevation with `r2p_angle`, returning the position and the bytes used. Pure, no I/O, no lock.

## `crates/radioctl-station/src/virtual_rotator.rs`

### `fn rot2prog_frames(&mut self) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_station_vrot_rot2prog]

Serves every complete 13-byte Rot2Prog frame in the virtual rotator's buffer, dropping bytes before a `0x57`. A set frame (`0x2F`) moves the target if it is inside the travel and answers nothing; a status (`0x1F`) or stop (`0x0F`) frame answers the 12-byte position reply, stop first freezing the rotator where it is. Reply digit bytes are plain 0 to 9 when `raw_digits` is true (the default, as Hamlib reads a controller) and ASCII otherwise; the pulses byte is the description's. Nothing is answered when the rotator is silent. Only the virtual state changes.
