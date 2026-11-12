# radioctl function anchors, batch 52

Function anchors for the `radioctl-station` crate in two files: `power_meter.rs`, which polls an external wattmeter (the Elecraft KPA500 and KPA1500 amplifier meters) over a byte stream, turns its replies into a `Sample`, keeps the latest one in a `PowerFeed` that ages out, and offers a virtual meter for tests; and `virtual_rotator.rs`, a sans-IO virtual rotator that speaks the rotctld and GS-232 protocols with a slewing model, plus the TCP server and control handle around it.

Transmit safety in this batch: nothing here keys a transmitter or switches station hardware. The meter code only writes read requests (`^WS;`, `^PWF;`, `^PWR;`) and parses replies; its output is a reading that other code may use, and a stale or missing reading yields no value rather than a stale one. The virtual rotator and virtual meter are test doubles that never touch real hardware. No interlock, transmit gate or channel lock applies in these files; the only locks are the poller's async mutex that serialises one serial exchange, the feed's short std mutex, and the virtual rotator's mutex.

## `crates/radioctl-station/src/power_meter.rs`

### `pub fn name(self) -> &'static str` [@ANCHOR: radioctl:radioctl_station_meter_driver_name]

Method of `MeterDriver`: the name a station configuration uses for this meter driver, `"kpa500"` for `Kpa500` and `"kpa1500"` for `Kpa1500`. The argument is the `Copy` enum by value. Returns a static string, never allocates, and cannot fail or panic. No locks, no I/O. It is the inverse of `from_name`.

### `pub fn from_name(name: &str) -> Option<MeterDriver>` [@ANCHOR: radioctl:radioctl_station_meter_driver_from_name]

Looks up a meter driver by its configuration name. `name` is a borrowed string compared exactly (case-sensitive) with each driver's `name()`. Returns `Some(driver)` for `kpa500` and `kpa1500`, otherwise `None`; the caller turns `None` into its own configuration error. Cannot panic, pure, no I/O, no locks.

### `pub fn unit(self) -> PowerUnit` [@ANCHOR: radioctl:radioctl_station_meter_driver_unit]

Returns the unit in which this driver reports power. Both supported drivers read watts, so the result is always `PowerUnit::Watts`. The argument is the enum by value. It is used when building a `PowerFeed`, so the feed converts the raw numbers with the right unit. Pure, cannot fail, no locks, no I/O.

### `pub fn default_baud(self) -> u32` [@ANCHOR: radioctl:radioctl_station_meter_driver_default_baud]

Returns the serial speed to use when the configuration gives none: 38400 for both drivers, in bits per second. The source comment says the figure is the one the KPA500 document gives, and that the KPA1500 document names no default so 38400 there is unverified. The argument is the enum by value. Pure, cannot fail, no I/O, no locks.

### `pub fn requests(self) -> &'static [&'static [u8]]` [@ANCHOR: radioctl:radioctl_station_meter_driver_requests]

Returns the byte strings one poll sends, in order, each sent only after the previous reply has been read. The KPA500 sends the single request `^WS;` (power and SWR); the KPA1500 sends `^PWF;` (forward power) then `^PWR;` (reflected power). The slices are static and borrowed; the argument is the enum by value. Cannot fail, no I/O, no locks. `MeterPoller::poll` iterates it.

### `pub fn has_reflected(self) -> bool` [@ANCHOR: radioctl:radioctl_station_meter_driver_has_reflected]

Says whether this driver's poll yields a reflected-power figure: true for the KPA1500, which has the separate `^PWR;` request, and false for the KPA500, whose single reply carries forward power and an SWR figure that the parser discards. The argument is the enum by value. Pure, cannot fail, no I/O, no locks.

### `pub fn parse_kpa500(frame: &str) -> Result<Sample, StationError>` [@ANCHOR: radioctl:radioctl_station_meter_parse_kpa500]

