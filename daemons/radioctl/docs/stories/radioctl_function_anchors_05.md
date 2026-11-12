# radioctl function anchors, batch 05

Function anchors for the transmit-frequency derivation and its readers (`radioctl-core` `TxState`, `RigHandle`, the splitter and `radioctld` service), the scope frames, streams and CI-V scope encoding, and the unit helpers (hertz, meters, power, per-model level ranges, power-meter calibration). Tests cite each anchor with `Tests [@ANCHOR: ...]`.
Several of these decide which frequency a rig would transmit on. None of them keys a transmitter, but the transmit gate reads `TxState::known_for_transmit`, and a wrong transmit frequency would defeat the band check, so the entries say where a value can be unknown and what then happens.

## `crates/radioctl-core/src/txstate.rs`

### `pub fn active_tx_frequency(&self) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_core_txstate_active_tx_frequency]

Returns the frequency the rig transmits on now, or will when keyed, in hertz: the frequency of the transmit VFO as derived by `tx_vfo`. It is correct in split operation and while keyed, and it is what an amplifier, tuner, antenna selector or band decoder follows; the actor publishes it as `Observation::ActiveTxFrequency` after every observation. It returns `None` until that VFO's frequency has been read. Caution: when the split state has not been read it assumes "not split", so for a rig that can split it can name the wrong VFO. Anything that decides whether to key must use `known_for_transmit`, which refuses that case. Pure, takes `&self`, no side effects.

### `pub fn known_for_transmit(&self, split_capable: bool) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_core_txstate_known_for_transmit]

The gate input of every transmit-capable command: the transmit frequency when it is known well enough to key, else `None` (fail closed). `split_capable` says whether the rig has split operation (its description lists the capability). When it is true and the split state has never been read, the result is `None`, because the transmit VFO would be a guess; the derivation alone would assume not split. When it is false, the rig transmits on the receive VFO and only that VFO's frequency is needed. Otherwise it equals `active_tx_frequency`. A `None` result means the caller must refuse to key, not that the frequency is zero. Pure; the lock, band check and transmit gate then compare the returned frequency with their limits.

### `fn frequency(&self, vfo: Vfo) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_core_txstate_frequency]

Looks up the cached frequency of one VFO in the state: `Vfo::A` gives the stored VFO A frequency, `Vfo::B` the stored VFO B frequency, each `None` when it has not been read. `vfo` is passed by value (it is `Copy`). Private helper of `rx_frequency` and `tx_frequency`; it does no derivation and no I/O. Cannot fail. The frequency is in whole hertz; a RIT or XIT offset is not included.

## `crates/radioctl-core/src/rig.rs`

### `pub async fn split(&self, freshness: Freshness) -> Result<Cached<bool>, RigError>` [@ANCHOR: radioctl:radioctl_core_handle_split]

Asks whether the rig is in split operation (the transmit VFO differs from the receive VFO). `freshness` says how old a cached answer may be: `Cached` accepts any cached value and asks the rig only if there is none, `MaxAge(d)` accepts one no older than `d`, `Fresh` always asks the rig. The result carries the value and its age (zero after a fresh read). Errors: whatever the actor reports for the read (`Unsupported` when the rig cannot report split, `Timeout`, `LinkDown`, and so on) and `Malformed` if the answer is not a split observation or the read returned no value. Async; a read goes through the actor's command queue and may wait behind other commands. It changes nothing on the rig.

### `async fn tx_state(&self, freshness: Freshness) -> Result<TxState, RigError>` [@ANCHOR: radioctl:radioctl_core_handle_tx_state]

Brings the inputs of the transmit-frequency derivation up to date and returns them as a `TxState`. If the rig has the split capability it reads the split state and, when it also has a second VFO, VFO B's frequency; it always reads VFO A's frequency. Each read obeys `freshness`: with `Fresh` it asks the rig for all of them in turn (up to three commands, one after another); otherwise it asks only for what the cache lacks or has too old. Any error from a read (`Timeout`, `LinkDown`, `Unsupported` and so on) aborts and is returned. The result is built from the cache with `TxState::from_cache`, so reported receive and transmit VFOs are included if known. Private; called by the four public readers below. Async, no side effects on the rig.

### `pub async fn tx_vfo(&self, freshness: Freshness) -> Result<Vfo, RigError>` [@ANCHOR: radioctl:radioctl_core_handle_tx_vfo]

