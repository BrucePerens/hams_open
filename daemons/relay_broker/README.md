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

### `Broker::set_system_time` (broker.rs) [@ANCHOR: relay_broker:relay_broker_broker_set_system_time]

**Purpose.** Steps the clock when the config enables it (`set_time = true`) and the ceilings in `time_guard.rs` allow it. Every outcome is audited with the clock before, the target, the source and the evidence. The relay uses it to set time on a machine without Internet.

**Signature.** `fn set_system_time(&mut self, peer: Peer, now: Instant, target: u64, source: TimeSource, agreeing: u8) -> (Response, Option<OwnedFd>)` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `peer` | `Peer` | Caller, audit only. |
| `now` | `Instant` | Monotonic time for the guard. |
| `target` | `u64`, Unix seconds | The time to set. |
| `source` | `TimeSource` | The relay's claim of origin (`Gps` is proof alone). |
| `agreeing` | `u8` | Count of agreeing independent sources. |

**Returns.** `TimeSet { before_unix_secs, after_unix_secs }`. **Errors.** `Denied` ("clock steps not enabled", or the `TimeDenial::reason`), `Failed` when `clock_settime` fails (needs `CAP_SYS_TIME`). **Side effects.** Changes `CLOCK_REALTIME`; records the step in the guard only after success. **Security.** The broker applies its own limits and does not trust the relay. **Tests.** `the_clock_is_stepped_only_when_enabled_and_inside_the_ceilings` (tests/broker.rs).

### `Broker::time_status` (broker.rs) [@ANCHOR: relay_broker:relay_broker_broker_time_status]

**Purpose.** Tells the relay the broker's clock and its step limits, so it can decide whether a step is worth attempting. Read-only and always served (it is not behind `set_time`), but still rate-limited and audited.

**Signature.** `fn time_status(&mut self, peer: Peer, now: Instant) -> (Response, Option<OwnedFd>)` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `peer` | `Peer` | Caller, audit only. |
| `now` | `Instant` | Passed to `TimeGuard::status`. |

**Returns.** `Response::TimeStatus { now_unix_secs, steps_last_hour, breaker_open, min_target_unix_secs }`, the last being `BUILD_DATE_UNIX_SECS`. **Errors.** None. **Side effects.** Reads the clock; expires guard state; one audit line. **Tests.** `the_clock_is_stepped_only_when_enabled_and_inside_the_ceilings` (tests/broker.rs).

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

### `Broker::new` (broker.rs) [@ANCHOR: relay_broker:relay_broker_broker_new]

**Purpose.** Assembles the one `Broker` value that every connection shares: the allow-list, the open policy, the feature switches, the clock guard, the rate limiter, the audit log and the OS layer. `Broker` is the data structure that holds all broker state: public fields `allow: AllowList`, `policy: Policy`, `apply_groups`, `bind_udp_ports: Vec<u16>`, `hub_enabled`, `update_package`, `set_time` (all switches default off except the time port list), and private `time_guard`, `rate`, `audit`, `os: Box<dyn Os + Send>`.

**Signature.** `pub fn new(config: &Config, audit: AuditLog, os: Box<dyn Os + Send>) -> Broker`

| Argument | Type and range | Meaning |
|---|---|---|
| `config` | `&Config` | The parsed config; borrowed, its lists are cloned. |
| `audit` | `AuditLog` | The opened log; moved in and owned by the broker. |
| `os` | `Box<dyn Os + Send>` | The OS layer: `LinuxOs` in production, a fake in tests (the only test seam). Owned. |

**Returns.** The broker. **Errors.** None. **Side effects.** None; builds `TimeGuard::new(max_unproven_step_secs, max_time_steps_per_hour)` and `RateLimiter::new(rate_per_minute)`. **Concurrency.** The value is used from one thread, one connection at a time. **Tests.** `an_unlisted_peer_is_closed_before_any_byte_is_read` and the other builders in tests/broker.rs.

### `Broker::os` (broker.rs) [@ANCHOR: relay_broker:relay_broker_broker_os]

**Purpose.** Read access to the broker's OS layer. The server uses it to read peer credentials of an accepted socket (`peer_credentials`) before the allow-list is consulted, so authentication goes through the same seam as every other system effect.

**Signature.** `pub fn os(&self) -> &dyn Os`

**Arguments.** None besides `&self`. **Returns.** A shared borrow of the `Os` trait object, valid while the broker is borrowed. **Errors.** None. **Side effects.** None; callers can only call the `&self` methods of the trait. **Concurrency.** No locking. **Tests.** `an_unlisted_peer_is_closed_before_any_byte_is_read` (tests/broker.rs) goes through it.

