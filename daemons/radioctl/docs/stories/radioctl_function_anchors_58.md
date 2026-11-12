# radioctl function anchors, batch 58

Function anchors for the Easycomm rotator engine in `radioctl-station` and the virtual rotator's Easycomm side. Transmit safety: nothing here keys a transmitter; an Easycomm controller moves an antenna rotator, and a position outside the description's travel is refused before anything is written.

## `crates/radioctl-station/src/rotator.rs`

### `pub fn new(desc: RotatorDescription) -> EasycommEngine` [@ANCHOR: radioctl:radioctl_station_easycomm_new]

Builds the Easycomm engine for `desc`, taking ownership of it. The description's protocol (`easycomm1`, `easycomm2` or `easycomm3`) picks the version. No I/O, cannot fail; the description is validated by `RotatorDescription::engine`, which is the normal way to get one.

### `fn easycomm_position(buf: &[u8], want_el: bool) -> Result<Option<(RotReply, usize)>, StationError>` [@ANCHOR: radioctl:radioctl_station_easycomm_parse_position]

Reads a position reply from `buf`: words separated by a space, CR or LF, where `AZ<number>` is the azimuth and `EL<number>` the elevation; a word counts only once its separator has arrived. Returns `Ok(None)` while a wanted word is missing or unfinished (`want_el` false needs only the azimuth), otherwise the position and the bytes used up to and including the separator of the later wanted word. Other words (an echo, a version) are skipped; an `AL` word is a `Driver` error carrying the alarm text; an `AZ` or `EL` word with a non-numeric or non-finite value is a `Driver` error. Pure, no I/O, no lock.

### `fn description(&self) -> &RotatorDescription` [@ANCHOR: radioctl:radioctl_station_easycomm_description]

Implementation of `RotatorEngine::description` for the Easycomm engine; takes `&self` and returns `&RotatorDescription`, a borrow of the description the engine was built with (`EasycommEngine::new` took ownership of it, so the borrow lives as long as the engine). The description holds the rotator's `id` and `name`, the wire `protocol` (`easycomm1`, `easycomm2` or `easycomm3`, which `encode` reads to choose the command shape), the travel (`az_min`, `az_max`, `el_min`, `el_max`), `has_elevation`, the optional `park` position, `reply_timeout_ms` (the longest wait for a reply, which the rotator driver reads from this call at its read step) and `verified`. `radioctld`'s rotctl front end also reads the name and travel through it. No I/O, no lock, no side effects, cannot fail or panic. The result never changes after construction, so it is safe to call from any thread (the trait requires `Send + Sync`). Test: `easycomm_encodes_and_parses_the_specification_shapes` in `rotator.rs`.

### `fn encode(&self, op: &RotOp) -> Result<Encoded, StationError>` [@ANCHOR: radioctl:radioctl_station_easycomm_encode]

Encodes one command. `SetPosition` and `Park` become `AZaaa.a ELeee.e` plus a newline (Easycomm I adds Hamlib's `UP000 XXX DN000 XXX` radio fields; an azimuth-only description sends `AZaaa.a` only), one decimal place, no reply expected. `GetPosition` becomes `AZ EL` (or `AZ`) and expects a position; on Easycomm I it is `Refused` because that protocol has no read. `Stop` becomes `SA SE` (or `SA`), no reply. The position is checked by `target` first, so an out-of-travel position or a missing park position is `Refused` and nothing is encoded. Pure, no I/O, no lock.

### `fn parse(&self, expect: RotExpect, buf: &[u8]) -> Result<Option<(RotReply, usize)>, StationError>` [@ANCHOR: radioctl:radioctl_station_easycomm_parse]

Parses an Easycomm reply according to `expect`. `Nothing` is done at once with 0 bytes used (Easycomm acknowledges moves with nothing); `Status` is a `Driver` error (no status replies); `Position` is `easycomm_position`. Pure, no I/O, no lock.

## `crates/radioctl-station/src/virtual_rotator.rs`

### `fn easycomm(&mut self, text: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_station_vrot_easycomm]

The virtual rotator's answer to one Easycomm line (already trimmed): `AZ<n>` and `EL<n>` words set a target together (the missing axis keeps its current target), a bare `AZ` or `EL` asks for the position and the reply is `AZaaa.a ELeee.e` on one line, `SA` and `SE` freeze an axis where it is, other words (`UP`, `DN`, `XXX`) are skipped. A target outside the travel is refused with an `ALout of range` line, as a controller raises an alarm, and nothing moves. Only the virtual rotator's state changes; no I/O, no lock.