Returns the VFO the rig transmits on. It refreshes the derivation inputs with `tx_state(freshness)` and applies `TxState::tx_vfo`: the receive VFO when split is off, the rig's own transmit-VFO report where its protocol gives one, otherwise the other VFO in split operation (VFO B, the unselected VFO on CI-V, on most rigs). `freshness` is as for `split`. Errors are those of the reads inside `tx_state`; the derivation itself cannot fail because it always has a default (VFO A). Async. Do not use this alone to decide whether to key: it does not say whether the split state was actually read.

### `pub async fn rx_frequency(&self, freshness: Freshness) -> Result<Hz, RigError>` [@ANCHOR: radioctl:radioctl_core_handle_rx_frequency]

Returns the frequency the rig receives on, in hertz: VFO A, or the receive VFO the rig reports (Kenwood). It does not depend on which VFO the caller names. It refreshes the derivation inputs with `tx_state(freshness)` (`Cached`, `MaxAge` or `Fresh` as in `split`) and takes `TxState::rx_frequency`. Errors: those of the reads, and `ErrorKind::Malformed` ("receive frequency unknown") if the frequency of the receive VFO is still not in the cache. The value is the VFO frequency; RIT is not added. Async.

### `pub async fn tx_frequency(&self, freshness: Freshness) -> Result<Hz, RigError>` [@ANCHOR: radioctl:radioctl_core_handle_tx_frequency]

Returns the frequency of the transmit VFO in hertz, which equals the receive frequency when the rig is not split. Unlike `RigHandle::frequency`, it never depends on which VFO the caller names, which is the point of Hamlib issue 138. `freshness` is as for `split`; it refreshes through `tx_state`. Errors: the read errors, and `Malformed` ("transmit frequency unknown") if the transmit VFO's frequency is not known. XIT is not folded in. Async, read only. It is not the transmit gate: it assumes "not split" if the split state is unread, and the gate uses `known_for_transmit`.

### `pub async fn active_tx_frequency(&self, freshness: Freshness) -> Result<Hz, RigError>` [@ANCHOR: radioctl:radioctl_core_handle_active_tx_frequency]

Returns the frequency the rig transmits on now, or will when keyed, in hertz, correct in split operation and while keyed. Amplifiers, tuners, antenna selectors and band decoders follow it. `freshness` is as for `split`. It refreshes the inputs with `tx_state` and returns `TxState::active_tx_frequency`; the status-change stream separately reports `Field::ActiveTxFrequency` whenever it changes, including a change made on the front panel. Errors: the read errors and `Malformed` ("transmit frequency unknown"). Async, no side effects on the rig. Not a gate: a rig with split whose split state is unread is answered as not split.

### `pub fn scope_frames(&self) -> ScopeStream` [@ANCHOR: radioctl:radioctl_core_handle_scope_frames]

Subscribes to the rig's spectrum scope sweeps and returns an async stream that starts now (sweeps published earlier are not replayed). Scope frames have a broadcast channel of their own, separate from the status-change events, so a slow subscriber cannot hold back status changes; one that falls behind skips frames and the miss is added to the `scope_lagged` counter. The rig sends sweeps only after `Op::SetScopeOutput(true)` has been run. Synchronous and cheap; it can be called many times, each stream receiving every frame. The stream holds a weak reference to the actor's shared state, so it does not keep the rig alive. No arguments, cannot fail.

### `pub async fn next(&mut self) -> Option<ScopeFrame>` [@ANCHOR: radioctl:radioctl_core_scope_stream_next]

Waits for the next complete scope sweep and returns it as a `ScopeFrame`. `&mut self` is needed because the stream tracks its position. It returns `None` once the rig has stopped and the channel is closed. If the subscriber fell behind and the channel dropped frames, the number missed is added to the rig's `scope_lagged` counter and the call goes on waiting for the next frame, so a lag never ends the stream and never returns an error. Cancel safe: dropping the future loses nothing already queued. Async; frames carry sequence numbers, so a gap is also visible to the caller.

### `pub fn filter_scope_frames(&self) -> FilterScopeStream` [@ANCHOR: radioctl:radioctl_core_handle_filter_scope_frames]

Subscribes to the rig's filter-scope sweeps (the passband display, a few hundred amplitude pixels with no frequency axis) and returns an async stream starting now. It is separate from the band scope's stream and has its own lag count, `filter_scope_lagged`. A rig that has no filter scope never sends any, so the stream then simply never yields. Synchronous and cheap, no arguments, cannot fail; any number of subscribers may exist. The stream holds only a weak reference to the actor's shared state.