### `Broker::log` (broker.rs) [@ANCHOR: relay_broker:relay_broker_broker_log]

**Purpose.** Writes one audit record for a peer. Every action method in `broker.rs` calls it for every outcome, allowed or refused, so the log is the complete history of what the broker decided.

**Signature.** `pub fn log(&mut self, peer: Peer, action: &'static str, outcome: &'static str, detail: String)`

| Argument | Type and range | Meaning |
|---|---|---|
| `peer` | `Peer` | Kernel credentials; `uid` and `pid` go into the record. |
| `action` | `&'static str` | Action name such as `open_device`, `bind_udp`, `set_system_time`. |
| `outcome` | `&'static str` | `allowed`, `denied`, `failed`, `dry_run`, `applied`, `rate_limited`. |
| `detail` | `String` | Free text (path, reason); moved in, escaped on output. |

**Returns.** Nothing. **Errors.** A failed write is ignored here: the log is best effort once it has been opened (opening it is mandatory, `AuditLog::open`). **Side effects.** Appends to `events.jsonl` through `AuditLog::record`. **Tests.** `provisioning_reports_by_default_and_applies_only_when_configured` (tests/broker.rs) reads the lines back.

### `Broker::handle` (broker.rs) [@ANCHOR: relay_broker:relay_broker_broker_handle]

**Purpose.** The one function that turns an authenticated, parsed `Request` into a `Response`. There is no string anywhere that becomes a command: it rate-limits the peer, dispatches on the request variant to the action method, and every path audits. The server calls it once per connection, after `AllowList::permits` and the wire decode.

**Signature.** `pub fn handle(&mut self, peer: Peer, req: &Request, now: Instant) -> (Response, Option<OwnedFd>)`

| Argument | Type and range | Meaning |
|---|---|---|
| `peer` | `Peer` | Authenticated credentials of the caller. |
| `req` | `&Request` | One of `ListDevices`, `OpenDevice`, `GrantGroup`, `ProvisionGroups`, `BindUdp`, `HubStart`, `HubStop`, `HubStatus`, `UpdatePackage`, `SetSystemTime`, `TimeStatus`. |
| `now` | `Instant` | Monotonic time, used for the rate limit and the clock guard. |

**Returns.** The response and, for a successful open or bind, the descriptor to pass (owned; the server sends it with SCM_RIGHTS and drops it). `ListDevices` filters by class and removes any device whose USB id is on `deny_usb`. **Errors.** Reported as `Response::Error` with a `Status` (`RateLimited`, `Denied`, `Failed`); nothing panics. **Side effects.** Audit record per request; the action's own effects. **Tests.** `policy_refuses_disks_protected_devices_traversal_and_symlink_games` and `a_peer_over_its_rate_is_refused_and_logged` (tests/broker.rs).

### `Broker::open_device` (broker.rs) [@ANCHOR: relay_broker:relay_broker_broker_open_device]

**Purpose.** The open action, the broker's core service: it opens one device node for the relay so the worker never needs `/dev` access and never learns another device's path. Order: lexical check (`Policy::check_path`), node walk (`check_node`), open, post-open descriptor check (`check_fd`).

**Signature.** `fn open_device(&mut self, peer: Peer, class: DeviceClass, read_only: bool, path: &str) -> (Response, Option<OwnedFd>)` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `peer` | `Peer` | Caller, for the audit record. |
| `class` | `DeviceClass` | `Serial` or `Audio`; the node's major must belong to it. |
| `read_only` | `bool` | Open read-only when true, read-write otherwise. |
| `path` | `&str` | The requested device path; peer-supplied, untrusted. |

**Returns.** `(Response::Opened, Some(fd))` on success; otherwise `Denied` (policy refusal, with the stable reason) or `Failed` (the open failed) and no descriptor. Any failure closes the descriptor by drop. **Side effects.** Opens the node `O_NOFOLLOW`, `O_CLOEXEC`, `O_NOCTTY`; audit record per outcome. **Security.** The post-open check re-examines the descriptor itself, closing the race between walk and open. **Tests.** `policy_refuses_disks_protected_devices_traversal_and_symlink_games` and `an_unreadable_usb_identity_is_refused_unless_the_config_allows_it` (tests/broker.rs).

### `Broker::grant` (broker.rs) [@ANCHOR: relay_broker:relay_broker_broker_grant]

**Purpose.** Serves `grant_group` (one group) and `provision_groups` (`dialout` and `audio`): reports which of those groups the caller's own account lacks and, only when `apply_groups = true`, adds the account to them. The account is the peer's own UID; a peer can never name another user.

