# radioctl function anchors, batch 50

Function anchors for the `radioctl-station` crate, the Home Assistant driver in `homeassistant.rs`: the redacting `Secret` and the secret stores, the audit log, the `HaConfig` configuration, address and entity validation, the `HomeAssistant` channel driver (REST service calls, event-confirmed state) and the WebSocket session task that keeps the state cache. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]` or is a private helper reached through those tests.
Transmit safety in this batch: `HomeAssistant::write_line` switches real hardware (a switch, light or fan behind Home Assistant) over the network, so it can power a device that matters to transmitting, such as an amplifier. The driver reports `Assurance::Mediated`, the lowest level, and the switchboard (`channel.rs`) refuses a `ptt` channel on a mediated driver, so this driver is never a PTT path. The driver does not itself apply the interlock, the keyed-state check or the transmit gate; those run in the `Station` before a call reaches the driver. The driver only enforces its own allow-list: only configured entities of a writable domain can be written, and a read-only domain (`binary_sensor`) is refused with `Refused`. No lock is held across a network call: the `Mutex` guards here are short and only protect the caches.

## `crates/radioctl-station/src/homeassistant.rs`

### `pub fn new(value: impl Into<String>) -> Secret` [@ANCHOR: radioctl:radioctl_station_ha_secret_new]

Wraps a secret value. The argument `value` is anything convertible to `String` and is taken by value (moved or copied into the new `Secret`). Returns the `Secret` holding it. It cannot fail and never panics. No side effects, no locking, no I/O. The wrapper exists so the text is not printed by `Debug` and has no `Display`; use `expose` to reach the text. Calling it twice gives two independent values.

### `pub fn expose(&self) -> &str` [@ANCHOR: radioctl:radioctl_station_ha_secret_expose]

Returns the secret text, borrowed from the `Secret` for as long as the borrow lives. It is the only way to the plain text and is meant for the wire only: the HTTP bearer header in `post` and the WebSocket auth message in `session`. Argument: `&self`, read only. It cannot fail, takes no lock, does no I/O and is idempotent. Do not log or store the result.

### `fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result` [@ANCHOR: radioctl:radioctl_station_ha_secret_debug]

The `Debug` implementation of `Secret`. It always writes the fixed text `Secret(<redacted>)` to the formatter `f` (mutably borrowed, supplied by the formatting machinery) and never the value, so a secret cannot leak through `{:?}` in a log line, an error or a derived `Debug` of a struct holding it. Returns the formatter's `fmt::Result`, an error only if the underlying writer fails. No other side effects, no lock.

### `pub fn new() -> MemorySecrets` [@ANCHOR: radioctl:radioctl_station_ha_memsecrets_new]

Creates an empty in-memory secret store, the one used by tests or by a front end that has unlocked a vault and wants to hand secrets to the driver. No arguments. Returns a `MemorySecrets` with an empty map behind a `Mutex`; it is the same as `MemorySecrets::default()`. It cannot fail, does no I/O and reads nothing from disk.

### `pub fn set(&self, name: &str, value: &str)` [@ANCHOR: radioctl:radioctl_station_ha_memsecrets_set]

Stores a secret under a name in the in-memory store. Arguments: `name` and `value`, both borrowed and copied into owned strings; an existing secret with the same name is replaced. Takes `&self` because the map is behind a `Mutex`, which is locked for the insert only (a poisoned lock is recovered, see `lock`). Returns nothing, cannot fail and does no I/O. Nothing is validated, so an empty name or value is accepted here.

### `fn secret(&self, name: &str) -> Result<Secret, StationError>` [@ANCHOR: radioctl:radioctl_station_ha_memsecrets_secret]

The `SecretStore` implementation of `MemorySecrets`: looks `name` (borrowed) up in the map and returns a new `Secret` holding a copy of the value. Locks the map briefly. Error: when no secret has that name it returns `StationErrorKind::Config` from `missing_secret`, whose message names the secret but never a value. No I/O, no side effects, and repeated calls return equal results until `set` changes the entry.

### `pub fn new(dir: impl Into<PathBuf>) -> FileSecrets` [@ANCHOR: radioctl:radioctl_station_ha_filesecrets_new]

Creates a file-backed secret store over the directory `dir`, taken by value and kept as a path. Nothing is read or checked here: the directory need not exist yet, and every check happens in `secret`. Returns the `FileSecrets`. It cannot fail, takes no lock, and does no I/O, so it is safe to call on a startup path before the directory is mounted.

### `fn secret(&self, name: &str) -> Result<Secret, StationError>` [@ANCHOR: radioctl:radioctl_station_ha_filesecrets_secret]

The `SecretStore` implementation of `FileSecrets`: reads the file `<dir>/<name>`. The name must be plain: non-empty, not starting with a dot, and only ASCII letters, digits, underscore, hyphen and dot, so it cannot name a path outside the directory. The file must be a regular file; on Unix, if group or others have any permission bit (mode and 0o077 nonzero) it is refused with a message telling the owner to use mode 0600. One trailing newline is dropped and an empty result is refused. Errors, all `StationErrorKind::Config`: bad name, missing or unreadable file or directory (reported as a missing secret, with no operating system detail), a non-regular file, loose permissions, or an empty file. Side effect: one `metadata` call and one file read; no lock, and nothing is cached, so changes on disk are seen on the next call.

### `fn missing_secret(name: &str) -> StationError` [@ANCHOR: radioctl:radioctl_station_ha_missing_secret]

Builds the error returned when a secret cannot be found or is unusable: kind `StationErrorKind::Config` and the message "no secret named {name} in the secret store". Argument: `name`, borrowed, used only in the message; the secret value is never involved. Returns the `StationError`; it cannot fail, does no I/O and takes no lock. Used by both secret stores so a missing file and an empty file look alike to the caller.

### `pub fn new() -> MemoryAudit` [@ANCHOR: radioctl:radioctl_station_ha_memaudit_new]

Creates an empty in-memory audit log, the `AuditLog` used by tests and by anything that wants to show the recent history of Home Assistant actions. No arguments. Returns a `MemoryAudit` with no entries, equal to `MemoryAudit::default()`. It cannot fail, does no I/O and takes no lock. The log is not persisted: it is gone when the value is dropped.

### `pub fn entries(&self) -> Vec<AuditEntry>` [@ANCHOR: radioctl:radioctl_station_ha_memaudit_entries]

Returns a copy of every recorded audit entry, oldest first, as an owned `Vec` the caller may keep or change. Argument: `&self`, read only. It locks the entries mutex while cloning, so a very long log briefly delays a writer; a poisoned lock is recovered. It cannot fail and has no other side effects. The log holds at most the most recent 10000 entries, so the result is bounded. Repeated calls return a growing snapshot.

### `fn record(&self, entry: AuditEntry)` [@ANCHOR: radioctl:radioctl_station_ha_memaudit_record]

The `AuditLog` implementation of `MemoryAudit`: appends `entry` (taken by value, moved into the log) at the end, then drops the oldest entries so that at most 10000 remain. The check uses a saturating subtraction so it cannot underflow. It locks the entries mutex for the push and the drain, so it is cheap and safe from the session task and from callers at once. Returns nothing, cannot fail, does no I/O. Ordering is the order of arrival.

### `pub fn new(address: impl Into<String>, token_name: impl Into<String>, entities: Vec<String>) -> HaConfig` [@ANCHOR: radioctl:radioctl_station_ha_config_new]

Builds a configuration. Arguments, all owned: `address`, the Home Assistant IP literal with optional port, `token_name`, the name of the access token in the secret store, and `entities`, where line `n` is `entities[n]`. Defaults set here: request timeout 5 seconds, confirmation wait 3 seconds, ping every 15 seconds, `Backoff::default()` and no TLS (plain HTTP and WebSocket). Nothing is validated; `HomeAssistant::start` does that. Returns the `HaConfig`; it cannot fail and does no I/O. The public fields can be changed afterwards.

### `fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T>` [@ANCHOR: radioctl:radioctl_station_ha_lock]

Locks a mutex and returns the guard, ignoring poisoning: if another thread panicked while holding the lock, the inner guard is taken anyway instead of panicking. Argument: `m`, a borrowed `Mutex<T>` of any type. It blocks until the lock is free, so callers keep the critical section short and never hold it across an `.await`. It cannot fail or panic itself. Used for the secret map, the audit log, the state cache and the expectations map.

### `fn unix_now() -> u64` [@ANCHOR: radioctl:radioctl_station_ha_unix_now]

Returns the current wall-clock time in whole seconds since the Unix epoch, used to stamp audit entries (`AuditEntry::at_unix`). No arguments. If the system clock is before the epoch the conversion fails and the function returns 0 instead of an error or a panic. It reads the system clock only, takes no lock and has no other side effects. Not monotonic: a clock step changes the result.

### `fn driver_err(m: impl Into<String>) -> StationError` [@ANCHOR: radioctl:radioctl_station_ha_driver_err]

Builds a `StationError` of kind `StationErrorKind::Driver` with the message `m`, taken by value. It is the common failure for "Home Assistant cannot be reached, is not synced, or refused" so the caller (and the switchboard) can tell a driver or network failure from a configuration or refused request. Returns the error; it cannot fail, takes no lock and does no I/O. The message must not contain a secret and callers here never put one in.

### `pub fn parse_ha_address(text: &str) -> Result<SocketAddr, StationError>` [@ANCHOR: radioctl:radioctl_station_ha_parse_address]

Parses the owner's Home Assistant address. Argument: `text`, borrowed, whitespace trimmed. It accepts an IP socket address (`192.168.1.20:8123`) or a bare IP (port defaults to 8123); a host name is not accepted, so there is no DNS lookup. The address must pass `is_private_or_loopback` (private, loopback or link-local, including IPv4-mapped IPv6), so Home Assistant can never be on the public internet. Returns the `SocketAddr`. Errors, both `StationErrorKind::Config` with a plain-language message: not an IP, or a public address. Pure: no I/O, no lock.

### `pub fn check_entity(entity: &str) -> Result<bool, StationError>` [@ANCHOR: radioctl:radioctl_station_ha_check_entity]

Validates one entity id given as `domain.object_id`. Argument: `entity`, borrowed. Both parts must be non-empty and made only of lower-case ASCII letters, digits and underscore. Returns `Ok(true)` when the domain is writable (`switch`, `input_boolean`, `light`, `fan`), `Ok(false)` when it is read only (`binary_sensor`, a sensor channel), and `Err` of kind `Config` naming the entity when there is no dot, a character is not allowed, or the domain is not handled. The result is what `write_line` and `discover` use as the allow-list gate. Pure: no I/O, no lock; same input, same answer.

### `pub fn start(runtime: &Handle, config: HaConfig, secrets: &dyn SecretStore, audit: Arc<dyn AuditLog>) -> Result<HomeAssistant, StationError>` [@ANCHOR: radioctl:radioctl_station_ha_start]

Creates the driver and starts its session task. `runtime` is a borrowed tokio handle on which the task is spawned; `config` is moved in; `secrets` is only borrowed for the token read; `audit` is a shared log kept by the driver. Validation happens first and before any network use: the address (`parse_ha_address`), the entity count (1 to 64), every entity (`check_entity`), the TLS connector if `config.tls` is set, and the token read once by `config.token_name`. Errors: `Config` for a bad address, count, entity or TLS setup, and whatever the secret store returns for a missing token. Nothing is switched. On success the background `run_sessions` task is running and tries to connect with its backoff. The returned driver aborts the task when dropped.

### `pub fn is_synced(&self) -> bool` [@ANCHOR: radioctl:radioctl_station_ha_is_synced]

Reports whether the WebSocket session is up, authenticated and has received the full `get_states` snapshot; only then is any entity state known. Argument: `&self`. Returns the cache's `synced` flag, read under a brief lock of the state cache. It cannot fail, does no I/O and is cheap. The value can change at any moment as the session task connects or loses the connection, so a true result is not a promise about the next call.

### `pub fn discover(&self) -> Result<Vec<Discovered>, StationError>` [@ANCHOR: radioctl:radioctl_station_ha_discover]

Lists the switchable and readable entities Home Assistant currently reports so the owner can choose lines. Argument: `&self`. Returns a `Vec<Discovered>` sorted by entity id, each with its state, friendly name, whether it is writable (by `check_entity`) and whether it is already one of the configured entities. Reads only the cache; writes nothing to Home Assistant. Error: `StationErrorKind::Driver` if the session is not synced. Side effect: one audit entry, `discover`, with `ok` or the error. Locks the cache briefly. Not async, no network call.

### `fn entity(&self, line: u32) -> Result<&str, StationError>` [@ANCHOR: radioctl:radioctl_station_ha_entity]

Maps a line number to its entity id: line `n` is `entities[n]` of the configuration. Argument: `line`, zero based. Returns a borrow of the id string. Error: `StationErrorKind::Config` saying the line is past the number of entities when `line` is out of range. Pure and bounds-checked (uses `get`, never indexes, so it cannot panic). No lock, no I/O.

### `fn drop(&mut self)` [@ANCHOR: radioctl:radioctl_station_ha_drop]

Drop implementation of `HomeAssistant`: aborts the background session task so the connection stops and no more reconnect attempts or audit entries happen. Argument: `&mut self`. It does not wait for the task to finish and does not switch anything: entities keep whatever state they have in Home Assistant, so dropping the driver does not turn a device off. Cannot fail or panic. Aborting an already finished task is harmless.

### `fn assurance(&self) -> Assurance` [@ANCHOR: radioctl:radioctl_station_ha_assurance]

Reports `Assurance::Mediated`, the lowest assurance level, because Home Assistant, the network and the device behind it can each be down or slow. The switchboard uses it to refuse a `ptt` channel on this driver and the station definition requires an independent stop for an amplifier powered through it. Argument: `&self`, unused. Constant result, cannot fail, no lock, no I/O.

### `fn write_line(&self, line: u32, on: bool) -> BoxFuture<'_, Result<(), StationError>>` [@ANCHOR: radioctl:radioctl_station_ha_write_line]