### `pub async fn next(&mut self) -> Option<FilterScopeFrame>` [@ANCHOR: radioctl:radioctl_core_filter_scope_stream_next]

Waits for the next filter-scope sweep and returns it as a `FilterScopeFrame`. Returns `None` when the rig has stopped. Frames missed while the subscriber lagged are skipped and counted in the rig's `filter_scope_lagged`; the call then continues to wait, so it never fails. `&mut self` because the stream keeps its place in the channel. Cancel safe. Async. It does not touch the rig; it only reads what the actor has published.

## `crates/radioctl-splitter/src/session.rs`

### `fn keyed_split_frequency(&self, vfo: Vfo) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_splitter_keyed_split_frequency]

Decides whether a plain read of the current frequency, from a client of the splitter, should be answered with the transmit frequency instead. It returns `Some(hz)` only when all of these hold: `vfo` is VFO A (the current VFO of both personalities); the splitter configuration has `report_tx_frequency_while_keyed` switched on; and the cache shows PTT on, split on and an active transmit frequency. In every other case it returns `None`, meaning "answer as usual". It reads only cached observations, with no I/O and no locks beyond the cache's; it cannot fail. It never keys or changes anything; it affects only what a client is told while the rig is already transmitting in split, which is useful to amplifier and logging software that read "the frequency".

## `crates/radioctld/src/service.rs`

### `fn tx_state(&self) -> TxState` [@ANCHOR: radioctl:radioctld_service_tx_state]

Builds the transmit-state derivation from what the rig last reported, without asking the rig: it calls `TxState::from_cache` over the rig's cache, so values never read are `None`. The daemon's service uses it to resolve the receive and transmit VFO selections (`VfoSel::Rx` and `VfoSel::Tx`) and to answer the active transmit frequency request (`\get_active_tx_freq`), while `f` keeps its named-VFO meaning. Synchronous, takes `&self`, no side effects, cannot fail. Because it is cache only, it may be stale by up to the poll interval; callers wanting a fresh value must read through the rig handle.

## `crates/radioctl-protocols/src/kenwood.rs`

### `fn vfo_of(digit: &str) -> Vfo` [@ANCHOR: radioctl:radioctl_kenwood_vfo_of]

Maps the digit of a Kenwood `FR` (receive VFO) or `FT` (transmit VFO) answer to a VFO: the string `"1"` is VFO B and every other string, including `"0"`, is VFO A. `digit` is borrowed and compared as a whole string. There is no error: an unexpected value silently becomes VFO A, which is why callers parse the digit from a fixed position of a well-formed frame first. Pure. The result feeds the reported receive and transmit VFO observations that `TxState` uses, so a mistaken digit would show the wrong transmit VFO.

## `crates/radioctl-protocols/src/civ.rs`

### `fn encode_scope(&self, op: &Op) -> Option<Result<Request, RigError>>` [@ANCHOR: radioctl:radioctl_civ_engine_encode_scope]

Encodes the scope operations of the core API as CI-V command 0x27. `op` is borrowed. It returns `None` when the operation is not a scope operation, so the caller goes on to the other encoders. Handled: set and get scope output, scope axis (mode) per receiver, span per receiver, fixed-mode edges, and reference level in hundredths of a decibel. A set expects an `Ack`; a get expects the matching observation. A span the model's menu does not offer (the description lists `spans_hz`) is refused here with `Unsupported` ("span not offered by this scope") instead of being sent for the rig to refuse; an empty list means any. Other errors come from `scope_request_for`: no scope description, or a value that does not fit its field. Pure, no I/O. Scope commands do not transmit.

### `fn scope_request_for(&self, command: ScopeCommand, reply: ReplyKind) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_civ_engine_scope_request_for]

Builds the CI-V request frame for one scope command with the reply the caller will wait for. `command` is the typed `ScopeCommand` (owned, `Copy`); `reply` is `ReplyKind::Ack` for a set, an observation kind for a get, or `None` for a query whose answer is not waited for. Errors, all `Unsupported`: the model has no scope description ("this model has no scope description"); the command's value is out of range for its field ("scope value out of range"); or the command data is empty. Otherwise the sub-command and its data are placed in command 0x27 through the engine's `command` encoder, which adds the addresses and framing. Pure; the frame is returned, not sent. Used by `scope_request` and `encode_scope`.

## `crates/radioctl-core/src/scope.rs`

### `pub fn span(&self) -> Hz` [@ANCHOR: radioctl:radioctl_core_scope_frame_span]