**Signature.** `fn grant(&mut self, peer: Peer, action: &'static str, groups: &[Group]) -> (Response, Option<OwnedFd>)` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `peer` | `Peer` | The account that is looked up (`username(uid)`). |
| `action` | `&'static str` | `grant_group` or `provision_groups`, for the audit record. |
| `groups` | `&[Group]` | The groups asked about; `Group` is a closed enum, so no arbitrary group name exists. |

**Returns.** `Response::GroupReport { applied, user, groups }`: `applied` is true only when the change was made, `groups` lists those that were (or would be) added, empty when nothing is missing; no descriptor. **Errors.** `Failed` for "unknown uid" or when `add_to_group` fails (it stops at the first failure). **Side effects.** With `apply_groups` set, runs `gpasswd -a USER GROUP` with a fixed argv through the OS layer; audit outcome `allowed` (nothing missing), `applied`, or `dry_run`. **Tests.** `provisioning_reports_by_default_and_applies_only_when_configured` (tests/broker.rs).

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

### `parse_usb_pair` (config.rs) [@ANCHOR: relay_broker:relay_broker_parse_usb_pair]

**Purpose.** Parses `vvvv:pppp` (hex) into a vendor/product pair for the `deny_usb` config key. Private helper of `Config::parse`.

**Signature.** `fn parse_usb_pair(s: &str) -> Option<(u16, u16)>`

| Argument | Type and range | Meaning |
|---|---|---|
| `s` | `&str` | Text such as `08bb:29c0`; split on the first `:`, each half parsed as base-16 `u16` (so no `0x` prefix). |

**Returns.** `Some((vendor, product))`, or `None` for a missing colon or a non-hex or out-of-range half; the caller turns `None` into the config error "expected vvvv:pppp". **Side effects.** None; pure. **Tests.** `keys_are_closed_and_lists_replace_defaults` (config.rs).

### `Config::parse` (config.rs) [@ANCHOR: relay_broker:relay_broker_config_parse]

**Purpose.** Parses the config file text into a `Config`. Comments start with `#`; blank lines are skipped; every other line is `key = value`. The schema is closed: an unknown key is an error, so a typo cannot silently weaken a control. `ConfigError { line: usize (1-based, 0 for file-level), message: String }` is the error type.

**Signature.** `pub fn parse(text: &str) -> Result<Config, ConfigError>`

| Key | Default | Meaning |
|---|---|---|
| `allow_uid`, `allow_gid` | none | Repeatable; the allow-list (peer UID or primary GID). |
| `apply_groups`, `hub`, `update_package`, `set_time`, `allow_unreadable_usb` | false | `true` or `false` switches. |
| `max_unproven_step_secs`, `max_time_steps_per_hour` | 30, 4 | Clock ceilings. |
| `bind_udp_port` | 123 | Repeatable; the first line replaces the default list. |
| `audit_log` | `/var/log/hams-relay-broker/events.jsonl` | Log path. |
| `rate_per_minute`, `idle_exit_secs` | 60, 60 | Rate limit; idle exit. |
| `device_root` | `/dev` | Absolute path. |
| `serial_major`, `audio_major`, `deny_usb`, `deny_path` | built-in lists | Repeatable; the first line replaces the built-in list. |

**Returns.** The `Config`. **Errors.** `ConfigError` with the line for a missing `=`, a bad number, uid, port or major, a non-boolean, a relative `device_root`, a bad `vvvv:pppp`, or an unknown key. **Side effects.** None; pure. **Tests.** `keys_are_closed_and_lists_replace_defaults` (config.rs).

### `Config::load` (config.rs) [@ANCHOR: relay_broker:relay_broker_config_load]

**Purpose.** Reads and parses the config file after checking its ownership: first requiring it to be owned by `owner_uid` (0 in production) and writable by nobody else, because the config holds the allow-list and the policy. The owner is a parameter so the tests can load a file they own; the binary passes `--config-owner` (default 0).

**Signature.** `pub fn load(path: &Path, owner_uid: u32) -> Result<Config, ConfigError>`

| Argument | Type and range | Meaning |
|---|---|---|
| `path` | `&Path` | The config file, normally `/etc/hams-relay-broker.conf`. |
| `owner_uid` | `u32` | The uid that must own it. |

**Returns.** The parsed `Config`. **Errors.** `ConfigError` with line 0 for an unreadable file, a wrong owner, or a group- or world-writable mode (`mode & 0o022`); otherwise the errors of `Config::parse`. **Side effects.** Reads the file; the `metadata` check and the read are separate calls, so the check is not atomic with the read. **Tests.** `load_requires_the_expected_owner_and_private_mode` (config.rs).