Switches the entity of `line` on or off by calling the Home Assistant service `turn_on` or `turn_off` in the entity's domain (a REST POST to `/api/services/<domain>/<service>` with `entity_id`). Arguments: `line` zero based, `on` the commanded state. It is sent even while the state is unknown, because an off must always be attempted. Before sending it records the expected state so `read_line` can wait for the confirming event; the expectation is removed again if the POST fails. Errors: `Config` for a line out of range, `Refused` (and an audit entry) for a read-only entity such as a `binary_sensor`, and `Driver` or `Timeout` from `post`. Success only means Home Assistant accepted the call, not that the device changed. Async; one network exchange, bounded by the request timeout; an audit entry is written for success and failure. This is real hardware switching: the interlock and transmit gate are applied above, in the station.

### `fn read_line(&self, line: u32) -> BoxFuture<'_, Result<bool, StationError>>` [@ANCHOR: radioctl:radioctl_station_ha_read_line]

Returns the entity's state from the event cache: true for `on`, false for `off`. If a command was just written for this entity, the call waits, up to `confirm_wait`, for a state event that shows the commanded state, woken by the session's change notification, and returns the first matching state; if the deadline passes it returns the current known value (or error) and clears the expectation. Argument: `line`, zero based. Errors: `Config` if the line is out of range and `Driver` when the state is unknown: the session is down or not synced, the entity has not been reported, or it reports anything but `on` or `off` (`unavailable`, `unknown`). Async; no network call, only the cache and a bounded wait. The locks are taken briefly and not held across the wait.