Parses one KPA500 reply of the form `^WSppp sss;`, where the first number is the output power in watts and the second the SWR. `frame` is the borrowed complete frame including the leading `^WS` and trailing `;`. Returns a `Sample` with `forward` in watts and `reflected` set to `None`. The SWR field must still be all digits but its value is discarded. Errors: a `Driver` error (the private `proto` helper) when the prefix or the `;` is missing, there is no space separator, or either field is empty or contains a non-digit. Pure, no I/O, no locks.

### `pub fn parse_kpa1500(frame: &str) -> Result<(char, f64), StationError>` [@ANCHOR: radioctl:radioctl_station_meter_parse_kpa1500]

Parses one KPA1500 reply, `^PWFnnnn;` for forward or `^PWRnnnn;` for reflected power. `frame` is the borrowed whole frame. Returns the tag character (`'F'` or `'R'`) and the watts as `f64`. Errors, all of kind `Driver`: the `^PW` prefix or trailing `;` is missing, the tag is neither `F` nor `R`, or the number is empty or has a non-digit character (no sign, no decimal point). Pure, no I/O, no locks. `MeterPoller::poll` assigns the value to forward or reflected by the tag.

### `pub fn new(driver: MeterDriver, io: T, reply_timeout: Duration) -> MeterPoller<T>` [@ANCHOR: radioctl:radioctl_station_meter_poller_new]

Builds a poller for `driver` over the byte stream `io` (an async read and write stream, usually a serial port). `io` is taken by value and wrapped in a tokio `Mutex` so one exchange at a time uses the stream. `reply_timeout` bounds the wait for each single byte of a reply. Nothing is sent until `poll` is called. Cannot fail, no I/O, no panic.

### `async fn read_frame(&self, io: &mut T) -> Result<String, StationError>` [@ANCHOR: radioctl:radioctl_station_meter_poller_read_frame]

Private method: reads from the already locked stream `io` (mutably borrowed) one byte at a time until a `;` ends the frame, and returns the frame as text. Bytes before the first `^` are dropped, which discards the echo of a wake-up `;` and line noise. Each byte read is bounded by `reply_timeout`. Errors: `Timeout` ("no reply") when a byte does not arrive in time; `Driver` when the port reports end of stream, a read error occurs, the frame grows beyond 32 bytes without a `;`, or the bytes are not valid UTF-8. Async, performs serial reads, takes no lock itself.

### `pub async fn poll(&self) -> Result<Sample, StationError>` [@ANCHOR: radioctl:radioctl_station_meter_poller_poll]

Asks the meter once and returns the resulting `Sample`. It locks the stream for the whole exchange (a serial bus is half duplex, as the source comments say), then for each request of the driver writes it, reads one frame and parses it: the KPA500 reply becomes the sample, the KPA1500 replies fill forward and reflected in turn. Errors: a `Driver` error if a write fails or a reply is malformed, and a `Timeout` if the meter does not answer. On any error the caller has no new reading and the old one ages out. Async, writes to and reads from the serial port, holds the stream mutex across those awaits on purpose; each call is independent, so calling again is safe.

### `pub fn new(name: &str, unit: PowerUnit, offset_db: f64, stale_after: Duration) -> PowerFeed` [@ANCHOR: radioctl:radioctl_station_feed_new]

Creates an empty feed for one named external meter. `name` is copied into the feed and appears in the reading's origin as `PowerOrigin::External(name)`. `unit` is the unit the meter reports. `offset_db` is the coupler and attenuator loss in decibels between transmitter and meter, applied on every read. `stale_after` is how long a pushed sample remains valid. It uses the real clock (`Instant::now`). Cannot fail, no I/O, no locks held after return. The feed has no reading until the first `push`.

### `pub fn with_clock(name: &str, unit: PowerUnit, offset_db: f64, stale_after: Duration, clock: Clock) -> PowerFeed` [@ANCHOR: radioctl:radioctl_station_feed_with_clock]