### `parse_args` (cli.rs) [@ANCHOR: relay_broker:relay_broker_main_parse_args]

**Purpose.** Parses argv for `hams-relay-broker`; unknown options are an error. The private `Args { config: PathBuf (default `/etc/hams-relay-broker.conf`), socket: Option<PathBuf>, config_owner: u32 (default 0) }` is its result.

**Signature.** `fn parse_args(argv: &[String]) -> Result<Args, String>` (private)

| Option | Meaning |
|---|---|
| `--config PATH` | Config file. |
| `--socket PATH` | A path to bind when no socket is activated (development). |
| `--config-owner UID` | Uid that must own the config. |

**Returns.** The `Args`. **Errors.** `Err(text)` for an unknown option, a missing value ("--x needs a value") or a bad uid; `run_main` prints it and exits 2. The first element of `argv` is skipped as the program name. **Side effects.** None. **Tests.** `options_are_closed_and_defaults_point_at_etc` (cli.rs).

### `remove_stale_socket` (cli.rs) [@ANCHOR: relay_broker:relay_broker_remove_stale_socket]

**Purpose.** Removes a leftover socket at the `--socket` path from an earlier run, so a restart can bind; any other kind of file there is an error and is left alone, so a wrong path cannot delete a real file.

**Signature.** `fn remove_stale_socket(path: &Path) -> io::Result<()>` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `path` | `&Path` | The socket path; examined with `symlink_metadata`, so a symlink is not followed (and is refused). |

**Returns.** `Ok(())` after removing a socket, or when nothing exists. **Errors.** An `io::Error` "exists and is not a socket; not removing it" for anything else, or the error from the metadata or removal. **Side effects.** Deletes the socket file. **Tests.** `only_a_leftover_socket_is_removed` (cli.rs).

### `run_main` (cli.rs) [@ANCHOR: relay_broker:relay_broker_main]

**Purpose.** The program: `main.rs` calls it so the relay package can build the same binary. It parses argv, loads the config, opens the audit log and the listener (the systemd-activated fd 3, else binds `--socket` with mode 0660), builds the `Broker` with `LinuxOs` and serves until idle for `idle_exit_secs`.

**Signature.** `pub fn run_main() -> ExitCode`

**Arguments.** Read from the process argv (see `parse_args`). **Returns.** `ExitCode::SUCCESS` after an idle exit; exit 2 for a bad option, config, audit log or listener (message to stderr: `hams-relay-broker: ...`); `FAILURE` when the accept loop errors. **Concurrency.** Single-threaded, one connection at a time; blocking in `run`. **Side effects.** Creates the log and the socket; may delete a stale socket. **Security.** The broker refuses to start unauditable or with an unsafe config. **Tests.** `the_program_exits_2_on_bad_options_or_config_and_0_after_an_idle_run` (tests/cli.rs).

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
### `RateLimiter::new` (ratelimit.rs) [@ANCHOR: relay_broker:relay_broker_rate_limiter_new]

**Purpose.** Builds the per-UID token-bucket limiter: `per_minute` requests sustained, with a burst of the same size. Zero means one a minute. `RateLimiter` holds `capacity`, `refill_every` and a map uid to `(tokens, refill accounted up to, last request)`, so a worker looping on a refused open cannot turn the broker into a log flood or a CPU sink.

**Signature.** `pub fn new(per_minute: u32) -> RateLimiter`

| Argument | Type and range | Meaning |
|---|---|---|
| `per_minute` | `u32` | Sustained rate; capacity is `max(per_minute, 1)` and one token returns every `60 s / capacity`. Config default 60. |

**Returns.** An empty limiter. **Errors.** None. **Side effects.** None. **Tests.** `a_burst_is_allowed_then_refused_until_time_passes_per_uid` and `the_sustained_rate_keeps_the_fraction_and_a_huge_rate_does_not_wrap` (ratelimit.rs).

### `RateLimiter::allow` (ratelimit.rs) [@ANCHOR: relay_broker:relay_broker_rate_limiter_allow]

**Purpose.** Takes one token for `uid` at time `now`; false when the bucket is empty. Called first in `Broker::handle`.

**Signature.** `pub fn allow(&mut self, uid: u32, now: Instant) -> bool`

| Argument | Type and range | Meaning |
|---|---|---|
| `uid` | `u32` | The peer's UID; each UID has its own bucket, so one noisy peer does not starve another. |
| `now` | `Instant` | Monotonic time; refills are whole tokens, keeping the fractional remainder. |