### `fn can_read_back(&self) -> bool` [@ANCHOR: radioctl:radioctl_station_ha_can_read_back]

Reports that this driver can read a state back, because Home Assistant publishes entity state; the switchboard therefore asks `read_line` to confirm writes. The answer is always `true`, whether or not the session is currently synced; an unknown state is reported by `read_line` as an error instead. Argument: `&self`, unused. Cannot fail, no lock, no I/O.

### `fn record(&self, action: &str, entity: Option<&str>, result: Result<(), &str>)` [@ANCHOR: radioctl:radioctl_station_ha_record]

Writes one entry to the audit log of the driver. Arguments, all borrowed: `action`, a short description such as `service switch.turn_on`, `discover` or `websocket lost`; the `entity` concerned or `None`; and `result`, where `Ok` is stored as `ok` and `Err(why)` as `error: <why>`. The entry is stamped with `unix_now`. Calls the `AuditLog` implementation, which decides its own locking and storage. Returns nothing and cannot fail. It never receives the token, so entries hold no secret.

### `fn state_of(&self, entity: &str) -> Result<bool, StationError>` [@ANCHOR: radioctl:radioctl_station_ha_state_of]

Looks an entity up in the state cache and maps it to a boolean. Argument: `entity`, borrowed id. Returns `Ok(true)` for `on`, `Ok(false)` for `off`. Errors of kind `Driver`: the session is not synced (the state of every entity is unknown), the entity has not been reported, or it reports any other text such as `unavailable` or `unknown`; the message includes the entity and the reported text. Reads under a brief lock of the cache, does no I/O and has no side effects.

