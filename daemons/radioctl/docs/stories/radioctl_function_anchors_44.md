# radioctl function anchors, batch 44

Function anchors for the `radioctl-station` crate, part 1: the error type (`error.rs`), the band follower (`band_follow.rs`), the pinned-certificate TLS (Transport Layer Security) connector (`tls.rs`), the rig spawner (`spawner.rs`), the board catalog and pin map (`gpio_map.rs`), the network relay driver for Shelly and Tasmota (`http_relay.rs`) and the powered-off radio record file (`record.rs`). Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
Transmit safety in this batch: none of these functions keys a transmitter. `follow_tx_frequency` and `follow_configured` do move an antenna selector, but only through `Station::select`, which applies the station interlock (refused while a rig is keyed or its keyed state is unknown). The HTTP relay driver's `write_line` switches a real relay when the station tells it to; the interlock and the transmit gate are applied by the `Station` above it, not here. `RigSpawner::spawn` leaves radioctl's transmit gate closed.

## `crates/radioctl-station/src/error.rs`

### `pub fn new(kind: StationErrorKind, detail: impl Into<String>) -> StationError` [@ANCHOR: radioctl:radioctl_station_error_new]

Builds a `StationError`, the one error type of the station layer. `kind` is the coarse class a caller branches on (`Config`, `Refused`, `KeyedUnknown`, `Driver`, `NotConfirmed`, `Timeout`, `Unknown`, `Store`); `detail` is the human-readable text, any `Into<String>` (taken by value, so a `&str` is copied). Pure, infallible, no I/O, no panics. Everything in the crate that fails builds its error with this function.

### `fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result` [@ANCHOR: radioctl:radioctl_station_error_display]

The `Display` form of a `StationError`: the kind's `Debug` name, a colon, a space and the detail, for example `Refused: the amplifier cannot power on while the rig is keyed`. `self` is borrowed; the result is the formatter's own (an error only if the sink fails). It is what the daemon puts into its reply lines and logs, so the kind name appears at the start of the text and is part of what scripts may match. Pure.

## `crates/radioctl-station/src/band_follow.rs`

### `pub fn route_for(routes: &[BandRoute], hz: Hz) -> Option<&BandRoute>` [@ANCHOR: radioctl:radioctl_station_band_route_for]

Chooses the band route for a frequency. `routes` is the configured list of `BandRoute` values (each has `low`, `high`, a selector `channel` and a `port`); `hz` is the transmit frequency in hertz. Returns a borrow of the first route in list order whose inclusive range `low <= hz <= high` holds it, so an overlap goes to the earlier route. `None` when no band holds the frequency, in which case the caller leaves the selector where it is (it never guesses a band). Pure, no panics, no I/O.

### `pub async fn follow_tx_frequency(station: &Station, rig: &RigHandle, routes: &[BandRoute], on_result: impl FnMut(&BandRoute, &Result<(), StationError>))` [@ANCHOR: radioctl:radioctl_station_band_follow_tx_frequency]

Keeps an antenna or band selector in step with a rig's transmit frequency. It is an async loop meant to be run as a task for the life of the rig and returns only when the rig's event stream ends. `station` is the station whose `select` is called; `rig` supplies `active_tx_frequency` (read with `Freshness::Cached`, so no radio round trip) and its event stream; `routes` is the table for `route_for`; `on_result` is called with the route and the outcome after every switch attempt. At start and after each relevant event it reads the frequency, picks the route and calls `Station::select(channel, port)` only when the wanted way differs from the one last applied successfully. A refused or failed switch (interlock `Refused` or `KeyedUnknown`, driver error, readback mismatch) is reported to `on_result`, is not recorded as applied and so is retried on the next event; a switch is thus never forced past the interlock. Only events that change `ActiveTxFrequency` or `Ptt` wake it (a `Ptt` change is what lets a switch that was refused because the rig was keyed go through). The loop awaits one event at a time and holds no lock across the wait. It never keys the transmitter; the antenna change is subject to the station interlock.

### `pub async fn follow_configured(station: &Station, rig_name: &str, rig: &RigHandle, on_result: impl FnMut(&BandRoute, &Result<(), StationError>))` [@ANCHOR: radioctl:radioctl_station_follow_configured]