**Returns.** `true` if a token was taken, `false` when empty. **Errors.** None; time going backwards is saturated. **Memory bound.** At most 256 peers are tracked; beyond that the peer that asked longest ago is dropped (not the one refilled longest ago). **Tests.** `a_burst_is_allowed_then_refused_until_time_passes_per_uid`, `the_peer_evicted_is_the_one_that_asked_longest_ago` (ratelimit.rs); `a_peer_over_its_rate_is_refused_and_logged` (tests/broker.rs).

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
### `Broker::update_package` (broker.rs) [@ANCHOR: relay_broker:relay_broker_broker_update_package]

**Purpose.** The package update action: it asks for the relay package upgrade, which the OS layer turns into a request file for the package's root-owned path unit. The command is one fixed command (`apt-get install --only-upgrade hams-local-relay`, or the `dnf` equivalent) that the caller cannot add to or change, so a verified-signature repository the owner configured is the only source of what installs.

**Signature.** `fn update_package(&mut self, peer: Peer) -> (Response, Option<OwnedFd>)` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `peer` | `Peer` | Caller, audit only; the request carries no parameters. |

**Returns.** `Response::Updated`. **Errors.** `Denied` "updates not enabled" unless `update_package = true`; `Failed` when the request file cannot be created. **Side effects.** `Os::update_package` (see `LinuxOs::update_package`); audited. **Tests.** `update_package_runs_only_when_the_config_enables_it` (tests/broker.rs).

### `Broker::bind_udp` (broker.rs) [@ANCHOR: relay_broker:relay_broker_broker_bind_udp]

**Purpose.** The bind action: only a port the config lists (default 123, the time service). The broker binds the UDP socket and passes the descriptor, so the relay never needs a privilege to hold a privileged port.

**Signature.** `fn bind_udp(&mut self, peer: Peer, port: u16) -> (Response, Option<OwnedFd>)` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `peer` | `Peer` | Caller, audit only. |
| `port` | `u16` | Requested port; must be in `bind_udp_ports`. |

**Returns.** `(Response::Opened, Some(fd))`, the descriptor owned by the caller. **Errors.** `Denied` "port not allowed"; `Failed` when the bind fails (port in use, no privilege). **Side effects.** Binds a socket in the broker's process via `Os::bind_udp`; the descriptor is closed in the broker after sending. **Tests.** `bind_udp_serves_only_listed_ports_and_passes_a_descriptor` (tests/broker.rs).

### `Broker::hub_start` (broker.rs) [@ANCHOR: relay_broker:relay_broker_broker_hub_start]

**Purpose.** The hub start action: the stand-alone emergency hub. Settings are validated, both config files (`hostapd.conf`, `dnsmasq.conf`) are produced from the fixed templates in `hub.rs`, and the programs are started through the OS layer.

**Signature.** `fn hub_start(&mut self, peer: Peer, settings: &HubSettings) -> (Response, Option<OwnedFd>)` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `peer` | `Peer` | Caller, audit only. |
| `settings` | `&HubSettings` | Typed settings (interface, address, SSID and the rest); validated by `hub::hostapd_conf` and `hub::dnsmasq_conf`; no free text reaches a command. |

**Returns.** `Response::Hub { access_point, dhcp }` with the status after the start. **Errors.** `Denied` "hub not enabled" unless `hub = true`, or the validation message; `Failed` when the start fails. **Side effects.** Writes the files (mode 0600), configures the interface, starts `hostapd` and `dnsmasq`; audited with the interface name. **Tests.** `the_hub_is_refused_unless_enabled_and_then_written_from_the_templates` (tests/broker.rs).

### `Broker::hub_stop` (broker.rs) [@ANCHOR: relay_broker:relay_broker_broker_hub_stop]

**Purpose.** The hub stop action: allowed only when the hub is enabled; stops both programs through the OS layer and answers with the hub status.

**Signature.** `fn hub_stop(&mut self, peer: Peer) -> (Response, Option<OwnedFd>)` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `peer` | `Peer` | Caller, audit only. |

**Returns.** `Response::Hub { access_point, dhcp }`, both normally false. **Errors.** `Denied` "hub not enabled"; `Failed` when stopping fails. **Side effects.** Signals only the processes named in the broker's own pid files (see `HubRunner::stop`); audited. **Tests.** `the_hub_is_refused_unless_enabled_and_then_written_from_the_templates` (tests/broker.rs).

- [@ANCHOR: relay_broker:relay_broker_linux_bind_udp] (os.rs) `LinuxOs::bind_udp(port)`: opens a UDP socket (dual stack IPv6 with v6only off when the kernel has IPv6, plain IPv4 otherwise), close-on-exec, binds every address on `port` and returns the descriptor. An address already in use or a refused port is the returned `io::Error`.