### `async fn post(&self, path: &str, body: &str) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_ha_post]

Sends one HTTP/1.0 POST to Home Assistant and checks the status line. Arguments: `path`, the request path, and `body`, a JSON string, both borrowed. The request carries the bearer token (`Authorization: Bearer`), JSON content type and `Connection: close`; the reply is read to the end of the connection, at most 64 KiB. A TLS server that closes without a close notify is treated as end of data if something was received. The whole exchange is limited by `request_timeout`. Result: `Ok` for a status containing 200 or 201. Errors: `Timeout` when no answer comes in time; `Driver` when the connection, send or read fails, for a 401 or 403 (the access token was rejected) and for any other status, whose line is quoted. Async; opens and closes one connection per call and holds no lock. Not idempotent: a retry sends the service call again.

### `fn apply_state(&self, entity: &str, state: &str, name: Option<String>)` [@ANCHOR: radioctl:radioctl_station_ha_apply_state]

Records one entity's state in the cache, from a `state_changed` event or a `get_states` snapshot entry. Arguments: `entity` and `state`, borrowed; `name`, the friendly name if the message had one, otherwise the previously kept name is retained. Entities whose domain is neither writable nor read only are ignored, as is a new entity once the catalog holds 20000 entries (existing entities are still updated). Takes the cache lock briefly. Returns nothing and cannot fail. It does not wake waiters; the session calls the change notification afterwards.