Starts band following from the station's own configuration. `rig_name` is the name the rig has in the station definition; the routes are `station.def().band_routes_for(rig_name)`, which keeps only the `[[band_routes]]` whose channel belongs to that rig and that have not opted out. When the list is empty it returns at once without touching the rig; otherwise it runs `follow_tx_frequency` with those routes, so it likewise completes only when the rig's event stream ends. `rig` and `on_result` are as for `follow_tx_frequency`. Async; same interlock behaviour; no hardware change of its own.

## `crates/radioctl-station/src/tls.rs`

### `pub fn parse_pin(text: &str) -> Result<[u8; 32], StationError>` [@ANCHOR: radioctl:radioctl_station_tls_parse_pin]

Reads a SHA-256 certificate fingerprint as the owner typed it. Colons and any ASCII whitespace are removed first (so `AB:CD:...`, `ab cd ...` and the bare 64-digit form all work), and exactly 64 hexadecimal digits, upper or lower case, must remain. Returns the 32 bytes. A wrong length or a non-hex character gives `StationErrorKind::Config` with the text "the certificate fingerprint must be 64 hex digits (SHA-256)". `text` is borrowed. Pure, no I/O, no panics. Used by `connector` for the pinned mode.

### `pub fn connector(tls: &HaTls) -> Result<TlsConnector, StationError>` [@ANCHOR: radioctl:radioctl_station_tls_connector]

Builds the TLS client for a Home Assistant style `https` or `wss` connection. `tls` is the owner's setting: exactly one of `ca_pem` (the owner's own certificate authority, PEM text) or `pin_sha256` (the fingerprint of the one certificate to trust). With a CA, only that CA's certificates are trusted (the system trust store is not used) and an empty or unusable CA gives `Config`; with a pin, a custom verifier accepts only a server certificate whose SHA-256 equals the pin while the handshake signature is still checked against the certificate's key (so the peer must hold the private key); the TLS version set is rustls's safe default and the crypto provider is `ring`. Neither or both settings give `Config` ("TLS needs exactly one of ..."). No client certificate is sent. Pure construction: no network I/O happens until the connector is used. Built per connection attempt by the callers.

### `pub fn server_name(ip: IpAddr) -> ServerName<'static>` [@ANCHOR: radioctl:radioctl_station_tls_server_name]