Returns the width of a scope sweep's axis in hertz: the highest frequency minus the lowest (`high - low`), saturating at 0 if a malformed frame has `high` below `low`. The result is derived from the frame's own `low` and `high` fields and does not allocate. Pure; `&self`; no errors.

### `pub fn centre(&self) -> Hz` [@ANCHOR: radioctl:radioctl_core_scope_frame_centre]

Returns the centre of a scope sweep's axis in hertz: the lowest frequency plus half the span (`low + span / 2`, integer division, so a odd span loses half a hertz). Derived from `low` and `high`; pure, `&self`, no allocation. It is the frequency a front end would mark in the middle of the displayed band. If a frame has `high` below `low` the span is 0 and the centre is `low`; the sum is not checked for overflow, which cannot occur with real radio frequencies.

### `pub fn peak(&self) -> u8` [@ANCHOR: radioctl:radioctl_core_filter_scope_frame_peak]

Returns the largest amplitude sample in a filter-scope sweep, or 0 for a sweep with no samples. The samples are one byte per pixel, lowest audio frequency first, in the rig's own amplitude units (`amplitude_max` gives the ceiling). Pure; `&self`; no errors; the scan is linear in the number of pixels, a few hundred. It is a convenience for callers that want the strongest pixel of a sweep.

## `crates/radioctl-protocols/src/io.rs`

### `pub fn scope_sweep(&self)` [@ANCHOR: radioctl:radioctl_virtual_io_scope_sweep]

Makes a virtual rig send one scope sweep of its Main receiver on its own, as a rig with waveform output switched on does. It takes the virtual rig's lock, asks the model for the sweep bytes, and if there are any puts them on the outbox with the current time, to be delivered after the configured reply delay. If the model has no scope, or scope output is off, nothing is sent. Returns nothing and cannot fail; a closed outbox is ignored. For tests of the scope stream. It touches no hardware. The lock is held only while the sweep is built.

## `crates/radioctl-protocols/src/civ_scope.rs`

### `fn from(mode: ScopeMode) -> ScopeAxis` [@ANCHOR: radioctl:radioctl_scope_axis_from_mode]

Converts the CI-V scope mode (the wire vocabulary: centre, fixed, scroll-centre, scroll-fixed, codes 0 to 3) into the core API's `ScopeAxis`, the one-to-one same four values. It is the `From` implementation used when a CI-V reply is turned into the axis observation. `mode` is by value. Total and pure: every mode has an axis, so it cannot fail. It is the inverse of the conversion from `ScopeAxis`.

### `fn from(axis: ScopeAxis) -> ScopeMode` [@ANCHOR: radioctl:radioctl_scope_mode_from_axis]

Converts the core API's `ScopeAxis` into the CI-V `ScopeMode`, the four values map one to one (centre, fixed, scroll-centre, scroll-fixed). It is the `From` implementation used by `encode_scope` when a `SetScopeAxis` operation is turned into the wire code (0 to 3, through `ScopeMode::code`). `axis` is by value. Total and pure; it cannot fail, and converting back gives the original axis.

## `crates/radioctl-core/src/units.rs`

### `pub fn from_khz(khz: u64) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_core_hz_from_khz]

Builds a frequency in hertz from a whole number of kilohertz. `khz` is a `u64`; the result is `khz * 1000`. It returns `None` if the multiplication overflows `u64` (above about 1.8e16 kHz, far beyond any radio), so callers parsing a kilohertz figure from a file or a protocol can refuse bad input instead of wrapping. Pure, no allocation, no other errors. Frequencies are integer hertz throughout radioctl and never floating point, so there is no rounding.

### `pub fn name(self) -> &'static str` [@ANCHOR: radioctl:radioctl_core_meter_name]

Returns the file-format name of a raw meter: `swr`, `alc`, `comp`, `vd` or `id`. `self` is the `Meter` by value. The names are those used in transcript files, rig description files and consoles, and `from_name` is the inverse. The returned string is static and not owned. The fallback "unknown" in the code cannot be reached while every meter is in the table. Pure and cannot fail.

### `pub fn from_name(name: &str) -> Option<Meter>` [@ANCHOR: radioctl:radioctl_core_meter_from_name]

Finds the raw meter with a file-format name. `name` is borrowed and compared exactly, case sensitive, against `swr`, `alc`, `comp`, `vd` and `id`. Returns `Some(meter)` for a match and `None` for any other string, including an empty one or the wrong case; the caller decides how to report an unknown meter (a parse error for a transcript or description file). The inverse of `Meter::name`. Pure, no allocation.