Same as `PowerFeed::new` but with the time source supplied as `clock`, a shared function returning an `Instant`, so a test can control freshness without sleeping. The other arguments are as in `new`: `name` borrowed and copied, `unit`, `offset_db` in decibels, `stale_after` as a duration. The clock is read by `push` and `read`. Cannot fail; no I/O; the state starts empty under a std mutex.

### `pub fn push(&self, sample: Sample)` [@ANCHOR: radioctl:radioctl_station_feed_push]

Records `sample` as the latest reading, stamped with the feed's clock at the moment of the call, replacing any earlier one. `sample` is taken by value. Takes the feed's std mutex briefly; a poisoned mutex is recovered rather than panicking, so it cannot fail. No I/O, not async. Idempotent in effect: the last push wins. `run_poller` calls it after each successful poll.

### `pub fn name(&self) -> &str` [@ANCHOR: radioctl:radioctl_station_feed_name]

Returns the meter's configured name as a borrowed string slice tied to the feed. No other arguments. Takes no lock, performs no I/O, cannot fail or panic. The same name is used as the origin of the readings the feed produces.

### `fn read(&self) -> Option<PowerReading>` [@ANCHOR: radioctl:radioctl_station_feed_read]

Implementation of `RfPowerReader::read` for `PowerFeed`: the current reading or `None`. Returns `None` when nothing was ever pushed, when the sample is older than `stale_after`, or when the forward figure cannot be converted to power (the unit's `to_power` rejects it, for example a negative or non-finite value). The reflected value is converted the same way and is `None` if absent or unconvertible. Otherwise returns a `PowerReading` with origin `External(name)`, `valid_for` equal to the remaining freshness, `relative` false and no scale. Takes the std mutex briefly (poison recovered), reads the clock, no I/O; it never returns a stale figure.

### `pub async fn run_poller<T: AsyncRead + AsyncWrite + Unpin + Send>(poller: MeterPoller<T>, feed: Arc<PowerFeed>, interval: Duration)` [@ANCHOR: radioctl:radioctl_station_meter_run]

Task body that polls forever: every `interval` it calls `poller.poll()` and pushes a successful sample into `feed`. `poller` is owned, `feed` is a shared handle, `interval` is the period; missed ticks are delayed rather than bursting. A failed poll (timeout or malformed reply) is ignored and the feed is left alone, so its last reading ages out after `stale_after`. It never returns and has no error value; it ends only when the task is dropped or aborted. Async, performs serial I/O through the poller.

### `pub fn set(&self, value: VirtualPower)` [@ANCHOR: radioctl:radioctl_station_vmeter_set]

Method of `VirtualMeterControl`: sets what the virtual meter reports, replacing the whole `VirtualPower` (forward watts, reflected watts, SWR in tenths, and the `silent` flag). `value` is taken by value. Takes the shared std mutex briefly (poison recovered); cannot fail. No I/O. The serving task reads the value at the next request, so the change applies to the next reply. Test support only.

### `pub fn serve_virtual_meter<T>(driver: MeterDriver, mut io: T) -> VirtualMeterControl` [@ANCHOR: radioctl:radioctl_station_vmeter_serve]

Serves a virtual meter of `driver` on `io`, the far end of a duplex stream or a pty, and returns its control. It spawns a tokio task (so it must be called inside a runtime) that reads bytes until a `;`, upper-cases the command, and answers `^WS;` (KPA500, `^WSppp sss;`), `^PWF;` and `^PWR;` (KPA1500, four digits), echoes a bare `;`, and ignores everything else, as the real amplifiers do. Values are capped to the field width. When the control's `silent` is set it answers nothing. The task ends when the stream closes or a write fails. The initial state is all zero and not silent. Cannot fail; I/O on `io` only.

## `crates/radioctl-station/src/virtual_rotator.rs`

### `fn step(from: f32, to: f32, amount: f32) -> f32` [@ANCHOR: radioctl:radioctl_station_vrot_step]

Private helper: moves `from` toward `to` by at most `amount` and returns the new value. All three are degrees (`amount` is degrees of movement in the elapsed time). If `to` is within `amount` it returns `to` exactly, otherwise it adds or subtracts `amount`. Pure, no I/O, no locks. It does not guard against a negative `amount` or NaN; callers pass a non-negative product of time and slew rate.

### `pub fn new(desc: RotatorDescription) -> VirtualRotator` [@ANCHOR: radioctl:radioctl_station_vrot_new]

Creates a virtual rotator at rest at the minimum of its travel: azimuth and elevation, and their targets, all start at the description's `az_min` and `el_min`. `desc` is taken by value and supplies the protocol, the travel limits, the slew rate, whether there is elevation and the park position. Not stuck, not silent, with empty received and input buffers and the internal clock at zero. Cannot fail, no I/O, no locks.

### `pub fn advance(&mut self, now: Duration)` [@ANCHOR: radioctl:radioctl_station_vrot_advance]

Lets simulated time pass up to `now`, an elapsed duration measured from the same epoch on every call. The rotator moves azimuth and elevation toward their targets by the elapsed time times `slew_deg_per_s`, each axis independently. A `now` earlier than the last one counts as no time and does not move the clock back. If `stuck` is set the clock still advances but the position does not. Mutates `self`; pure simulation, no I/O, no locks, cannot fail.

### `pub fn state(&mut self, now: Duration) -> RotatorState` [@ANCHOR: radioctl:radioctl_station_vrot_state]

Advances the simulation to `now` (see `advance`) and returns a copy of the `RotatorState`: current azimuth and elevation, and the target azimuth and elevation, in degrees. `&mut self` because it advances time. Cannot fail, no I/O, no locks. Calling twice with the same `now` returns the same state.

### `pub fn received(&self) -> &[String]` [@ANCHOR: radioctl:radioctl_station_vrot_received]

Returns every command line the rotator has received so far, in arrival order, as a borrowed slice. Lines are trimmed, empty lines are not recorded, and a line is recorded even when it is refused or the rotator is silent. Tests use it to check what the engine sent. No arguments besides `self`, no I/O, no lock, cannot fail.

### `fn set_target(&mut self, az: f32, el: f32) -> bool` [@ANCHOR: radioctl:radioctl_station_vrot_set_target]

Private: accepts a new target position in degrees if it lies inside the travel. If the description has no elevation, `el` is replaced with 0. Returns false, changing nothing, when the position is outside the description's limits (`contains`, which also rejects non-finite values); otherwise sets `target_az` and `target_el` and returns true. The rotator does not jump; it slews toward the target in `advance`. Mutates `self`, no I/O, no locks.

### `pub fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_station_vrot_feed]

Gives the rotator bytes received from the client at time `now` and returns the reply bytes to send back (empty when silent or when there is no reply). It first advances to `now`, appends `bytes` to an input buffer (cleared if it exceeds 4096 bytes, so a client that never ends a line cannot grow it), then handles each complete line: terminated by `\n` for rotctld and `\r` for GS-232. Each non-empty line is recorded in `received`, run through the protocol handler, and its reply appended unless `silent`. A partial line stays buffered. Cannot fail; no I/O (sans-IO), no locks.

### `fn rotctld(&mut self, text: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_station_vrot_rotctld]

Private: handles one trimmed rotctld command line and returns the reply. `P az el` sets a target: `RPRT 0` on success, `RPRT -1` if the target is outside the travel, `RPRT -8` if an argument is missing or not a number. `p` returns the current azimuth and elevation, six decimals, one per line. `S` stops by making the target equal to the current position (`RPRT 0`). `K` goes to the park position: `RPRT 0`, or `RPRT -11` when there is no park position or it is refused. Anything else gets `RPRT -4`. Mutates `self`, no I/O, no locks, cannot fail.

### `fn gs232(&mut self, text: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_station_vrot_gs232]

Private: handles one trimmed GS-232 command line (the carriage return has already been removed) and returns the reply. `Waaa eee` sets the target and replies with nothing when accepted, or `?>` plus CR LF when a number is missing or the target is outside the travel. `C2` replies with the position as `+0aaa+0eee` and CR LF, rounded to whole degrees. `S` stops (target becomes the current position, no reply). Any other command gets `?>`, as the real controller answers. Mutates `self`; no I/O; no locks; cannot fail.

### `fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T>` [@ANCHOR: radioctl:radioctl_station_vrot_lock]

Private helper that locks a std mutex and returns the guard, recovering the inner guard if a previous holder panicked (poisoning), so a panic in one test thread does not wedge the others. `m` is borrowed; the guard lives as long as the borrow. Blocking, synchronous, not to be held across an await. Cannot fail or panic.

### `pub fn state(&self) -> RotatorState` [@ANCHOR: radioctl:radioctl_station_vrot_control_state]

Method of `VirtualRotatorControl`: returns where the served rotator is and is going, after advancing the simulation to the real time elapsed since the control was created (the epoch). Takes the rotator mutex briefly. Returns a copy of the state in degrees. Cannot fail (a poisoned mutex is recovered), no I/O, not async.

### `pub fn received(&self) -> Vec<String>` [@ANCHOR: radioctl:radioctl_station_vrot_control_received]

Returns a copy of every command line the served rotator has received, in order, so a test can assert what the client sent. Takes the rotator mutex briefly and clones the list; the result is owned by the caller. Cannot fail, no I/O, not async.

### `pub fn set_stuck(&self, stuck: bool)` [@ANCHOR: radioctl:radioctl_station_vrot_control_set_stuck]

Makes the served rotator stuck (`true`: it accepts commands and updates its target but never moves) or frees it (`false`). It first advances the simulation to the present, so motion up to now is kept and only later motion is affected. Takes the rotator mutex briefly; cannot fail, no I/O, not async. Idempotent for the same value. Used to test a rotator that does not arrive.

### `pub fn address(&self) -> Option<SocketAddr>` [@ANCHOR: radioctl:radioctl_station_vrot_control_address]

Returns the socket address the virtual rotator listens on, as the listener reported it when `serve_virtual_rotator` started, so a test bound to port 0 can learn the real port. `None` if the listener could not report an address. Returns a copy; no lock, no I/O, cannot fail.

### `pub async fn stop(&self)` [@ANCHOR: radioctl:radioctl_station_vrot_control_stop]

Stops the serving task: it takes the task handle out of the control (under a short lock), aborts it and awaits its end, which drops the listener and the client connection, closing the port. The rotator state stays readable through the control afterwards. A second call, on this or a clone, finds no task and does nothing. Async; no error is returned (a cancellation result is ignored).

### `pub fn set_silent(&self, silent: bool)` [@ANCHOR: radioctl:radioctl_station_vrot_control_set_silent]

Makes the served rotator silent (`true`: commands are still recorded and acted on but no reply is sent) or lets it answer again (`false`). Takes the rotator mutex briefly. Cannot fail, no I/O, not async. Idempotent for the same value. Used to test a controller that never answers.

### `pub fn serve_virtual_rotator(runtime: &Handle, listener: TcpListener, rotator: VirtualRotator) -> VirtualRotatorControl` [@ANCHOR: radioctl:radioctl_station_vrot_serve]

Serves `rotator` on `listener` from a task spawned on `runtime`, and returns the control. `runtime` is a borrowed tokio handle, `listener` an already bound TCP listener (moved in), `rotator` is moved into a shared mutex. One client at a time: the task accepts a connection, sets no-delay, reads up to 256 bytes at a time, feeds them to the rotator with the elapsed real time and writes back any reply; when the client closes or a read or write fails it accepts the next, so a new connection replaces the old one. Does not return errors; the task runs until `stop`. Network I/O on the listener only.