Wraps a device address as the TLS server name. Station devices are addressed by IP (the owner's LAN), so the name a certificate must carry is the address itself, as an IP subject alternative name, not a DNS name. `ip` is copied. Pure, infallible. With a pinned fingerprint the name is not compared, with a CA it is.

## `crates/radioctl-station/src/spawner.rs`

### `pub fn tcp(description: RigDescription, address: impl Into<String>) -> RigSpec` [@ANCHOR: radioctl:radioctl_station_rigspec_tcp]

Describes a radio that is reached over TCP. `description` (taken by value) is the rig file the protocol engine is built from; `address` is `host:port` text. The spec uses `NetConfig::new(address)` for the reconnecting link, the raw byte stream (`framing: None`) and default `RigOptions`. Nothing is opened: a spec is only data, used later by `DescriptionSpawner::spawn`. Pure, infallible.

### `pub fn serial(description: RigDescription, path: impl Into<PathBuf>, baud: u32) -> RigSpec` [@ANCHOR: radioctl:radioctl_station_rigspec_serial]

Describes a radio on a serial port. `description` as above; `path` is the device node (for example `/dev/ttyUSB0`) kept as a `PortTarget::Path`; `baud` is the speed in bits per second, passed unchecked to the port setup at spawn time. Default `RigOptions`. Nothing is opened. Pure, infallible.

### `pub fn new() -> DescriptionSpawner` [@ANCHOR: radioctl:radioctl_station_dspawner_new]

An empty rig spawner: it knows no radios, so every `spawn` returns `Unknown` until `add` has been called. The same as `DescriptionSpawner::default()`. Takes no arguments and returns the spawner by value. It cannot fail and has no side effects. The spawner is filled with `add` and then handed to the radio manager, which calls `spawn` for each radio it must bring up.

### `pub fn add(&mut self, radio: DeviceId, spec: RigSpec)` [@ANCHOR: radioctl:radioctl_station_dspawner_add]

Registers the spec for one radio. `radio` is the station's device id; `spec` is moved in. An existing spec for the same id is replaced. Needs `&mut self`, so it is done while the spawner is being set up, before it is shared with the manager. Pure, no I/O.

### `fn fail(e: impl std::fmt::Display) -> StationError` [@ANCHOR: radioctl:radioctl_station_dspawner_fail]

Maps any engine, transport or actor error met while spawning to `StationErrorKind::Driver`, with the error's `Display` text as the detail. Used as the `map_err` of the spawn path, so the manager sees one kind for "the radio could not be brought up". Pure.

### `fn spawn(&self, radio: &DeviceId) -> BoxFuture<'_, Result<RigConnection, StationError>>` [@ANCHOR: radioctl:radioctl_station_dspawner_spawn]

The `RigSpawner` implementation used by the radio manager to bring a radio's actor up. Looks up the radio's `RigSpec` (none gives `Unknown` "no rig spec for <id>"), builds the protocol engine from its description (`engine_for`, failures become `Driver`), opens the link and starts the rig actor. For TCP it opens a reconnecting link with `first_attempts = 1` and the given framing (raw if none); for serial it opens the port. The link's status watch is set as the actor's `link_watch`, so a dropped link is visible as an unknown keyed state, and also returned in `RigConnection::watch`. The first connection is a single attempt; the manager's backoff spaces the retries. Returns the actor's `RigHandle` and watch. Async; must run inside a Tokio runtime (it uses `Handle::current()`). It opens a socket or serial port but sends nothing, and radioctl's transmit gate stays closed (nothing here can open it).

## `crates/radioctl-station/src/gpio_map.rs`

### `pub fn from_toml(text: &str) -> Result<BoardCatalog, StationError>` [@ANCHOR: radioctl:radioctl_station_catalog_from_toml]

Parses a board catalog: TOML with a `boards` array of `{ name, pins = [...] }`. A board must list at least one pin, no pin twice, and each name only once; any violation, or a TOML error, gives `StationErrorKind::Config` ("boards: ..."). `text` is borrowed. Returns the catalog keyed by board name. Pure.

### `pub fn builtin() -> BoardCatalog` [@ANCHOR: radioctl:radioctl_station_catalog_builtin]

The catalog that ships in the crate (`BUILTIN_BOARDS`, included from `data/station/boards.toml`: the pin lists of the supported relay boards such as `fx2-lcsoft-mini` and `sainsmart-16ch-modbus`). Parsed with `from_toml`; if the built-in text were ever invalid it yields an empty catalog (`unwrap_or_default`) rather than panicking; its test checks that the known boards are present. Pure, allocates.

### `pub fn get(&self, name: &str) -> Option<&BoardModel>` [@ANCHOR: radioctl:radioctl_station_catalog_get]

Looks a board model up by exact, case-sensitive name. `name` is borrowed; the result is a borrow of the `BoardModel` (its name and pin list) that lives as long as the catalog, or `None` when the board is not in the catalog (the caller then reports an unknown board model). It cannot fail, does no I/O and takes no lock; the catalog is read-only after it is built, so it may be shared freely between tasks.

### `pub fn from_toml(text: &str, catalog: &BoardCatalog) -> Result<GpioMap, StationError>` [@ANCHOR: radioctl:radioctl_station_map_from_toml]

Parses an owner's GPIO map (the board name and a list of lines, each with a pin, a function, a direction, an optional safe level, a channel name, `pulse_ms` and so on) and validates it against `catalog` with `GpioMap::validate`. A TOML error gives `Config` ("gpio map: ..."); so does any validation failure. Returns the validated map. Nothing is driven. Pure.

### `pub fn validate(&self, catalog: &BoardCatalog) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_map_validate]

Checks a pin map for mistakes that could mis-key hardware. `catalog` must know the map's board (else `Config`, "unknown board model"). For every line: the pin must be one the board exposes; no pin may be used twice; power, ptt, keyline, selector, momentary, band and heartbeat lines must be outputs and a sensor must be an input; a power, ptt or keyline line must define a safe state, and an input must not; power, ptt, keyline, selector, momentary and sensor lines need a non-empty channel name; a momentary line needs `pulse_ms` above zero; the rest of the function-specific fields must agree with the direction. The first problem is returned as `Config` ("gpio map: pin N: ..."), so a map either passes whole or is refused. Read-only on `self`; pure.

### `pub fn safe_levels(&self) -> BTreeMap<String, Level>` [@ANCHOR: radioctl:radioctl_station_map_safe_levels]

The pins that have a defined safe level, as a map from pin name to `Level`. The firmware or driver applies these when the controller releases or exits, so that a line with a safe level falls to it. Pins without a safe level (inputs, most selectors) are absent. Returns a new map; pure.

## `crates/radioctl-station/src/http_relay.rs`

### `pub fn is_private_or_loopback(ip: IpAddr) -> bool` [@ANCHOR: radioctl:radioctl_station_http_is_private]

True for an address on the owner's own network: IPv4 loopback, private (10/8, 172.16/12, 192.168/16) or link-local; IPv6 loopback, unique-local (`fc00::/7`) or link-local (`fe80::/10`). An IPv4-mapped IPv6 address is judged as its IPv4 form. Public addresses are false. `ip` is copied. Pure; it is the guard that keeps a network relay off the public internet.

### `pub fn parse_private_address(text: &str) -> Result<SocketAddr, StationError>` [@ANCHOR: radioctl:radioctl_station_http_parse_address]

Reads the device address the owner entered. `text` (trimmed) must be an IP literal, optionally with `:port`; a bare IP gets port 80. A host name or other text gives `Config` ("... such as 192.168.1.50") because no name resolution is done. An address that fails `is_private_or_loopback` gives `Config` ("a network relay must be on a private network address, never the public internet"). Returns the socket address. Pure.

### `pub fn new(family: Family, address: &str, relays: u32) -> Result<HttpRelay, StationError>` [@ANCHOR: radioctl:radioctl_station_http_new]

Builds the driver for one network relay device. `family` is `Shelly` or `Tasmota` (the URL dialect); `address` goes through `parse_private_address`; `relays` is the number of relays on the device and must be 1 to 32 (else `Config`, "a relay bank has 1 to 32 relays"). Nothing is sent. Pure construction; the driver is later given to the board as a `LineDriver`.

### `async fn get(&self, path_and_query: &str) -> Result<String, StationError>` [@ANCHOR: radioctl:radioctl_station_http_get]

One plain HTTP/1.0 GET to the device, with `Connection: close`, and returns the body of a 200 answer. `path_and_query` is the request target. The whole exchange (connect, send, read) has a 3-second limit (`IO_TIMEOUT`), giving `Timeout` ("the relay did not answer"); connect, send or read failures give `Driver`; the reply is read up to 16 KiB (`MAX_REPLY`); a reply with no header separator gives `Driver`; any status other than 200 gives `Driver` with the status line and a hint that a device with a password is not supported. No TLS, no authentication, no redirects. Network side effect: one TCP connection per call. Async; no lock held.

### `fn check(&self, line: u32) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_http_check]

Refuses a relay number outside the bank. `line` is zero-based; valid values are 0 to `relays - 1`, otherwise `Config` ("this relay bank has relays 0 to N"). Pure; called before any request so a bad index never reaches the device.

### `fn write_line(&self, line: u32, on: bool) -> BoxFuture<'_, Result<(), StationError>>` [@ANCHOR: radioctl:radioctl_station_http_write_line]

Switches one relay on or off. After `check`, the request is `/rpc/Switch.Set?id=<line>&on=<true|false>` for a Shelly and `/cm?cmnd=Power<line+1>%20On|Off` for a Tasmota (its relays are numbered from 1). Success is a 200 reply whose body is ignored; confirming the state is the board's job, by `read_line`. Errors are those of `check` and `get`. A real relay changes on success; callers reach this only through the station's interlock. Async, one network request.

### `fn read_line(&self, line: u32) -> BoxFuture<'_, Result<bool, StationError>>` [@ANCHOR: radioctl:radioctl_station_http_read_line]

Reads a relay's actual state from the device. Shelly: `/rpc/Switch.GetStatus?id=<line>`, JSON field `output` (bool). Tasmota: `/cm?cmnd=Power<line+1>`, JSON `POWER<n>` or `POWER`, compared case-insensitively with `ON`. A non-JSON body or a missing field gives `Driver`; `check` and `get` errors pass through. Returns `true` for on. Async; one request; no state change.

### `fn can_read_back(&self) -> bool` [@ANCHOR: radioctl:radioctl_station_http_can_read_back]

Always `true`: both device families report their relay state, so the board confirms each switch by reading it back and refuses to call it confirmed otherwise. Takes no arguments, returns a constant, cannot fail, does no I/O. A driver that returned `false` would make the board accept a write without confirmation; neither HTTP family does.

## `crates/radioctl-station/src/record.rs`

### `pub fn new(id: DeviceId, make: &str, model: &str, connect: ConnectProcedure) -> RadioRecord` [@ANCHOR: radioctl:radioctl_station_record_new]

A bare record for a radio that is registered but not yet seen: `id` the device id, `make` and `model` text (copied), `connect` how to connect and power it. No identity, description or capabilities, no last state, lifecycle `Unreachable` at time 0, no power channel, boot time 0, default health. Pure, infallible; `validate` is not called here.

### `pub fn validate(&self) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_record_validate]

Rejects a record that must not be trusted or saved: an empty id or one over 128 bytes; a `boot_time_ms` over `MAX_BOOT_MS` (one hour); make, model, description or connect notes over 4096 bytes; more than 1024 capabilities or one over 4096 bytes; a last state with more than 256 levels or a non-finite (NaN or infinite) level. Failure is `StationErrorKind::Store` with "record <id>: <reason>". Read-only; pure.

### `pub fn cached(&self, now_unix: u64) -> Option<Cached>` [@ANCHOR: radioctl:radioctl_station_record_cached]

What the powered-off radio last reported, for display while it is off. `now_unix` is the current time in Unix seconds; the age is `now_unix - last_state.seen_unix`, saturating at zero so a clock that went backwards reads as age 0. `None` if the radio was never read. Returns a clone of the state with its `age`. Pure.

### `pub fn new(path: impl Into<PathBuf>) -> RecordStore` [@ANCHOR: radioctl:radioctl_station_store_new]

A handle on the record file at `path`, any `Into<PathBuf>` (taken by value; relative paths are resolved against the working directory when the file is used). No file is opened, created or read, and the directory need not exist yet; `save` writes into the path's parent directory, so it must exist by then. Cannot fail; pure.

### `pub fn path(&self) -> &Path` [@ANCHOR: radioctl:radioctl_station_store_path]

The file this store reads and writes, as set by `new`; the returned `&Path` borrows from the store. Used for messages and tests. Takes no arguments; the path is exactly the one given to `new`, not canonicalised. Cannot fail; no I/O; no lock.

### `pub fn load(&self) -> Loaded` [@ANCHOR: radioctl:radioctl_station_store_load]

Reads the record file and returns `Loaded { records, problems }`; it never fails and never panics. A missing file gives an empty store and no problem. Other open or read errors give an empty store with a problem text. On Unix, group and other permission bits are first removed (`tighten`) and reported. At most `MAX_RECORD_FILE_BYTES` (4 MiB) are read; a larger file, one that is not JSON or not UTF-8, or one whose `schema` is not `RECORD_SCHEMA` (1) is renamed aside with `set_aside` and treated as empty. Inside a good file, a record that fails to parse, fails `validate` or whose stored id differs from its key is dropped, with a problem note. Side effects: file reads, a chmod and renames. Blocking file I/O: call from a blocking context or accept the short delay.

### `fn tighten(&self, file: &std::fs::File, problems: &mut Vec<String>)` [@ANCHOR: radioctl:radioctl_station_store_tighten]

Unix only. If the open record file has any group or other permission bit, sets its mode to 0600 and pushes a note ("mode 640 tightened to 600") to `problems`, or a note that it could not be tightened. `file` is the open handle (permissions are set on it, so there is no path race); `problems` is appended to. A file that is already private produces nothing. Called by `load`.

### `fn set_aside(&self, why: &str, problems: &mut Vec<String>)` [@ANCHOR: radioctl:radioctl_station_store_set_aside]

Keeps an unreadable file out of the way without deleting it: renames `<path>` to `<path>.bad-<n>` using the first `n` from 0 to 999 that is not taken. `why` is the reason ("corrupt", "too large", "schema"), put into the problem note with the new name; a rename failure is noted, and if all 1000 names are taken a "too many set-aside files" note is added. Appends to `problems`; never fails. Called by `load`.

### `pub fn save(&self, records: &BTreeMap<DeviceId, RadioRecord>) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_store_save]

Writes all records durably. Every record is validated first (a bad one gives `Store` and nothing is written). The JSON image (`schema` and a `radios` map) is written to `<path>.tmp-<pid>` created with mode 0600, `fsync`ed, then renamed over the real file, and the parent directory is `fsync`ed. A failure at any step removes the temporary file and returns `Store` ("write record file: ..." or "replace record file: ..."), leaving the old file intact, so a crash never leaves a half-written record. Blocking file I/O. Callers must not run two saves at once: the temporary name holds only the process id, so two concurrent saves from one process would share it (the radio manager is the one caller).