### `fn set_synced(&self, synced: bool)` [@ANCHOR: radioctl:radioctl_station_ha_set_synced]

Marks the cache usable (`true`) or discards it (`false`). Argument: `synced`. When false all cached states are cleared, so after a lost session no stale state can be read as confirmed, and every entity is unknown until the next snapshot. The cache lock is taken for the change and released before it wakes every task waiting in `read_line` through the change notification, so waiters re-check at once. Returns nothing, cannot fail, no I/O. Calling it repeatedly with the same value is harmless.

### `async fn run_sessions(inner: Arc<Inner>)` [@ANCHOR: radioctl:radioctl_station_ha_run_sessions]

The long-running session task spawned by `start`. Argument: `inner`, the shared driver state, owned (an `Arc`). It loops forever: run one `session`, mark the cache not synced, audit the end as `websocket lost` with the reason, increase the failure count (saturating) and sleep for the configured `Backoff` delay for that count. A successful sync inside `session` resets the count to zero, so a stable connection reconnects after the first delay. It never returns and does not panic; it ends only when the task is aborted by the driver's `Drop`. Async; network connections and audit entries are its side effects, and it holds no lock across an await.

### `async fn open_stream(addr: SocketAddr, tls: Option<&TlsConnector>) -> Result<Box<dyn Io>, String>` [@ANCHOR: radioctl:radioctl_station_ha_open_stream]

Opens a byte stream to Home Assistant: a TCP connection to `addr`, with Nagle disabled (best effort), and, when `tls` is given, a TLS handshake on top, with the server name taken from the IP address (the certificate must match it or the pin, see `tls.rs`). Arguments: `addr` by value, `tls` borrowed. Returns a boxed stream. Errors are strings prefixed `connect:` or `TLS:` carrying the underlying error; callers wrap or log them. Async; no timeout of its own, so callers wrap it in the request timeout. No lock.

### `async fn read_head(stream: &mut (impl AsyncRead + Unpin)) -> Result<(String, Vec<u8>), String>` [@ANCHOR: radioctl:radioctl_station_ha_read_head]

Reads an HTTP response head from `stream` (mutably borrowed) up to and including the blank line (`\r\n\r\n`), in 1024-byte reads. Returns the head as text (lossy UTF-8) and any bytes that came after it, which belong to the first WebSocket frames. Errors as strings: the head exceeds 8192 bytes without an end, the peer closes during the handshake, or a read fails. Async; reads from the connection and has no other side effects or lock. No timeout of its own; the session wraps it in one.

### `async fn send_json(stream: &mut (impl AsyncWrite + Unpin), value: &Json) -> Result<(), String>` [@ANCHOR: radioctl:radioctl_station_ha_send_json]

Sends one JSON value as a single masked WebSocket text frame. Arguments: `stream`, mutably borrowed and written to, and `value`, borrowed and serialised with `to_string`. Clients must mask frames, so a fresh random 4-byte mask is used each time. Returns `Ok(())` after the frame is written; error is a `write:` string if the write fails. Async; the only side effect is bytes on the connection. No lock, no timeout of its own, and it must not be used with a value holding more than the server accepts.

### `async fn session(inner: &Arc<Inner>, failures: &mut u32) -> Result<(), String>` [@ANCHOR: radioctl:radioctl_station_ha_session]

Runs one WebSocket session to Home Assistant until it fails; it never returns `Ok` in normal use. Arguments: `inner`, the shared state, and `failures`, a mutable counter reset to zero once the first full snapshot has been applied. Steps: connect and do the HTTP upgrade handshake, each limited by `request_timeout`; answer `auth_required` with the token; on `auth_ok` subscribe to `state_changed` (id 1) and request `get_states` (id 2); apply each snapshot and event with `apply_state`, set the cache synced after the snapshot and wake waiters on every change. It answers pings, sends its own ping (ids from 101) every `ping_every` and fails if the previous one is not answered within a further interval. Returns `Err` with a reason for: connect or handshake failure or timeout, a rejected token (audited as `websocket auth` with `token rejected`), a failed subscribe or `get_states`, a close or end of stream, invalid JSON, a decoder error (frames above 16 MiB) and a missed pong. Async; network I/O, audit entries `websocket auth` and `websocket connect`, and cache updates are the side effects; it holds no lock across an await.
