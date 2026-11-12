# hams-relay-broker (prototype)

The relay's privileged broker from `hams_com/docs/proposals/RELAY_ZERO_PRIVILEGE_SPLIT.md`
(sections 3.1, 3.5, 3.6, 5, 6), phase 3 of that plan, built as a standalone crate. **Nothing here
is wired into the relay or installed by any package**; the integration path is at the end.

A small separate process (about 2,000 lines of source without tests; no web, no async runtime,
no FFI, no shell, two commented `unsafe` blocks that adopt kernel-provided descriptors) that
answers a fixed set of typed requests from authenticated local peers over a Unix domain socket:

| Action | What it does | Privilege it needs |
|---|---|---|
| `list_devices` | serial ttys and ALSA (the Linux sound system) nodes from sysfs (read only), USB ids, product and manufacturer strings; protected devices are left out | none |
| `open_device` | validates one path against the policy, opens it `O_NOFOLLOW|O_CLOEXEC|O_NOCTTY`, re-checks the open descriptor, passes it with SCM_RIGHTS; the worker never needs `/dev` access and never learns another device's path | group membership (`dialout`, `audio`) or, failing that, one capability (see the unit) |
| `grant_group` / `provision_groups` | reports which of `dialout`/`audio` the **caller's own** account lacks; changes nothing unless `apply_groups = true`, then runs `/usr/sbin/gpasswd -a USER GROUP` with a fixed argv | root or a gpasswd setuid, only with apply |
| `bind_udp` | binds a UDP socket on a port the config lists (default only 123, the relay's time service) on the wildcard address and passes the descriptor; any other port is `Denied` | root (privileged port) |
| `update_package` | asks for the relay package upgrade: creates `/var/lib/hams-relay-broker/update-requested`, which the package's root-owned `hams-relay-update.path` unit turns into the one fixed `apt-get install --only-upgrade hams-local-relay` (or `dnf upgrade`), because the broker itself holds no right to run a package manager; nothing from the caller; only with `update_package = true` in the config (the relay package's config turns it on) |
| `set_system_time` / `time_status` | `set_system_time` steps the system clock to a UTC the relay chose (`CLOCK_REALTIME`; needs `CAP_SYS_TIME`), only with `set_time = true` (the relay package's config turns it on). The broker applies its own ceilings and does not trust the relay: the target must not be before the build date (2026-11-01); a step beyond `max_unproven_step_secs` (default 30) needs a GPS source or two agreeing sources; at most `max_time_steps_per_hour` (default 4) steps an hour, then one more attempt trips a circuit breaker that refuses everything for an hour. Every attempt is audited with the clock before, the target, the source and the evidence. `time_status` is read-only: the clock, steps in the last hour, breaker state, the earliest accepted target | root or `CAP_SYS_TIME` |
| `hub_start` / `hub_stop` / `hub_status` | the stand-alone emergency hub: only with `hub = true` in the config. Validates the typed settings, writes `hostapd.conf` and `dnsmasq.conf` from fixed templates into `/run/hams-relay-broker/hub` (mode 0600), gives the wifi interface the hub address with `ip` and starts `hostapd -B` and `dnsmasq` with fixed argument lists; stop signals only a pid whose `/proc/PID/comm` still names the program, then removes the files | root |

Every request is rate-limited per UID (user ID) and written to an append-only `events.jsonl` audit log (a peer turned away is logged once per UID per minute; the file moves to `events.jsonl.1` at 4 MiB).

## Wire protocol (`src/wire.rs`, `src/transport.rs`)

Length-prefixed frames (`u32` big-endian, 1..=4096 bytes) over a Unix stream socket; the body is
a version byte (1), a tag byte and typed fields; strings are `u16` length + UTF-8, at most 255
bytes, no NUL. The schema is closed: four request variants, four response payloads, six status
codes. Unknown tags, short buffers, trailing bytes and over-long strings are errors, never
panics (`tests/fuzz_parser.rs`, proptest, 2,000 cases per property; the same three functions are
what a cargo-fuzz harness would call). A passed descriptor rides in SCM_RIGHTS alongside the
`Opened` response; descriptors a *client* sends are closed unread.

## Peer authentication (`src/auth.rs`)

`SO_PEERCRED` on accept, before any byte is read. The UID or primary GID must be on the
allow-list in the root-owned config (`/etc/hams-relay-broker.conf`, `key = value`, unknown keys
are errors; it must be owned by uid 0 and not group/world-writable). An empty allow-list answers
nobody. Unlisted peers are logged and closed with no response.

The other direction: a client reads the server's `SO_PEERCRED` after `connect` and sends nothing
unless the uid is one it expects (`client::verify_server`; `connect_and_call` expects root, which
is what a socket-activated broker shows; `connect_and_call_as` names other accounts). A process that
took over the socket path therefore cannot answer the relay, and the relay does not depend on the
installer making the socket's directory writable by root alone.

## Policy (`src/policy.rs`)

Three checks, each closing a different hole: lexical (absolute, under `/dev`, no `.`/`..`/empty
segment, byte-for-byte what will be opened, not on `deny_path`: `/dev/mem`, `/dev/kmem`,
`/dev/port`, `/dev/console`, `/dev/tty`), node walk (no symlink anywhere in the chain, so
`/dev/stdin`, `/dev/fd/0`, `/dev/serial/by-id/...` and a planted link are all refused; the final
node is a character device whose major belongs to the requested class: 4/166/188/204/207
serial, 116 ALSA; disks are block devices and fail here), USB identity (the VNWA codec
`08bb:29c0` and anything on `deny_usb`; this check **fails closed**: a `tty*` or `snd/*` node whose sysfs entry cannot be read, so its USB identity is unknown, is refused with "usb identity unreadable", because otherwise hiding sysfs would bypass `deny_usb`. A host that must run without readable sysfs, such as a container, says so explicitly with `allow_unreadable_usb = true` in the config and then gives up the VNWA protection; a node that sysfs describes as not USB is unaffected), and a post-open `fstat` re-check so a swap between the
walk and the open fails closed.

## Threat model

Protects against: a compromised relay worker (web parser, serial reply parser, decoder) using the
broker to open a device it was not meant to have (a disk, `/dev/mem`, another rig's port, the
VNWA), to reach a device by a symlink or a traversal, to flood the broker, or to change group
membership of any account but its own; a stranger on the machine talking to the socket (socket
mode 0660 plus the allow-list); a malformed or oversize request crashing or stalling the broker
(bounded frames, strict parser, I/O timeouts, one request per connection); provisioning changing
the machine without an explicit configuration decision (dry run by default). No action prompts,
blocks on an operator, or asks for a password: the emergency path has none of those (3.5, 3.6).

NOT protected: a compromised **core** (it holds the keys; the broker never sees them and cannot
help); a worker that already holds a descriptor (the broker is off the data path by design, 3.5);
the config file itself (root-owned by assumption; the broker refuses other owners but cannot
defend against root); `pid` in the audit log (reusable, informational); the gpasswd binary and
`/etc/group` on apply; a kernel driver bug reachable through a *legitimately* opened device;
denial of service by a legitimate peer against itself (rate limit is per UID); TOCTOU (time-of-check to time-of-use race) on sysfs
USB identity (the id is read before the open; a device re-enumerating between the two is a
hardware event, not an attacker); macOS and Windows (the `Os` trait is the seam, nothing is
implemented there yet).

## Root-only paths and how they are tested

Tests run unprivileged (`socketpair`, temporary directories, the real `/dev/null`, a sysfs tree
built in a tempdir, `/etc/group`-shaped files). What cannot run without root and is covered only
by the `Os` trait fake plus review: opening a real `/dev/ttyUSB*` as a non-member of `dialout`,
a successful `gpasswd` apply, the root-ownership check on the config (the same code path is
exercised with the test's own UID), and the systemd units (examples under `packaging/systemd`,
never installed; `CapabilityBoundingSet=` empty by default, one capability to be added per
action only when measured necessary).

The emergency hub's generated configuration has one more test, `tests/hub_netns.rs`, skipped unless `HAMS_HUB_NETNS=1`. It runs the real `dnsmasq`
from `hub::dnsmasq_conf` in a network namespace joined to a client namespace by a veth pair (no Wi-Fi), takes a DHCP (automatic network address assignment) lease (router, DNS server and NTP
server options, pool, lease time) and resolves the relay name and the captive-portal probe names against it; `hostapd` cannot run without a radio, so
`hub::hostapd_conf` is only parsed by the real `hostapd` (a deliberately broken file must be rejected). It needs passwordless `sudo ip`, and runs as a
bounded job: `agent_procs.py run --tag NAME hubns -- timeout 120 env HAMS_HUB_NETNS=1 cargo test --test hub_netns`.

## Build and test

```sh
cd daemons/relay_broker
CARGO_TARGET_DIR=... nice -n 10 cargo test
cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --check && cargo deny check
# development run (no systemd):
hams-relay-broker --config ./broker.conf --config-owner $(id -u) --socket /tmp/broker.sock
cargo run --example broker_client -- /tmp/broker.sock list
cargo run --example broker_client -- /tmp/broker.sock open serial /dev/ttyUSB0
```

## Path to integration (phases 3 to 6 of the proposal; nothing done yet)

Relay call sites this broker replaces, from `RELAY_PRIVILEGE_INVENTORY.md` and
`privilege_inventory.json` (broker-class modules):

- `audio_open_policy::check`/`plan_open` (the single "may this device be opened" decision) moves
  into `policy.rs` as the audio class rules; the relay keeps the user-facing choice logic.
- `serial_permissions::fix_serial_permissions` and `privileged_action::run_pkexec` become
  `provision_groups` (headless, no polkit); `missing_groups` becomes a dry-run `provision_groups`.
- `discovery` (serial/USB enumeration over `/dev/tty*`, `/sys/bus/usb`), `hotplug` (serial
  scans; the uevent socket stays in the radio worker), `audio_probe` (`/proc/asound`,
  `/dev/snd`, the `sh` subprocess) and `audio_select` (`/dev/serial`, `pactl`) use
  `list_devices`; `firmware_guard` becomes a `deny_usb` table entry per printer id.
- Every serial and ALSA open in the radio and media workers becomes `open_device` and receives
  a descriptor. Hamlib, direwolf and the tty bridges take a path, not a descriptor
  (proposal section 5): they need `/proc/self/fd/N` or a pty shim, or stay in a worker that is
  allowed the device directly.
- `tool_install` and `updater` are not in this prototype: by 3.7 they become package-manager
  calls (`update-package` as a fixed action) once the package repository exists.

Remaining steps: (3) wire the relay's call sites above behind a feature flag with the
`client` module, and run the fuzz target in CI; (4) split the web front; (5) split radio, media
and decoders with the latency measurement gate; (6) per-worker systemd restrictions. Tracked in
`hams_com/night_shift_todo/high/`.

## Conventions

- Anchors (ADR 0090): a function carries `// [@ANCHOR: id]` in source, its test carries
  `// Tests [@ANCHOR: id]`, and this README cites it as `[@ANCHOR: relay_broker:id]` below.
  `tests/conventions.rs` fails when source, tests and README disagree or a file lacks its SPDX
  header.
- Tests use the production code paths; the only test seam is the `Os` trait.

## Function reference

### `TimeGuard::new` (time_guard.rs) [@ANCHOR: relay_broker:relay_broker_time_guard_new]

**Purpose.** Builds the clock-step guard that `Broker` consults before every `set_system_time`: the configured ceilings, an empty step history and a closed circuit breaker. The broker makes one guard when it is assembled (`Broker::new`) and keeps it for its lifetime, so the step count survives across requests but not across a broker restart.

**Signature.** `pub fn new(max_unproven_step_secs: u64, max_steps_per_hour: u32) -> TimeGuard`

| Argument | Type and range | Meaning |
|---|---|---|
| `max_unproven_step_secs` | `u64`, seconds | The largest step (absolute difference between the current clock and the target) allowed without proof. A larger step needs a GPS source or two agreeing sources. Comes from the config key of the same name. |
| `max_steps_per_hour` | `u32` | How many completed steps are allowed in any rolling hour. Stored as `usize`. Zero means every check trips the breaker. |

**Returns.** A `TimeGuard` with `steps` empty and `breaker_until = None`. **Errors.** None; it cannot fail or panic. **Concurrency.** Plain value, no locks; the broker serves one connection at a time, so the guard needs none. **Side effects.** None. **Tests.** `the_ceilings_hold_and_the_breaker_trips_and_resets` (time_guard.rs).

### `TimeGuard::check` (time_guard.rs) [@ANCHOR: relay_broker:relay_broker_time_guard_check]

**Purpose.** The decision half of the broker's own clock ceiling. It does not trust the relay's judgement: it refuses a clock step that is implausible, unproven or too frequent. `Broker::set_system_time` calls it after the config switch and before the OS call. Checks run in a fixed order: expire old state, breaker open, target before the build date, large step without proof, steps per hour (which trips the breaker).

**Signature.** `pub fn check(&mut self, now: Instant, current_unix_secs: u64, target_unix_secs: u64, source: TimeSource, agreeing: u8) -> Result<(), TimeDenial>`

| Argument | Type and range | Meaning |
|---|---|---|
| `now` | `Instant` (monotonic) | The time of the decision; passed in so tests need no clock. Not wall-clock, so it is immune to the step being checked. |
| `current_unix_secs` | `u64`, Unix seconds | The wall clock now; the step size is its absolute difference to the target. |
| `target_unix_secs` | `u64`, Unix seconds | The time the relay asks for. Refused when earlier than `BUILD_DATE_UNIX_SECS` (2026-11-01 UTC). |
| `source` | `TimeSource` | The relay's claimed source; `Gps` counts as proof by itself. |
| `agreeing` | `u8` | How many independent sources agree with the target; 2 or more count as proof. The broker cannot verify the claim, only the relay's own evidence rule. |

**Returns.** `Ok(())` when the step may proceed. An allowed step is NOT recorded: the caller calls `record_step` once the OS call succeeded. **Errors** (`TimeDenial`): `BreakerOpen` (breaker already open, or this call found the hour's step count full and opened it for an hour), `BeforeBuildDate`, `UnprovenLargeStep`. **Side effects.** Mutates the guard: expires state and may open the breaker, but never records a step. **Security.** This is the clock-poisoning ceiling: a compromised relay cannot walk the clock back or forward in a flood of steps. **Tests.** `the_ceilings_hold_and_the_breaker_trips_and_resets` (time_guard.rs).

### `TimeGuard::record_step` (time_guard.rs) [@ANCHOR: relay_broker:relay_broker_time_guard_record_step]

**Purpose.** Counts one completed clock step toward the per-hour limit. `Broker::set_system_time` calls it only after the OS `clock_settime` succeeded, so a refused or failed step never uses up the allowance.

**Signature.** `pub fn record_step(&mut self, now: Instant)`

| Argument | Type and range | Meaning |
|---|---|---|
| `now` | `Instant` | The monotonic time of the completed step; kept for one hour and then dropped by `expire`. |

**Returns.** Nothing. **Errors.** None. **Side effects.** Pushes onto the back of the in-memory `steps` queue; the queue is bounded in practice because `check` refuses and trips the breaker once `max_steps_per_hour` entries are present. It does not itself open the breaker. **Concurrency.** No locking; one request at a time. **Tests.** `the_ceilings_hold_and_the_breaker_trips_and_resets` (time_guard.rs): two recorded steps fill an hour of two.

### `TimeGuard::status` (time_guard.rs) [@ANCHOR: relay_broker:relay_broker_time_guard_status]

**Purpose.** Reports the guard's state for the read-only `time_status` action: how many steps were made in the last hour and whether the circuit breaker is open. It is what the relay shows an operator who asks why a clock step was refused.

**Signature.** `pub fn status(&mut self, now: Instant) -> (u8, bool)`

| Argument | Type and range | Meaning |
|---|---|---|
| `now` | `Instant` | The time of the query; old steps and an expired breaker are dropped first (`expire`), so the answer is current. |

**Returns.** `(steps in the last hour, breaker open)`. The count saturates at 255 (`u8::MAX`). **Errors.** None. **Side effects.** Calls `expire`, so it mutates the guard (it takes `&mut self`) but changes no answer a later `check` would not also reach. **Tests.** `the_ceilings_hold_and_the_breaker_trips_and_resets` (time_guard.rs): `(2, false)`, `(2, true)` while open, and `(0, false)` after the hour.

- [@ANCHOR: relay_broker:relay_broker_broker_set_system_time] (broker.rs) Steps the clock when enabled and inside the ceilings; audits every outcome with before, target, source and evidence.
- [@ANCHOR: relay_broker:relay_broker_broker_time_status] (broker.rs) The clock and the step limits; read-only, always served.
- [@ANCHOR: relay_broker:relay_broker_os_system_time] (os.rs) The wall clock now in Unix seconds (the Linux layer also steps it with `clock_settime`).
### `json_escape` (audit.rs) [@ANCHOR: relay_broker:relay_broker_json_escape]

**Purpose.** The only JSON encoding the audit log needs: it escapes a string for use inside a JSON string literal, so the hand-formatted lines stay valid whatever a path or reason contains (the broker has no serialisation dependency). Called by `AuditEvent::to_json_line` for `action`, `outcome` and `detail`.

**Signature.** `pub fn json_escape(s: &str) -> String`

| Argument | Type and range | Meaning |
|---|---|---|
| `s` | `&str` | Any text; borrowed, not retained. It may hold peer-supplied bytes (a device path), which is why escaping matters. |

**Returns.** A new `String`: `"` becomes `\"`, `\` becomes `\\`, newline, CR and tab become `\n`, `\r`, `\t`, other control characters below U+0020 become `\u00xx`, everything else is copied unchanged (non-ASCII stays UTF-8). **Errors.** None. **Side effects.** None; pure. **Security.** Stops a peer-controlled string from breaking out of its field or forging a second log line (a newline cannot survive). **Tests.** `lines_are_valid_json_with_escaped_detail` (audit.rs).

### `AuditEvent::to_json_line` (audit.rs) [@ANCHOR: relay_broker:relay_broker_audit_event_to_json_line]

**Purpose.** Formats one `AuditEvent` as the log's wire form: one JSON object per line, as `events.jsonl`. `AuditLog::record_at` writes it. The data structure is `AuditEvent { uid: u32 (`u32::MAX` when unknown), pid: i32, action: &'static str, outcome: &'static str, detail: String }`; `action` is one of `open_device`, `list_devices`, `grant_group`, `provision_groups`, `peer_rejected`, `malformed` (and the later actions), `outcome` one of `allowed`, `denied`, `dry_run`, `applied`, `rate_limited`, `failed`, `rejected`.

**Signature.** `pub fn to_json_line(&self, ts: u64) -> String`

| Argument | Type and range | Meaning |
|---|---|---|
| `ts` | `u64`, Unix seconds | The record's time, supplied by the caller. |

**Returns.** `{"ts":N,"uid":N,"pid":N,"action":"..","outcome":"..","detail":".."}` followed by `\n`; the three strings pass through `json_escape`. **Errors.** None. **Side effects.** None; pure. **Tests.** `lines_are_valid_json_with_escaped_detail` (audit.rs) parses the line back.

### `AuditLog::open` (audit.rs) [@ANCHOR: relay_broker:relay_broker_audit_log_open]

**Purpose.** Opens the append-only audit log at startup. `AuditLog` holds the optional file, its path, the current `size`, the rotation cap `max_bytes` (`MAX_LOG_BYTES`, 4 MiB) and a map from UID to the time of its last logged `peer_rejected`. The broker refuses to start unauditable, so an open failure is fatal.

**Signature.** `pub fn open(path: Option<&Path>) -> io::Result<AuditLog>`

| Argument | Type and range | Meaning |
|---|---|---|
| `path` | `Option<&Path>` | The log file; borrowed (copied into the struct). `None` disables logging to a file, which the tests use. |

**Returns.** The log, with `size` read from the existing file. **Errors.** Any `io::Error` from opening or reading the metadata (missing directory, permissions): returned to `main`, which exits. **Side effects.** Creates the file with mode 0600 if absent, opened with `O_APPEND` so existing lines are never rewritten; no rotation happens here. **Security.** Private mode, append-only. **Tests.** `records_append_to_a_private_file_and_none_is_a_sink` (audit.rs).

### `AuditLog::record` (audit.rs) [@ANCHOR: relay_broker:relay_broker_audit_log_record]

**Purpose.** Appends one event to the audit log, stamped with the current Unix time. Called by `Broker::log` for every request decided and by the server for every peer turned away. A write failure is reported but never stops the broker: once running, availability of the broker outranks the log.

**Signature.** `pub fn record(&mut self, event: &AuditEvent) -> io::Result<()>`

| Argument | Type and range | Meaning |
|---|---|---|
| `event` | `&AuditEvent` | The record, borrowed. |

**Returns.** `Ok(())` after the line is written, or when the throttle swallowed it or the log is a sink. **Errors.** `io::Error` from the write or from rotation; the caller (`Broker::log`) ignores it. **Side effects.** Reads the system clock, appends a line to the file, may rotate to `<path>.1`. A `peer_rejected` event is logged once per UID per minute, since a group member off the allow-list can connect as fast as it likes. **Concurrency.** `&mut self`; one writer. **Tests.** `records_append_to_a_private_file_and_none_is_a_sink` and `a_rejected_peer_is_logged_once_per_minute_and_the_log_rotates_at_the_cap` (audit.rs).

### `AllowList::permits` (auth.rs) [@ANCHOR: relay_broker:relay_broker_allow_list_permits]

**Purpose.** The broker's authentication decision for one connection. `AllowList { uids: Vec<u32>, gids: Vec<u32> }` is built from the root-owned config; the server asks `permits` on accept, from `SO_PEERCRED`, before any request byte is read, so an unknown peer never reaches the parser.

**Signature.** `pub fn permits(&self, peer: Peer) -> bool`

| Argument | Type and range | Meaning |
|---|---|---|
| `peer` | `Peer { uid, gid, pid }` | The kernel-reported credentials of the connecting process. Only `uid` and the primary `gid` are compared. |

**Returns.** `true` when the UID or the primary GID is listed. An empty list permits no one: a broker with no config answers nobody. **Errors.** None. **Side effects.** None; pure. **Security.** Only the primary GID counts, not supplementary groups. **Tests.** `only_listed_uids_or_gids_pass_and_an_empty_list_passes_nobody` (auth.rs).

- [@ANCHOR: relay_broker:relay_broker_broker_new] (broker.rs) Assembles a broker from a config, an audit log and an OS layer.
- [@ANCHOR: relay_broker:relay_broker_broker_os] (broker.rs) The OS layer (the server uses it for peer credentials).
- [@ANCHOR: relay_broker:relay_broker_broker_log] (broker.rs) Writes one audit record; a failed write is ignored here (the log is best effort once
- [@ANCHOR: relay_broker:relay_broker_broker_handle] (broker.rs) Decides and performs one request for an already-authenticated peer. Returns the response
- [@ANCHOR: relay_broker:relay_broker_broker_open_device] (broker.rs) The open action: lexical check, node walk, open, post-open check. Any failure closes the
- [@ANCHOR: relay_broker:relay_broker_broker_grant] (broker.rs) The provisioning action: which of `groups` the caller lacks, applied only when the
### `call` (client.rs) [@ANCHOR: relay_broker:relay_broker_client_call]

**Purpose.** The request-response exchange on an already verified stream: sends `req` and waits for the answer. The descriptor, if any, is the opened device of an `Opened` response; descriptors passed with any other response are dropped.

**Signature.** `pub fn call(stream: &UnixStream, req: &Request) -> io::Result<(Response, Option<OwnedFd>)>`

| Argument | Type and range | Meaning |
|---|---|---|
| `stream` | `&UnixStream` | A connected, verified socket; borrowed. |
| `req` | `&Request` | The request; encoded by `Request::encode`. |

**Returns.** The decoded `Response` and the passed descriptor. **Errors.** Any `io::Error` from the framing (`send_frame`, `recv_frame`), and `InvalidData` carrying the `WireError` when the body does not decode. **Side effects.** Sets the socket timeouts (`IO_TIMEOUT`, failure ignored), so it blocks no longer than that per read or write. **Tests.** `an_allowed_open_passes_a_working_descriptor_and_never_the_path` (tests/broker.rs).

### `DEFAULT_SERVER_UIDS` (client.rs) [@ANCHOR: relay_broker:relay_broker_client_default_server_uids]

**Definition.** `pub const DEFAULT_SERVER_UIDS: &[u32] = &[0];` the accounts a broker socket may be served by when the caller names none: root only. Used by `connect_and_call`; read-only constant, no runtime state.

**Why root.** A socket-activated broker has its listener made by systemd, so a client reads root's credentials from `SO_PEERCRED` even though the service runs as `hams-relay-broker`. A broker started directly by a service account must be named to `connect_and_call_as`. **Security.** The list is the client's trust anchor: a process that took over the socket path under another uid is refused by `verify_server`. **Tests.** `the_client_refuses_a_server_that_is_not_an_expected_account_and_sends_it_nothing` (tests/broker.rs). **Related.** `verify_server`, `connect_and_call`.

### `verify_server` (client.rs) [@ANCHOR: relay_broker:relay_broker_client_verify_server]

**Purpose.** The client authenticating the broker (the broker authenticating the client is `auth.rs`). Run after `connect` and before anything is sent.

**Signature.** `pub fn verify_server(stream: &UnixStream, expected_uids: &[u32]) -> io::Result<()>`

| Argument | Type and range | Meaning |
|---|---|---|
| `stream` | `&UnixStream` | The connected socket; borrowed. |
| `expected_uids` | `&[u32]` | The uids accepted as the server. An empty list permits no one. |

**Returns.** `Ok(())` when the server's `SO_PEERCRED` uid is in the list. **Errors.** `Err(PermissionDenied)` naming the uid found; any other `io::Error` is the failure to read the credentials. **Side effects.** None; blocking in theory but a `getsockopt` returns at once. Nothing has been sent when it runs, so an impostor learns nothing. **Tests.** `the_client_refuses_a_server_that_is_not_an_expected_account_and_sends_it_nothing` (tests/broker.rs).

### `connect_and_call` (client.rs) [@ANCHOR: relay_broker:relay_broker_client_connect_and_call]

**Purpose.** The default client entry point: `connect_and_call_as` with `DEFAULT_SERVER_UIDS` (root), for the usual socket-activated broker.

**Signature.** `pub fn connect_and_call(path: &Path, req: &Request) -> io::Result<(Response, Option<OwnedFd>)>`

| Argument | Type and range | Meaning |
|---|---|---|
| `path` | `&Path` | The broker's Unix socket path. |
| `req` | `&Request` | The typed request to send. |

**Returns.** The decoded `Response` and, for an `Opened` response, the passed descriptor (`Some(OwnedFd)` owned by the caller, who must close or use it). **Errors.** Connect failure, `PermissionDenied` from the server check, then those of `call` (timeouts, `InvalidData` for a malformed response). **Concurrency.** Blocking; one connection per call, closed on return. **Security.** The request is sent only after the server's uid is verified. **Tests.** `the_client_refuses_a_server_that_is_not_an_expected_account_and_sends_it_nothing` (tests/broker.rs).

### `connect_and_call_as` (client.rs) [@ANCHOR: relay_broker:relay_broker_client_connect_and_call_as]

**Purpose.** Connects to the broker, runs `verify_server`, and only then sends `req` and returns the answer (`call`). A wrong server is refused with `PermissionDenied` and receives no byte, so an impostor on the socket path cannot answer `ListDevices`, `ProvisionGroups` or `OpenDevice` with a descriptor of its choice.

**Signature.** `pub fn connect_and_call_as(path: &Path, req: &Request, expected_uids: &[u32]) -> io::Result<(Response, Option<OwnedFd>)>`

| Argument | Type and range | Meaning |
|---|---|---|
| `path` | `&Path` | The broker socket. |
| `req` | `&Request` | The request to send. |
| `expected_uids` | `&[u32]` | Accounts accepted as the server (root, or the service account of a broker started directly). |

**Returns.** `(Response, Option<OwnedFd>)`; the descriptor is present only for an `Opened` response and is owned by the caller. **Errors.** Connect failure, `PermissionDenied`, `InvalidData`, timeouts. **Concurrency.** Blocking, one connection per call. **Tests.** `the_accept_loop_serves_a_real_listener_filters_protected_devices_and_exits_when_idle` and `the_client_refuses_a_server_that_is_not_an_expected_account_and_sends_it_nothing` (tests/broker.rs).

- [@ANCHOR: relay_broker:relay_broker_parse_usb_pair] (config.rs) Parses `vvvv:pppp` (hex) into a vendor/product pair.
- [@ANCHOR: relay_broker:relay_broker_config_parse] (config.rs) Parses the file text. Comments start with `#`; blank lines are skipped.
- [@ANCHOR: relay_broker:relay_broker_config_load] (config.rs) Reads and parses `path`, first requiring it to be owned by `owner_uid` (0 in production)
- [@ANCHOR: relay_broker:relay_broker_main_parse_args] (cli.rs) Parses argv; unknown options are an error.
- [@ANCHOR: relay_broker:relay_broker_remove_stale_socket] (cli.rs) Removes a leftover socket at the `--socket` path from an earlier run; any other kind of file there is an error and is left alone. The listener is then bound and set to mode 0660.
- [@ANCHOR: relay_broker:relay_broker_main] (cli.rs) Entry point.
- [@ANCHOR: relay_broker:relay_broker_kind_of_mode] (os.rs) Maps `st_mode`/`st_rdev` to a `NodeKind`.
- [@ANCHOR: relay_broker:relay_broker_read_hex_u16] (os.rs) Reads a sysfs attribute holding a hexadecimal id (`idVendor`, `idProduct`).
- [@ANCHOR: relay_broker:relay_broker_usb_identity_from_sysfs] (os.rs) Walks up from a sysfs device directory (at most eight levels) to the USB device that holds
- [@ANCHOR: relay_broker:relay_broker_linux_sysfs_dir_for] (os.rs) The sysfs class directory of a device node: `/dev/ttyUSB0` is `class/tty/ttyUSB0`,
- [@ANCHOR: relay_broker:relay_broker_linux_scan_class] (os.rs) Lists one sysfs class directory. `keep` decides by entry name; `dev_dir` is where the
- [@ANCHOR: relay_broker:relay_broker_linux_peer_credentials] (os.rs) fn peer_credentials(&self, socket: BorrowedFd<'_>) -> io::Result<Peer> {
- [@ANCHOR: relay_broker:relay_broker_linux_update_package] (os.rs) fn update_package(&self) -> io::Result<()> { // creates the request file the package's root-owned path unit acts on; runs no program
- [@ANCHOR: relay_broker:relay_broker_linux_node_kind] (os.rs) fn node_kind(&self, path: &Path) -> NodeKind {
- [@ANCHOR: relay_broker:relay_broker_linux_open_device] (os.rs) fn open_device(&self, path: &Path, read_only: bool) -> io::Result<OwnedFd> {
- [@ANCHOR: relay_broker:relay_broker_linux_fd_kind] (os.rs) fn fd_kind(&self, fd: BorrowedFd<'_>) -> NodeKind {
- [@ANCHOR: relay_broker:relay_broker_linux_usb_identity] (os.rs) fn usb_identity(&self, path: &Path) -> Option<(u16, u16, String)> {
- [@ANCHOR: relay_broker:relay_broker_linux_list_devices] (os.rs) fn list_devices(&self) -> Vec<DeviceDescriptor> {
- [@ANCHOR: relay_broker:relay_broker_linux_username] (os.rs) fn username(&self, uid: u32) -> Option<String> {
- [@ANCHOR: relay_broker:relay_broker_linux_groups_of] (os.rs) fn groups_of(&self, user: &str) -> Vec<String> {
- [@ANCHOR: relay_broker:relay_broker_linux_add_to_group] (os.rs) fn add_to_group(&self, user: &str, group: &str) -> io::Result<()> {
- [@ANCHOR: relay_broker:relay_broker_denial_reason] (policy.rs) A stable short reason for the audit log and the response.
- [@ANCHOR: relay_broker:relay_broker_policy_check_path] (policy.rs) The lexical check. Returns the normalised path on success. No filesystem access.
- [@ANCHOR: relay_broker:relay_broker_policy_class_of_major] (policy.rs) The class a character-device major belongs to under this policy.
- [@ANCHOR: relay_broker:relay_broker_policy_check_kind] (policy.rs) Accepts a node kind only when it is a character device of `class`.
- [@ANCHOR: relay_broker:relay_broker_policy_check_node] (policy.rs) The filesystem check: every directory on the way from the device root is a real
- [@ANCHOR: relay_broker:relay_broker_policy_check_fd] (policy.rs) The post-open check on the descriptor itself.
- [@ANCHOR: relay_broker:relay_broker_rate_limiter_new] (ratelimit.rs) `per_minute` requests sustained, with a burst of the same size. Zero means one a minute.
- [@ANCHOR: relay_broker:relay_broker_rate_limiter_allow] (ratelimit.rs) Takes one token for `uid` at time `now`; false when the bucket is empty. At most 256
- [@ANCHOR: relay_broker:relay_broker_serve_connection] (server.rs) Serves one connection to completion. Never panics; every failure is an `Outcome`.
- [@ANCHOR: relay_broker:relay_broker_run] (server.rs) Accepts connections until `idle` passes with none, or until `max_connections` have been
- [@ANCHOR: relay_broker:relay_broker_activated_listener] (server.rs) The listener systemd passed (`LISTEN_PID` is this process and `LISTEN_FDS` is 1), if any.
- [@ANCHOR: relay_broker:relay_broker_set_timeouts] (transport.rs) Applies `IO_TIMEOUT` as the receive and send timeout of a socket.
- [@ANCHOR: relay_broker:relay_broker_send_frame] (transport.rs) Writes `body` as one frame, passing `fd` (if any) with the first bytes.
- [@ANCHOR: relay_broker:relay_broker_recv_exact] (transport.rs) Fills `buf` with `recvmsg`, appending any passed descriptors to `fds`. Descriptors beyond
- [@ANCHOR: relay_broker:relay_broker_adopt_received_fd] (transport.rs) Wraps a descriptor that `recvmsg` just received as SCM_RIGHTS. This is the crate's only
- [@ANCHOR: relay_broker:relay_broker_recv_frame] (transport.rs) Reads one frame: the body bytes and any descriptors passed with it. The announced length is
- [@ANCHOR: relay_broker:relay_broker_group_name] (wire.rs) The system group name.
- [@ANCHOR: relay_broker:relay_broker_reader_take] (wire.rs) The next `n` bytes, or `Truncated`.
- [@ANCHOR: relay_broker:relay_broker_reader_u8] (wire.rs) One byte.
- [@ANCHOR: relay_broker:relay_broker_reader_u16] (wire.rs) A big-endian u16.
- [@ANCHOR: relay_broker:relay_broker_reader_string] (wire.rs) A length-prefixed string of at most `MAX_STRING` bytes, UTF-8, no NUL.
- [@ANCHOR: relay_broker:relay_broker_reader_finish] (wire.rs) Succeeds only when every byte was consumed.
- [@ANCHOR: relay_broker:relay_broker_put_string] (wire.rs) Appends a length-prefixed string, truncating at a character boundary to `MAX_STRING` bytes
- [@ANCHOR: relay_broker:relay_broker_device_class_from_tag] (wire.rs) The class for a wire tag.
- [@ANCHOR: relay_broker:relay_broker_request_encode] (wire.rs) The body bytes (no frame header) for this request.
- [@ANCHOR: relay_broker:relay_broker_request_decode] (wire.rs) Parses a body. Every failure is a `WireError`; no input panics.
- [@ANCHOR: relay_broker:relay_broker_response_error] (wire.rs) Shorthand for an error response.
- [@ANCHOR: relay_broker:relay_broker_response_encode] (wire.rs) The body bytes (no frame header). Device lists are cut at `MAX_DEVICES`.
- [@ANCHOR: relay_broker:relay_broker_response_decode] (wire.rs) Parses a response body. Used by the client and by the fuzz/property tests.
- [@ANCHOR: relay_broker:relay_broker_frame_encode] (wire.rs) Prefixes a body with its length. Bodies above `MAX_FRAME` are an encoder bug and are cut to
- [@ANCHOR: relay_broker:relay_broker_frame_body_len] (wire.rs) The body length announced by a 4-byte header, if it is within bounds.
- [@ANCHOR: relay_broker:relay_broker_reader_bool] (wire.rs) `Reader::bool`: reads one byte and returns `false` for 0 and `true` for 1. Any other byte is `WireError::BadTag(byte)`, and an empty buffer is `Truncated`, so a flag field cannot carry a third value past the parser. Used for the 5 GHz flag, the optional-DNS flag and the hub and breaker status flags.
- [@ANCHOR: relay_broker:relay_broker_reader_ipv4] (wire.rs) `Reader::ipv4`: takes four bytes and returns them as an `Ipv4Addr` in network order. Fewer than four bytes left is `WireError::Truncated`; the reader position only advances on success. Used for the hub address and the optional uplink DNS address in a hub start request.
- [@ANCHOR: relay_broker:relay_broker_reader_u32] (wire.rs) `Reader::u32`: takes four bytes and returns a big-endian u32, or `WireError::Truncated` when fewer than four remain. Used for the lease length in minutes of a hub start request. It never panics and never reads past the end of the body.
### `TimeDenial::reason` (time_guard.rs) [@ANCHOR: relay_broker:relay_broker_time_denial_reason]

**Purpose.** Gives the fixed text for each reason a clock step is refused; the same text goes to the relay in the `Denied` response and into the audit log `detail`, so the two always agree. `TimeDenial` is a small closed enum (`BeforeBuildDate`, `UnprovenLargeStep`, `BreakerOpen`), `Copy`, with no data.

**Signature.** `pub fn reason(self) -> &'static str`

| Argument | Type and range | Meaning |
|---|---|---|
| `self` | `TimeDenial` | The reason to describe, by value. |

**Returns.** One of three static strings: "target is before the build date", "large step needs a GPS lock or two agreeing sources", "too many clock steps: circuit breaker open". **Errors.** None. **Side effects.** None; pure. **Security.** The strings carry no input, so nothing the relay sent is reflected into the log. **Tests.** `the_ceilings_hold_and_the_breaker_trips_and_resets` (time_guard.rs) checks that a reason is non-empty.

### `TimeGuard::expire` (time_guard.rs) [@ANCHOR: relay_broker:relay_broker_time_guard_expire]

**Purpose.** Private housekeeping for `check` and `status`: forgets recorded steps that are an hour old or older and closes the circuit breaker once its deadline has passed, so the one-hour window slides and the breaker heals without any timer thread.

**Signature.** `fn expire(&mut self, now: Instant)` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `now` | `Instant` | The reference time; steps with `now - t >= 3600 s` are popped from the front of the queue (it is in time order) and `breaker_until` is cleared when `now >= breaker_until`. |

**Returns.** Nothing. **Errors.** None; `Instant` arithmetic here cannot underflow because steps are recorded in time order. **Side effects.** Mutates `steps` and `breaker_until` only. **Tests.** Reached by `the_ceilings_hold_and_the_breaker_trips_and_resets` (time_guard.rs): after one hour the steps and the breaker are gone.

### `AuditLog::record_at` (audit.rs) [@ANCHOR: relay_broker:relay_broker_audit_log_record_at]

**Purpose.** The body of `record` with the timestamp supplied, so the once-a-minute rejection rule and the rotation can be tested without a clock. Private.

**Signature.** `fn record_at(&mut self, event: &AuditEvent, ts: u64) -> io::Result<()>`

| Argument | Type and range | Meaning |
|---|---|---|
| `event` | `&AuditEvent` | The record. |
| `ts` | `u64`, Unix seconds | Its time. |

**Behaviour.** A `peer_rejected` event from a UID already logged within `REJECTION_LOG_SECS` (60) returns `Ok(())` without writing; the remembered-UID map is cleared at 1024 entries, which only costs a few extra lines. Otherwise it formats the line, calls `rotate` first if the line would push the file past `max_bytes`, writes it with `write_all` and adds its length to `size`. **Errors.** `io::Error` from rotation or the write. **Side effects.** File append; throttle map update. **Tests.** `a_rejected_peer_is_logged_once_per_minute_and_the_log_rotates_at_the_cap` (audit.rs).

### `AuditLog::rotate` (audit.rs) [@ANCHOR: relay_broker:relay_broker_audit_log_rotate]

**Purpose.** Keeps the log directory bounded: the log is moved to `<path>.1` (replacing an older one) when it reaches 4 MiB, so the directory never holds more than about twice that. Private, called from `record_at`.

**Signature.** `fn rotate(&mut self) -> io::Result<()>`

**Arguments.** None besides `&mut self`; it uses the stored `path`. **Returns.** `Ok(())`; a log without a path (the sink) does nothing. **Errors.** `io::Error` from the `rename` or from creating the new file. **Side effects.** `rename(path, path.1)` (atomic, replaces an older `.1`), then opens a fresh empty file at `path` with `O_APPEND` and mode 0600 and resets `size` to 0. Only one generation is kept, so history older than about two files is lost. **Tests.** `a_rejected_peer_is_logged_once_per_minute_and_the_log_rotates_at_the_cap` (audit.rs).

- [@ANCHOR: relay_broker:relay_broker_hub_validate] (hub.rs) `hub::validate(settings)`: checks the hub settings and names the one that is wrong. The interface must be a plain name of at most 15 characters, the SSID 1 to 32 printable ASCII characters, the passphrase 8 to 63 printable ASCII characters, the channel valid for the band, the address private, the relay name one DNS label, the lease 1 minute to 24 hours. Nothing that could add a line to a config file passes.
- [@ANCHOR: relay_broker:relay_broker_hub_dnsmasq_conf] (hub.rs) `hub::dnsmasq_conf(settings)`: validates, then returns the dnsmasq configuration text for the hub /24: DHCP pool .20 to .250, the hub as DNS (option 6) and NTP (option 42) server, the relay name and the phone connectivity-probe names answering with the hub, and other names forwarded to the uplink resolver or refused. Error is the validation message.
- [@ANCHOR: relay_broker:relay_broker_hub_takeover_dnsmasq_conf] (hub.rs) `hub::takeover_dnsmasq_conf(takeover)`: the dnsmasq text for taking over DHCP on an existing network: the same /24 from a high pool (.200 to .250), five-minute leases, the router as gateway, the hub as DNS and NTP server. A gateway off the subnet or a hub address inside the pool is an error string.
- [@ANCHOR: relay_broker:relay_broker_hub_runner_write_private] (hub_runner.rs) `write_private(path, text)`: creates or truncates a file with mode 0600 and writes the text, so the generated hostapd file (which holds the Wi-Fi passphrase) is readable by root only. Errors are the open or write failure.
- [@ANCHOR: relay_broker:relay_broker_hub_runner_run] (hub_runner.rs) `HubRunner::run(program, args)`: runs one helper program (ip, hostapd or dnsmasq) to completion with a cleared environment and a fixed PATH. Success is exit status 0; any other status is an error naming the program and the status. A program that cannot start returns its spawn error.
- [@ANCHOR: relay_broker:relay_broker_hub_runner_start] (hub_runner.rs) `HubRunner::start(hostapd_conf, dnsmasq_conf, interface, address)`: stops any earlier hub, creates the 0700 state directory, writes both configs and the interface name privately, gives the interface its /24 address, brings it up and starts hostapd then dnsmasq. Any failure stops what was started and returns the error.
- [@ANCHOR: relay_broker:relay_broker_hub_runner_stop] (hub_runner.rs) `HubRunner::stop`: sends SIGTERM to hostapd and dnsmasq only when the pid file names a live process of that exact name, flushes the interface address, and removes the config, pid and interface files. Stopping a hub that is not running succeeds.
- [@ANCHOR: relay_broker:relay_broker_broker_update_package] (broker.rs) `Broker::update_package(peer)`: the update action. Denied (and audited) unless `update_package = true` in the config; when enabled it asks the OS layer to leave the root-owned path unit its request file and answers `Updated`. A failure is audited and returned as `Failed`. The caller cannot supply any command.
- [@ANCHOR: relay_broker:relay_broker_broker_bind_udp] (broker.rs) `Broker::bind_udp(peer, port)`: binds a UDP socket on a port the config lists (default 123 only) and passes the descriptor, so the unprivileged relay never holds the privilege. An unlisted port is `Denied`, a bind error `Failed`; each outcome is audited.
- [@ANCHOR: relay_broker:relay_broker_broker_hub_start] (broker.rs) `Broker::hub_start(peer, settings)`: the hub start action. Denied unless the hub is enabled in the config; validates the settings and builds both configs from the fixed templates (a bad setting is `Denied` and never reaches the OS), then starts the hub and answers with its status. Every outcome is audited.
- [@ANCHOR: relay_broker:relay_broker_broker_hub_stop] (broker.rs) `Broker::hub_stop(peer)`: the hub stop action. Denied unless the hub is enabled; otherwise stops both programs through the OS layer and answers with the resulting hub status. A failure is audited and returned as `Failed`.
- [@ANCHOR: relay_broker:relay_broker_linux_bind_udp] (os.rs) `LinuxOs::bind_udp(port)`: opens a UDP socket (dual stack IPv6 with v6only off when the kernel has IPv6, plain IPv4 otherwise), close-on-exec, binds every address on `port` and returns the descriptor. An address already in use or a refused port is the returned `io::Error`.