## `crates/radioctl-core/src/description.rs`

### `pub fn cal(&self) -> Option<crate::power::PowerCal>` [@ANCHOR: radioctl:radioctl_core_powermeter_cal]

Builds the power calibration of a rig's raw power meter from the description's points. The points are `(raw reading, milliwatts)`, ascending in both; each is converted to `(raw as f64, watts)`. It returns `Some` only when `PowerCal::new` accepts them: at least two points, strictly ascending raw values, watts not decreasing and not negative, all finite. Otherwise `None`, and radioctl then reports a relative value (a bar of 0 to 100) rather than watts. `&self`; the points are copied, not consumed. Pure, no errors. A rig that reports only a relative bar has no points.

### `pub fn level_range(&self, level: Level) -> Option<(u16, u16)>` [@ANCHOR: radioctl:radioctl_core_elecraft_level_range]

Looks up the per-model wire range of an Elecraft level in the description's `[elecraft] levels`. `level` is the level to look up (by value). Returns `Some((min, max))`, the smallest and largest value on the wire for the first entry that names the level, or `None` when the description gives none, in which case the K3 range of the engine applies. For example the KX3's AF gain is 0 to 60. `&self`; pure; no errors. How the engine applies the range to a level value is in the Elecraft engine (unknown here; check its level wire code).

### `pub fn level_range(&self, level: Level) -> Option<(u16, u16)>` [@ANCHOR: radioctl:radioctl_core_yaesu_level_range]

Looks up the per-model wire range of a Yaesu newcat level in the description's `[yaesu] levels`. `level` is by value. Returns `Some((min, max))` for the first entry that names the level, or `None` when the description gives none, in which case the engine's default range applies (the FT-891's `RG` is 0 to 30 and the FTDX5000's `MG` and `PC` are 0 to 255, as examples of overrides). `&self`; pure; no errors. How the engine applies the range is in the Yaesu newcat engine (unknown here; check its level wire code); the figures must come from the model's CAT manual.

## `crates/radioctl-core/src/power.rs`

### `pub fn from_watts(watts: f64) -> Option<Power>` [@ANCHOR: radioctl:radioctl_power_from_watts]

Makes a `Power` from a figure in watts, the canonical unit. `watts` is a `f64`. It returns `None` for a negative number, an infinity or not-a-number, so a `Power` is always finite and not negative; zero is allowed. There is no rounding or clamping. Every other conversion (`from_dbm`, `from_dbw`, `from_dbmv_50ohm`) goes through this check, so the invariant holds for all sources. Pure, `Copy` result, no other errors.

### `pub fn from_dbm(dbm: f64) -> Option<Power>` [@ANCHOR: radioctl:radioctl_power_from_dbm]

Makes a `Power` from decibels relative to one milliwatt: `P = 10^((dBm - 30) / 10)` watts, so 30 dBm is 1 W and 0 dBm is 1 mW. `dbm` is a `f64`, any sign. Returns `None` when the result is not finite and non-negative, which happens for NaN or an infinite or enormous input that overflows. Negative dBm values are valid and small. Pure, no other errors.

### `pub fn from_dbw(dbw: f64) -> Option<Power>` [@ANCHOR: radioctl:radioctl_power_from_dbw]

Makes a `Power` from decibels relative to one watt: `P = 10^(dBW / 10)` watts, so 0 dBW is 1 W and 30 dBW is 1 kW. `dbw` is a `f64`. Returns `None` if the result is not finite and not negative (NaN, or an input large enough to overflow). Pure, no other errors.

### `pub fn from_dbmv_50ohm(dbmv: f64) -> Option<Power>` [@ANCHOR: radioctl:radioctl_power_from_dbmv_50]

Makes a `Power` from decibels relative to one millivolt RMS across 50 ohms: the voltage is `V = 10^(dBmV / 20)` millivolts and the power `P = V^2 / 50` watts, about -46.99 dBm at 0 dBmV. `dbmv` is a `f64`. Returns `None` for NaN or an overflowing input. The 50 ohm load is fixed; a meter reading across another impedance needs a different conversion (unknown, not provided). Pure, no other errors.

### `pub fn watts(self) -> f64` [@ANCHOR: radioctl:radioctl_power_watts]

Returns the power as a number of watts, the canonical unit in which every source is stored. `self` is the `Power` by value (it is `Copy`). The value is finite and not negative by construction. Pure, no allocation, cannot fail. Other units are derived from it for display (`dbm`), and the display formatter chooses mW, W or kW.
