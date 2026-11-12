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

- Anchors (ADR 0090): a function carries an anchor comment with its id in source, its test carries
  a `Tests` anchor comment with the same id, and this README cites it in the heading of its entry
  (`### name`, then the id prefixed with `relay_broker:`; older one-line bullets do the same). `tests/conventions.rs` fails when source, tests and README disagree
  or a file lacks its SPDX header.
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

### `LinuxOs::wall_clock_secs` (os.rs) [@ANCHOR: relay_broker:relay_broker_os_system_time]

**Purpose.** The wall clock now in Unix seconds, the one source of "before" and "after" for `set_system_time` and of `now_unix_secs` for `time_status`. `LinuxOs::system_time_unix_secs` (the `Os` trait method) calls it; the Linux layer also steps the clock with `clock_settime` in `set_system_time`.

**Signature.** `fn wall_clock_secs() -> u64` (private, associated function)

**Arguments.** None. **Returns.** Seconds since 1970-01-01 UTC, or 0 when the clock reads before 1970. **Errors.** None; the failure case is mapped to 0 rather than a panic. **Side effects.** Reads `CLOCK_REALTIME` only. **Concurrency.** None; pure read. **Tests.** `the_clock_is_stepped_only_when_enabled_and_inside_the_ceilings` (tests/broker.rs) cites it (the broker tests drive the clock through a fake `Os`; whether the real wall clock is read there is unknown, to-do: a direct test of `LinuxOs::wall_clock_secs`).

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

### `kind_of_mode` (os.rs) [@ANCHOR: relay_broker:relay_broker_kind_of_mode]

**Purpose.** Maps `st_mode` and `st_rdev` from `lstat` or `fstat` to a `NodeKind`, the policy's view of a node. `NodeKind` is the closed enum `Char { major, minor }`, `Block`, `Dir`, `Symlink`, `Other` (file, fifo, socket) and `Missing` (does not exist or cannot be examined; produced by the callers).

**Signature.** `fn kind_of_mode(mode: u32, rdev: u64) -> NodeKind` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `mode` | `u32` | The `st_mode` field; only the file-type bits `S_IFMT` are used. |
| `rdev` | `u64` | `st_rdev`; the major and minor are taken from it only for a character device. |

**Returns.** The kind; a block device is `Block` (a disk, always refused by the policy). **Errors.** None. **Side effects.** None; pure. **Tests.** `real_nodes_are_classified_and_dev_null_opens_without_root` (os.rs).

### `read_hex_u16` (os.rs) [@ANCHOR: relay_broker:relay_broker_read_hex_u16]

**Purpose.** Reads a sysfs attribute holding a hexadecimal id (`idVendor`, `idProduct`, for example `08bb`), for the USB identity.

**Signature.** `fn read_hex_u16(path: &Path) -> Option<u16>` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `path` | `&Path` | The attribute file; read as text, trimmed, parsed as base-16 `u16`. |

**Returns.** `Some(value)`, or `None` when the file is missing, unreadable or not hex. **Errors.** None; every failure is `None`, which the caller treats as "no USB device here". **Side effects.** One file read. **Tests.** `sysfs_enumeration_reads_usb_ids_through_the_device_link` (os.rs).

### `usb_identity_from_sysfs` (os.rs) [@ANCHOR: relay_broker:relay_broker_usb_identity_from_sysfs]

**Purpose.** Finds the USB device behind a node: it walks up from a sysfs device directory (at most eight levels) to the directory that holds `idVendor` and `idProduct`, and returns them with the product and manufacturer strings. The tty or sound entry itself has no ids; the USB device is one of its ancestors.

**Signature.** `fn usb_identity_from_sysfs(device_dir: &Path) -> Option<(u16, u16, String, String)>` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `device_dir` | `&Path` | The `device` link of a sysfs class entry; canonicalised first, so the walk follows the real tree. |

**Returns.** `Some((vendor, product id, product string, manufacturer string))` (strings empty when the attributes are missing), or `None` when no ancestor within eight levels holds both ids (not USB, or unreadable). **Errors.** None. **Side effects.** Filesystem reads in sysfs only. **Tests.** `sysfs_enumeration_reads_usb_ids_through_the_device_link` (os.rs).

### `LinuxOs::sysfs_dir_for` (os.rs) [@ANCHOR: relay_broker:relay_broker_linux_sysfs_dir_for]

**Purpose.** The sysfs class directory of a device node: `/dev/ttyUSB0` is `class/tty/ttyUSB0`, `/dev/snd/pcmC1D0c` is `class/sound/pcmC1D0c`, below the configurable `sysfs_root` (normally `/sys`). `LinuxOs` also has `group_file` (`/etc/group`), the `hub: HubRunner` and `update_request` (`/var/lib/hams-relay-broker/update-requested`), all parameters so the tests need no root.

**Signature.** `fn sysfs_dir_for(&self, path: &Path) -> Option<PathBuf>` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `path` | `&Path` | A device path; must start with `/dev`. A single component starting `tty`, or `snd/<name>`, is recognised. |

**Returns.** The directory, or `None` for any other node (a disk, `/dev/null`, a deeper path). **Errors.** None. **Side effects.** None; no file access. **Tests.** `sysfs_enumeration_reads_usb_ids_through_the_device_link` (os.rs).

### `LinuxOs::scan_class` (os.rs) [@ANCHOR: relay_broker:relay_broker_linux_scan_class]

**Purpose.** Lists one sysfs class directory as `DeviceDescriptor` values for `list_devices`. `keep` decides by entry name; `dev_dir` is where the node lives. Only entries with a `device` link (real hardware, not virtual consoles) count.

**Signature.** `fn scan_class(&self, class: DeviceClass, sys_class: &str, dev_dir: &str, keep: fn(&str) -> bool) -> Vec<DeviceDescriptor>` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `class` | `DeviceClass` | The class stamped on each result. |
| `sys_class` | `&str` | The directory under `sysfs_root`, such as `class/tty`. |
| `dev_dir` | `&str` | The directory prefix of the reported path, such as `/dev` or `/dev/snd`. |
| `keep` | `fn(&str) -> bool` | Name filter. |

**Returns.** Descriptors sorted by name, at most `MAX_DEVICES` (64) before filtering, with vendor and product ids, product and manufacturer strings (0 and empty for non-USB hardware). An unreadable class directory gives an empty list. **Errors.** None. **Side effects.** sysfs reads. **Tests.** `sysfs_enumeration_reads_usb_ids_through_the_device_link` (os.rs).

### `LinuxOs::peer_credentials` (os.rs) [@ANCHOR: relay_broker:relay_broker_linux_peer_credentials]

**Purpose.** The kernel's `SO_PEERCRED` of a connected Unix socket: the unforgeable uid, gid and pid of the process that called `connect`. The server reads it on accept, before reading any byte, and feeds it to `AllowList::permits`. `Peer { uid: u32, gid: u32, pid: i32 }` is the result (effective ids; the pid is informational because a pid can be reused).

**Signature.** `fn peer_credentials(&self, socket: BorrowedFd<'_>) -> io::Result<Peer>`

| Argument | Type and range | Meaning |
|---|---|---|
| `socket` | `BorrowedFd` | A connected Unix stream socket. |

**Returns.** The `Peer`. **Errors.** `io::Error` from `getsockopt`; `serve_connection` then treats the peer as rejected. **Side effects.** None. **Tests.** `real_nodes_are_classified_and_dev_null_opens_without_root` (os.rs).

### `LinuxOs::update_package` (os.rs) [@ANCHOR: relay_broker:relay_broker_linux_update_package]

**Purpose.** Asks for the relay package upgrade without the broker holding the right to run a package manager. It creates the request file (`update_request`, `/var/lib/hams-relay-broker/update-requested`); the package's root-owned `hams-relay-update.path` unit notices it and runs the one fixed upgrade command (`apt-get install --only-upgrade hams-local-relay` or `dnf upgrade`). Nothing from the caller reaches that command, and the broker starts no program.

**Signature.** `fn update_package(&self) -> io::Result<()>`

**Arguments.** None. **Returns.** `Ok(())` when the file was written (an existing file is rewritten). **Errors.** The `io::Error` of the write (directory missing or not writable). **Side effects.** One empty file; the upgrade itself happens later, asynchronously, under the unit, so success here means "requested", not "installed". **Tests.** `update_package_leaves_a_request_file_and_runs_no_program` (os.rs).

### `LinuxOs::node_kind` (os.rs) [@ANCHOR: relay_broker:relay_broker_linux_node_kind]

**Purpose.** The kind of the node at `path` without following a final symlink (`lstat`), so a planted link is seen as a link. `Policy::check_node` calls it for every component of the path.

**Signature.** `fn node_kind(&self, path: &Path) -> NodeKind`

| Argument | Type and range | Meaning |
|---|---|---|
| `path` | `&Path` | The node to examine. |

**Returns.** The `NodeKind` through `kind_of_mode`; `Missing` when `lstat` fails for any reason. **Errors.** None; failure is a value. **Side effects.** One `lstat`. **Concurrency.** The answer can be stale by the time of the open, hence the post-open `fd_kind` check. **Tests.** `real_nodes_are_classified_and_dev_null_opens_without_root` (os.rs).

### `LinuxOs::open_device` (os.rs) [@ANCHOR: relay_broker:relay_broker_linux_open_device]

**Purpose.** Opens the validated node and returns the descriptor that is passed to the relay. Called by `Broker::open_device` after `check_path` and `check_node`.

**Signature.** `fn open_device(&self, path: &Path, read_only: bool) -> io::Result<OwnedFd>`

| Argument | Type and range | Meaning |
|---|---|---|
| `path` | `&Path` | The checked path. |
| `read_only` | `bool` | `O_RDONLY` when true, else `O_RDWR`. |

**Returns.** An owned descriptor opened with `O_NOFOLLOW`, `O_CLOEXEC`, `O_NOCTTY` and, only for the open itself, `O_NONBLOCK` (so a tty without carrier returns at once); the flag is cleared afterwards so the worker gets an ordinary blocking descriptor. **Errors.** `io::Error` from `open` (permission, no such device, busy) or from `fcntl`; the descriptor is closed on error. **Side effects.** Opens the device. The caller must run `fd_kind` on the result. **Security.** `O_NOCTTY` stops a serial port becoming the broker's controlling terminal. **Tests.** `real_nodes_are_classified_and_dev_null_opens_without_root` (os.rs).

### `LinuxOs::fd_kind` (os.rs) [@ANCHOR: relay_broker:relay_broker_linux_fd_kind]

**Purpose.** The kind of an already-open descriptor (`fstat`), the post-open check of the time-of-check to time-of-use race: what was opened, not what the path said before.

**Signature.** `fn fd_kind(&self, fd: BorrowedFd<'_>) -> NodeKind`

| Argument | Type and range | Meaning |
|---|---|---|
| `fd` | `BorrowedFd` | The descriptor just opened; borrowed. |

**Returns.** The `NodeKind`, `Missing` when `fstat` fails. **Errors.** None. **Side effects.** One `fstat`. **Tests.** `real_nodes_are_classified_and_dev_null_opens_without_root` (os.rs); `an_allowed_open_passes_a_working_descriptor_and_never_the_path` (tests/broker.rs).

### `LinuxOs::usb_identity` (os.rs) [@ANCHOR: relay_broker:relay_broker_linux_usb_identity]

**Purpose.** The USB vendor and product ids and the product string of the device behind a node, for the `deny_usb` check (for example the VNWA codec `08bb:29c0`). `UsbIdentity` is `Usb(vid, pid, product)`, `NotUsb` (readable and not USB, such as a legacy port or PCI sound card, or a node name with no sysfs home) or `Unreadable`.

**Signature.** `fn usb_identity(&self, path: &Path) -> UsbIdentity`

| Argument | Type and range | Meaning |
|---|---|---|
| `path` | `&Path` | The checked device path. |

**Returns.** `NotUsb` when `sysfs_dir_for` recognises no sysfs home or no USB ancestor exists; `Unreadable` when the node has a sysfs name but its directory is absent (sysfs not mounted or hidden from a container); otherwise `Usb(..)`. **Errors.** None. **Security.** `Policy::check_node` fails closed on `Unreadable` unless `allow_unreadable_usb`. **Side effects.** sysfs reads. **Tests.** `sysfs_enumeration_reads_usb_ids_through_the_device_link` (os.rs); `an_unreadable_usb_identity_is_refused_unless_the_config_allows_it` (tests/broker.rs).

### `LinuxOs::list_devices` (os.rs) [@ANCHOR: relay_broker:relay_broker_linux_list_devices]

**Purpose.** Read-only enumeration of serial and audio hardware for the `list_devices` action: serial nodes `ttyUSB*`, `ttyACM*`, `ttyS*` under `/dev`, and audio nodes `pcmC*`, `controlC*` under `/dev/snd`, each with USB ids and strings when known. The broker removes entries on `deny_usb` afterwards.

**Signature.** `fn list_devices(&self) -> Vec<DeviceDescriptor>`

**Arguments.** None. **Returns.** Serial entries followed by audio entries (two `scan_class` calls, each sorted and capped at 64). **Errors.** None; a missing sysfs gives an empty list. **Side effects.** sysfs reads only; no device is opened. **Tests.** `sysfs_enumeration_reads_usb_ids_through_the_device_link` (os.rs); `the_accept_loop_serves_a_real_listener_filters_protected_devices_and_exits_when_idle` (tests/broker.rs).

### `LinuxOs::username` (os.rs) [@ANCHOR: relay_broker:relay_broker_linux_username]

**Purpose.** The account name of the peer's UID, for the provisioning action (`Broker::grant`): the broker only ever acts on the caller's own account.

**Signature.** `fn username(&self, uid: u32) -> Option<String>`

| Argument | Type and range | Meaning |
|---|---|---|
| `uid` | `u32` | The peer's UID from `SO_PEERCRED`. |

**Returns.** The name through the passwd database (`getpwuid`), or `None` for an unknown uid or a lookup error. **Errors.** None. **Side effects.** A passwd lookup (may read `/etc/passwd` or NSS). **Tests.** `group_membership_is_read_from_the_group_file` (os.rs).

### `LinuxOs::groups_of` (os.rs) [@ANCHOR: relay_broker:relay_broker_linux_groups_of]

**Purpose.** The supplementary groups of an account, read from `group_file` (`/etc/group`), so `Broker::grant` can say which of `dialout` and `audio` are missing.

**Signature.** `fn groups_of(&self, user: &str) -> Vec<String>`

| Argument | Type and range | Meaning |
|---|---|---|
| `user` | `&str` | The account name. |

**Returns.** The names of groups whose member list contains `user` exactly; the account's primary group is not listed unless it also names the user. An unreadable file gives an empty list, so the broker then reports every group as missing. **Errors.** None. **Side effects.** Reads the file. **Tests.** `group_membership_is_read_from_the_group_file` (os.rs).

### `LinuxOs::add_to_group` (os.rs) [@ANCHOR: relay_broker:relay_broker_linux_add_to_group]

**Purpose.** Adds an account to a group. Only reached when the config enables `apply_groups`. This is the broker's only external program besides the hub's.

**Signature.** `fn add_to_group(&self, user: &str, group: &str) -> io::Result<()>`

| Argument | Type and range | Meaning |
|---|---|---|
| `user` | `&str` | The account name from the UID lookup. |
| `group` | `&str` | The group name; one of the closed `Group` set. |

**Returns.** `Ok(())` when `gpasswd` exits successfully. **Errors.** The spawn error, or "gpasswd exited with ..." for a non-zero exit (for example when the broker is not root). **Side effects.** Runs `/usr/sbin/gpasswd -a USER GROUP` with a fixed argv, no shell, a cleared environment and a fixed `PATH`; changes `/etc/group`; blocks until it finishes. **Security.** Both names were validated upstream. **Tests.** `add_to_group_as_an_ordinary_user_fails_without_running_a_shell` (os.rs).

### `Denial::reason` (policy.rs) [@ANCHOR: relay_broker:relay_broker_denial_reason]

**Purpose.** A stable short reason for the audit log and the response when an open is refused. `Denial` is the closed enum of refusals: `NotAbsolute`, `OutsideRoot`, `Traversal`, `TooLong` (over `MAX_STRING` bytes or a NUL), `DenyListed`, `Symlink`, `NotCharDevice`, `WrongClass`, `ProtectedUsb`, `UsbUnreadable`. `Broker::open_device` sends the text as the `Denied` reason and logs it.

**Signature.** `pub fn reason(self) -> &'static str`

| Argument | Type and range | Meaning |
|---|---|---|
| `self` | `Denial` | The refusal, by value (`Copy`). |

**Returns.** A fixed string per variant, for example "path traversal", "symlink in device path", "protected usb device", "usb identity unreadable". **Errors.** None. **Side effects.** None; pure. **Security.** The text is fixed, so the response never reflects peer input and never reveals which of several protected devices matched. **Tests.** `lexical_check_refuses_traversal_relative_and_deny_listed_paths` (policy.rs).

### `Policy::check_path` (policy.rs) [@ANCHOR: relay_broker:relay_broker_policy_check_path]

**Purpose.** The lexical first check of three: it decides from the text of the path alone, touching no file. `Policy` is the policy table loaded from the root-owned config: `device_root` (`/dev`), `serial_majors` (4, 166, 188, 204, 207), `audio_majors` (116), `deny_usb` (the VNWA codec `08bb:29c0`), `allow_unreadable_usb` (false) and `deny_paths` (`/dev/mem`, `/dev/kmem`, `/dev/port`, `/dev/console`, `/dev/tty`).

**Signature.** `pub fn check_path(&self, path: &str) -> Result<PathBuf, Denial>`

| Argument | Type and range | Meaning |
|---|---|---|
| `path` | `&str` | The peer-supplied path; borrowed. Must be absolute, at most `MAX_STRING` bytes, without NUL, no empty, `.` or `..` segment (so also no trailing slash), strictly under `device_root`, not in `deny_paths`. |

**Returns.** The path as a `PathBuf`, byte for byte what will be opened (it is split textually because `Path::components` silently drops `.` segments). **Errors.** `TooLong`, `NotAbsolute`, `Traversal`, `OutsideRoot` (including the root itself), `DenyListed`. **Side effects.** None; pure. **Tests.** `lexical_check_refuses_traversal_relative_and_deny_listed_paths` (policy.rs).

### `Policy::class_of_major` (policy.rs) [@ANCHOR: relay_broker:relay_broker_policy_class_of_major]

**Purpose.** The class a character-device major belongs to under this policy, so a serial tty cannot be opened as audio or the reverse. Used by `check_kind`.

**Signature.** `pub fn class_of_major(&self, major: u32) -> Option<DeviceClass>`

| Argument | Type and range | Meaning |
|---|---|---|
| `major` | `u32` | The device major number from `st_rdev`. |

**Returns.** `Some(Serial)` when the major is in `serial_majors` (checked first), `Some(Audio)` when in `audio_majors`, `None` for any other major (disks, memory devices, input devices). **Errors.** None. **Side effects.** None; pure. **Tests.** `kinds_are_matched_to_the_requested_class_and_disks_refused` (policy.rs).

### `Policy::check_kind` (policy.rs) [@ANCHOR: relay_broker:relay_broker_policy_check_kind]

**Purpose.** Accepts a node kind only when it is a character device of `class`. The shared test behind the node walk (`check_node`) and the post-open check (`check_fd`).

**Signature.** `pub fn check_kind(&self, kind: NodeKind, class: DeviceClass) -> Result<(), Denial>`

| Argument | Type and range | Meaning |
|---|---|---|
| `kind` | `NodeKind` | `Char { major, minor }`, `Symlink`, `Dir` or other, from `lstat` or `fstat`. |
| `class` | `DeviceClass` | The class requested (`Serial` or `Audio`). |

**Returns.** `Ok(())` for a character device whose major maps to `class`. **Errors.** `WrongClass` for another class or for a virtual console (major 4 with minor below 64 is `/dev/tty0..63`, the local keyboard and screen, never handed out); `Symlink`; `NotCharDevice` for everything else (a disk is a block device and fails here). **Side effects.** None; pure. **Tests.** `kinds_are_matched_to_the_requested_class_and_disks_refused` and `virtual_consoles_share_the_serial_major_but_are_never_handed_out` (policy.rs).

### `Policy::check_node` (policy.rs) [@ANCHOR: relay_broker:relay_broker_policy_check_node]

**Purpose.** The filesystem check: every directory on the way from the device root is a real directory (not a symlink), the final node is a character device of `class`, and its USB identity is not protected. `/dev/stdin`, `/dev/fd/0`, `/dev/serial/by-id/...` and a planted link are therefore all refused.

**Signature.** `pub fn check_node(&self, os: &dyn Os, clean: &Path, class: DeviceClass) -> Result<(), Denial>`

| Argument | Type and range | Meaning |
|---|---|---|
| `os` | `&dyn Os` | The OS layer used for `node_kind` and `usb_identity`; borrowed. |
| `clean` | `&Path` | The path returned by `check_path`; must be under `device_root`. |
| `class` | `DeviceClass` | The class requested. |

**Returns.** `Ok(())`. **Errors.** `OutsideRoot` (also for the root itself), `Symlink` (any component), `NotCharDevice` (a non-directory intermediate or missing node), `WrongClass`, `ProtectedUsb` (vendor:product on `deny_usb`), `UsbUnreadable` (sysfs unreadable and `allow_unreadable_usb` false: the check fails closed so hiding sysfs cannot bypass `deny_usb`). **Side effects.** `lstat` and sysfs reads through the OS layer. **Concurrency.** The node can change between this check and the open, which is why `check_fd` follows. **Tests.** `node_walk_uses_the_real_dev_null_without_root` (policy.rs); `an_unreadable_usb_identity_is_refused_unless_the_config_allows_it` (tests/broker.rs).

### `Policy::check_fd` (policy.rs) [@ANCHOR: relay_broker:relay_broker_policy_check_fd]

**Purpose.** The post-open check on the descriptor itself. A swap between the node walk and the open (a time-of-check to time-of-use race) still fails closed because the kind is re-read from the open descriptor with `fstat`.

**Signature.** `pub fn check_fd(&self, kind: NodeKind, class: DeviceClass) -> Result<(), Denial>`

| Argument | Type and range | Meaning |
|---|---|---|
| `kind` | `NodeKind` | From `Os::fd_kind` of the descriptor just opened. |
| `class` | `DeviceClass` | The class requested. |

**Returns.** `Ok(())` when it is a character device of the class. **Errors.** As `check_kind`. **Side effects.** None; the caller closes the descriptor (by drop) on error. **Tests.** `an_allowed_open_passes_a_working_descriptor_and_never_the_path` (tests/broker.rs) and the `check_kind` tests in policy.rs.

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

### `serve_connection` (server.rs) [@ANCHOR: relay_broker:relay_broker_serve_connection]

**Purpose.** Serves one connection to completion: one request per connection. Authenticates the peer with `SO_PEERCRED` before reading anything, reads one bounded frame, decodes, calls `Broker::handle`, answers and returns. Never panics; every failure is an `Outcome` (`PeerRejected`, `Malformed`, `Answered`, `IoError`).

**Signature.** `pub fn serve_connection(broker: &mut Broker, stream: &UnixStream) -> Outcome`

| Argument | Type and range | Meaning |
|---|---|---|
| `broker` | `&mut Broker` | The shared state. |
| `stream` | `&UnixStream` | The accepted connection; borrowed, closed by the caller. |

**Returns.** The `Outcome`. A peer not on the allow-list (or whose credentials cannot be read) gets no response and no byte is read; a rejected peer is audited. A descriptor sent by the client is closed unread. A bad version gets `UnsupportedVersion`, any other parse error `Malformed`. **Errors.** Read or write failures (timeouts after 5 s, peer gone, oversize frame) give `IoError`. **Concurrency.** Blocking; single-threaded. **Tests.** `an_unlisted_peer_is_closed_before_any_byte_is_read` and `oversize_and_malformed_frames_are_refused_without_a_panic` (tests/broker.rs).

### `run` (server.rs) [@ANCHOR: relay_broker:relay_broker_run]

**Purpose.** The accept loop. It accepts connections until `idle` passes with none, which lets systemd socket activation restart the broker on demand, or until `max_connections` have been served (`None` for no limit; the tests use it).

**Signature.** `pub fn run(broker: &mut Broker, listener: &UnixListener, idle: Duration, max_connections: Option<usize>) -> io::Result<usize>`

| Argument | Type and range | Meaning |
|---|---|---|
| `broker` | `&mut Broker` | Shared state. |
| `listener` | `&UnixListener` | The bound or activated listening socket. |
| `idle` | `Duration` | How long to wait for a connection (`poll`); config `idle_exit_secs`. |
| `max_connections` | `Option<usize>` | Stop after this many, or never. |

**Returns.** The number of connections served. **Errors.** `io::Error` from `poll` or a hard `accept` failure; `EINTR`, `WouldBlock` and `Interrupted` are retried. **Concurrency.** Blocking, strictly one connection at a time, each bounded by the 5 s frame deadline, so a slow peer delays others for at most that. **Tests.** `the_accept_loop_serves_a_real_listener_filters_protected_devices_and_exits_when_idle` (tests/broker.rs).

### `activated_listener` (server.rs) [@ANCHOR: relay_broker:relay_broker_activated_listener]

**Purpose.** The listener systemd passed (`LISTEN_PID` is this process and `LISTEN_FDS` is 1), if any. Adopting descriptor 3 (SD_LISTEN_FDS_START) is the crate's second and last `unsafe` block, with a `SAFETY` comment: systemd's protocol guarantees fd 3 is the listening socket and `run_main` calls this first, so ownership is taken exactly once.

**Signature.** `pub fn activated_listener() -> Option<UnixListener>`

**Arguments.** None; reads the environment variables `LISTEN_PID` and `LISTEN_FDS`. **Returns.** `Some(listener)` set to blocking mode, or `None` when the variables are missing, malformed, name another pid, or `LISTEN_FDS` is not 1. **Errors.** None; every failure is `None`. **Side effects.** Takes ownership of fd 3, so call it at most once per process. **Tests.** `the_accept_loop_serves_a_real_listener_filters_protected_devices_and_exits_when_idle` (tests/broker.rs) runs the loop on a listener the test made itself; whether it also exercises the environment-variable path is unknown (to-do: add a direct test of `activated_listener` with `LISTEN_PID`/`LISTEN_FDS` set).

### `set_timeouts` (transport.rs) [@ANCHOR: relay_broker:relay_broker_set_timeouts]

**Purpose.** Applies `IO_TIMEOUT` (5 seconds) as the receive and send timeout of a socket, so a stalled peer cannot hold the single-threaded broker or a client forever.

**Signature.** `pub fn set_timeouts(socket: BorrowedFd<'_>) -> io::Result<()>`

| Argument | Type and range | Meaning |
|---|---|---|
| `socket` | `BorrowedFd` | A connected stream socket; borrowed. |

**Returns.** `Ok(())`. **Errors.** The `io::Error` of `setsockopt` for `SO_RCVTIMEO` or `SO_SNDTIMEO`; callers ignore it. **Side effects.** Changes the two socket options. **Tests.** `a_frame_and_a_descriptor_cross_a_socketpair_and_oversize_is_refused` (transport.rs).

### `send_frame` (transport.rs) [@ANCHOR: relay_broker:relay_broker_send_frame]

**Purpose.** Writes `body` as one length-prefixed frame, passing `fd` (if any) with the first bytes as SCM_RIGHTS ancillary data. Used for every response and every request.

**Signature.** `pub fn send_frame(socket: BorrowedFd<'_>, body: &[u8], fd: Option<BorrowedFd<'_>>) -> io::Result<()>`

| Argument | Type and range | Meaning |
|---|---|---|
| `socket` | `BorrowedFd` | A connected Unix stream socket. |
| `body` | `&[u8]` | The message body; framed by `frame_encode` (bodies over `MAX_FRAME` are cut). |
| `fd` | `Option<BorrowedFd>` | At most one descriptor to pass; borrowed, the caller keeps ownership and closes it afterwards. |

**Returns.** `Ok(())` when the whole frame was sent. **Errors.** `io::Error` from `sendmsg`; `WriteZero` if it accepts no bytes. **Side effects.** Uses `MSG_NOSIGNAL`, so a vanished peer gives an error instead of `SIGPIPE`; loops over partial writes, attaching the descriptor only to the first chunk. **Tests.** `a_frame_and_a_descriptor_cross_a_socketpair_and_oversize_is_refused` (transport.rs).

### `recv_exact` (transport.rs) [@ANCHOR: relay_broker:relay_broker_recv_exact]

**Purpose.** Fills `buf` with `recvmsg`, appending any passed descriptors to `fds`. The whole fill must finish by `deadline`: the socket's own receive timeout restarts on every byte, so alone it would let a peer that sends one byte just inside each timeout hold the single-threaded broker for hours. Before each `recvmsg` the socket timeout is cut to the time that remains.

**Signature.** `fn recv_exact(socket: BorrowedFd<'_>, buf: &mut [u8], fds: &mut Vec<OwnedFd>, deadline: Instant) -> io::Result<()>` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `socket` | `BorrowedFd` | The connected socket. |
| `buf` | `&mut [u8]` | Filled completely; mutated. |
| `fds` | `&mut Vec<OwnedFd>` | Receives descriptors; at most four are kept, any beyond are closed on the spot, never leaked. |
| `deadline` | `Instant` | Latest completion time. |

**Returns.** `Ok(())` with `buf` full. **Errors.** `TimedOut` past the deadline, `UnexpectedEof` when the peer closes, other `io::Error` from `recvmsg` or `setsockopt`. **Side effects.** Rewrites `SO_RCVTIMEO` (never zero, which would mean wait forever); received descriptors are close-on-exec. **Tests.** `a_peer_dribbling_one_byte_per_second_cannot_outlast_the_whole_frame_deadline` (transport.rs).

### `adopt_received_fd` (transport.rs) [@ANCHOR: relay_broker:relay_broker_adopt_received_fd]

**Purpose.** Wraps a descriptor that `recvmsg` just received as SCM_RIGHTS in an `OwnedFd`, so it is closed when dropped whether kept or not. This is the crate's first `unsafe` block: nix 0.31 returns passed descriptors as plain integers, and `OwnedFd::from_raw_fd` is `unsafe` only because the caller must promise sole ownership.

**Signature.** `fn adopt_received_fd(raw: RawFd) -> OwnedFd` (private, `#[allow(unsafe_code)]`)

| Argument | Type and range | Meaning |
|---|---|---|
| `raw` | `RawFd` | A descriptor number created by the kernel in the `recvmsg` call that just returned. |

**Returns.** The owning wrapper. **Errors.** None. **Safety.** Sound only when `raw` is fresh and held by nothing else; the only call site is in `recv_exact`. **Security.** A hostile peer's extra descriptors cannot accumulate, since every one is wrapped at once. **Tests.** `a_frame_and_a_descriptor_cross_a_socketpair_and_oversize_is_refused` (transport.rs).

### `recv_frame` (transport.rs) [@ANCHOR: relay_broker:relay_broker_recv_frame]

**Purpose.** Reads one frame: the body bytes and any descriptors passed with it. The announced length is checked against `MAX_FRAME` before the body is read, so an oversize frame costs the peer a closed connection and the broker four bytes. The whole frame, header and body together, must arrive within `IO_TIMEOUT`, however the peer paces its bytes.

**Signature.** `pub fn recv_frame(socket: BorrowedFd<'_>) -> io::Result<(Vec<u8>, Vec<OwnedFd>)>`

| Argument | Type and range | Meaning |
|---|---|---|
| `socket` | `BorrowedFd` | A connected Unix stream socket. |

**Returns.** `(body, descriptors)`; the descriptors (at most four) are owned by the caller; the server drops them, the client keeps the one that comes with `Opened`. **Errors.** `InvalidData` for a length of 0 or above 4096, `TimedOut`, `UnexpectedEof`, other `io::Error`. **Side effects.** Blocks up to 5 s and changes the receive timeout. **Tests.** `oversize_and_malformed_frames_are_refused_without_a_panic` (tests/broker.rs); the two transport tests in transport.rs.

### `Group::name` (wire.rs) [@ANCHOR: relay_broker:relay_broker_group_name]

**Purpose.** The system group name for the closed `Group` enum (`Dialout = 1`, `Audio = 2`, the wire encoding, never reused). `Broker::grant` compares it with the account's groups and gives it to `gpasswd` as a fixed argument, so no group name from the peer ever reaches a command.

**Signature.** `pub fn name(self) -> &'static str`

**Arguments.** `self` by value. **Returns.** `"dialout"` or `"audio"`. **Errors.** None. **Side effects.** None. **Tests.** `frames_are_bounded_and_tags_closed` (wire.rs).

### `Reader::take` (wire.rs) [@ANCHOR: relay_broker:relay_broker_reader_take]

**Purpose.** The bounds-checked primitive under every other `Reader` method. `Reader<'a> { buf: &'a [u8], pos: usize }` is a cursor over one message body; nothing reads the slice except through `take`, so no input can index out of range.

**Signature.** `fn take(&mut self, n: usize) -> Result<&'a [u8], WireError>` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `n` | `usize` | Bytes wanted; `pos + n` is computed with `checked_add`. |

**Returns.** The next `n` bytes (borrowed from the body) and advances `pos`. **Errors.** `WireError::Truncated` when fewer than `n` remain or the sum overflows; `pos` is unchanged then. **Side effects.** Advances the cursor only. **Tests.** `strings_are_bounded_utf8_and_nul_free` (wire.rs) and the parser property test in tests/fuzz_parser.rs.

### `Reader::u8` (wire.rs) [@ANCHOR: relay_broker:relay_broker_reader_u8]

**Purpose.** Reads one byte: a version, a tag, a boolean-ish flag or a count. **Signature.** `fn u8(&mut self) -> Result<u8, WireError>` (private).

| Argument | Type and range | Meaning |
|---|---|---|
| (none) | | Reads from the cursor. |

**Returns.** The byte and advances one. **Errors.** `Truncated` when the body is exhausted. **Side effects.** Advances the cursor. **Tests.** `strings_are_bounded_utf8_and_nul_free` (wire.rs); the round-trip properties in tests/fuzz_parser.rs.

### `Reader::u16` (wire.rs) [@ANCHOR: relay_broker:relay_broker_reader_u16]

**Purpose.** Reads a big-endian `u16`: a string length, a port, a vendor or product id or a device count. **Signature.** `fn u16(&mut self) -> Result<u16, WireError>` (private).

| Argument | Type and range | Meaning |
|---|---|---|
| (none) | | Reads from the cursor. |

**Returns.** The value, most significant byte first, and advances two. **Errors.** `Truncated` when fewer than two bytes remain. **Side effects.** Advances the cursor. **Tests.** `strings_are_bounded_utf8_and_nul_free` (wire.rs); tests/fuzz_parser.rs.

### `Reader::string` (wire.rs) [@ANCHOR: relay_broker:relay_broker_reader_string]

**Purpose.** A length-prefixed string field: a `u16` length then that many bytes, of at most `MAX_STRING` (255) bytes, UTF-8, no NUL. Used for device paths, names, the wifi settings and reasons.

**Signature.** `fn string(&mut self) -> Result<String, WireError>` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| (none) | | Reads from the cursor. |

**Returns.** An owned `String`. **Errors.** `BadString` for a length above 255 (checked before the bytes are taken), an embedded NUL or invalid UTF-8; `Truncated` when the body is short. **Side effects.** Advances the cursor. **Security.** Rejecting NUL stops a truncation attack on later C-string use; the length bound keeps allocation small. **Tests.** `strings_are_bounded_utf8_and_nul_free` (wire.rs).

### `Reader::finish` (wire.rs) [@ANCHOR: relay_broker:relay_broker_reader_finish]

**Purpose.** Ends a decode: succeeds only when every byte was consumed, so a message with trailing bytes is refused rather than half read. **Signature.** `fn finish(&self) -> Result<(), WireError>` (private).

| Argument | Type and range | Meaning |
|---|---|---|
| (none) | | Compares `pos` with the body length. |

**Returns.** `Ok(())` when `pos == buf.len()`. **Errors.** `WireError::Trailing`. **Side effects.** None. **Tests.** `every_request_round_trips_and_trailing_bytes_are_rejected` (wire.rs).

### `put_string` (wire.rs) [@ANCHOR: relay_broker:relay_broker_put_string]

**Purpose.** Appends a length-prefixed string to an outgoing body, truncating at a character boundary to `MAX_STRING` bytes and dropping NUL bytes, so what the encoder writes always passes `Reader::string`.

**Signature.** `fn put_string(out: &mut Vec<u8>, s: &str)` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `out` | `&mut Vec<u8>` | The body being built; appended to. |
| `s` | `&str` | The text; cut to 255 bytes at a UTF-8 boundary, NULs removed (the length counts the cleaned bytes). |

**Returns.** Nothing. **Errors.** None. **Side effects.** Appends to `out`. **Tests.** `strings_are_bounded_utf8_and_nul_free` (wire.rs).

### `device_class_from_tag` (wire.rs) [@ANCHOR: relay_broker:relay_broker_device_class_from_tag]

**Purpose.** The class for a wire tag: 1 is `Serial`, 2 is `Audio`. The set is closed, and numbers are never reused. **Signature.** `fn device_class_from_tag(tag: u8) -> Result<DeviceClass, WireError>` (private).

| Argument | Type and range | Meaning |
|---|---|---|
| `tag` | `u8` | The byte from the body. |

**Returns.** The `DeviceClass`. **Errors.** `WireError::BadTag(tag)` for anything else. 0 means "all classes" only in `ListDevices`, handled by the caller. **Side effects.** None. **Tests.** `frames_are_bounded_and_tags_closed` (wire.rs).

### `Request::encode` (wire.rs) [@ANCHOR: relay_broker:relay_broker_request_encode]

**Purpose.** Produces the body bytes (no frame header) for a request: the version byte (1), a tag, then typed fields, big-endian. Tags: 1 `ListDevices` (class 0 = all), 2 `OpenDevice` (class, read-only flag, path), 3 `GrantGroup`, 4 `ProvisionGroups`, 5 `BindUdp` (u16 port), 6 `HubStart` (interface, ssid, passphrase, channel, 5 GHz flag, hub IPv4, relay name, optional DNS, `u32` lease minutes), 7 `HubStop`, 8 `HubStatus`, 9 `UpdatePackage`, 10 `SetSystemTime` (u64 target, source, agreeing), 11 `TimeStatus`.

**Signature.** `pub fn encode(&self) -> Vec<u8>`

**Arguments.** `&self`, the request. **Returns.** A fresh body, which `send_frame` frames. **Errors.** None; over-long strings are cut by `put_string`. **Side effects.** None. **Concurrency.** Pure. **Tests.** `every_request_round_trips_and_trailing_bytes_are_rejected` (wire.rs); `every_request_round_trips` (tests/fuzz_parser.rs).

### `Request::decode` (wire.rs) [@ANCHOR: relay_broker:relay_broker_request_decode]

**Purpose.** Parses a request body. Every failure is a `WireError`; no input panics. It is the first thing that sees peer bytes, after the allow-list, so it is also the fuzzing target.

**Signature.** `pub fn decode(body: &[u8]) -> Result<Request, WireError>`

| Argument | Type and range | Meaning |
|---|---|---|
| `body` | `&[u8]` | One frame body of 1 to 4096 bytes; borrowed, strings are copied. |

**Returns.** The typed `Request`. **Errors.** `Version(v)` for a version other than 1 (checked first, nothing else parsed), `BadTag(b)` for an unknown tag, class, group, source or a flag byte other than 0 or 1, `Truncated`, `BadString`, `Trailing`. **Side effects.** None; pure. **Tests.** `every_request_round_trips_and_trailing_bytes_are_rejected` (wire.rs); `arbitrary_bytes_never_panic_the_parsers` (tests/fuzz_parser.rs).

### `Response::error` (wire.rs) [@ANCHOR: relay_broker:relay_broker_response_error]

**Purpose.** Shorthand for an error response: builds `Response::Error { status, reason }`. `Status` is the closed enum `Ok = 0`, `Denied = 1`, `RateLimited = 2`, `Malformed = 3`, `UnsupportedVersion = 4`, `Failed = 5`.

**Signature.** `pub fn error(status: Status, reason: impl Into<String>) -> Response`

| Argument | Type and range | Meaning |
|---|---|---|
| `status` | `Status` | The non-`Ok` code (the type does not forbid `Ok`, but an `Ok` error would not decode back as an error). |
| `reason` | `impl Into<String>` | Short text; cut to 255 bytes at encode time. |

**Returns.** The response value. **Errors.** None. **Side effects.** None. **Tests.** `every_response_round_trips` (wire.rs).

### `Response::encode` (wire.rs) [@ANCHOR: relay_broker:relay_broker_response_encode]

**Purpose.** Produces the response body (no frame header): the version, a status byte, and for `Ok` a payload tag (1 `Devices`, 2 `Opened`, 3 `GroupReport`, 4 `Hub`, 5 `Updated`, 6 `TimeStatus`, 7 `TimeSet`) then fields; an error is the status then a reason string. The `Opened` descriptor is not in the body: it rides in SCM_RIGHTS.

**Signature.** `pub fn encode(&self) -> Vec<u8>`

**Arguments.** `&self`. **Returns.** The body. Device lists are cut at `MAX_DEVICES` (64) and at whole entries that fit within `MAX_FRAME`, so an oversized list loses its tail and not the whole list; a group report lists at most eight groups. **Errors.** None. **Side effects.** None. **Tests.** `every_response_round_trips` and `a_device_list_too_big_for_one_frame_loses_its_tail_not_the_whole_list` (wire.rs); `every_response_round_trips_modulo_truncation` (tests/fuzz_parser.rs).

### `Response::decode` (wire.rs) [@ANCHOR: relay_broker:relay_broker_response_decode]

**Purpose.** Parses a response body. Used by the client (`call`) and by the fuzz and property tests, so a malicious or broken server cannot crash the relay.

**Signature.** `pub fn decode(body: &[u8]) -> Result<Response, WireError>`

| Argument | Type and range | Meaning |
|---|---|---|
| `body` | `&[u8]` | One frame body; borrowed. |

**Returns.** The `Response`. **Errors.** `Version`, `BadTag` (status, payload tag, class or boolean), `TooMany` (more than 64 devices or 8 groups), `Truncated`, `BadString`, `Trailing`. **Side effects.** None; pure. **Tests.** `every_response_round_trips` (wire.rs); `arbitrary_bytes_never_panic_the_parsers` (tests/fuzz_parser.rs).

### `frame_encode` (wire.rs) [@ANCHOR: relay_broker:relay_broker_frame_encode]

**Purpose.** Prefixes a body with its length: `u32` big-endian, then the body. The frame is `length` (1 to 4096) plus that many body bytes. Bodies above `MAX_FRAME` are an encoder bug and are cut to it.

**Signature.** `pub fn frame_encode(body: &[u8]) -> Vec<u8>`

| Argument | Type and range | Meaning |
|---|---|---|
| `body` | `&[u8]` | The message body; copied. |

**Returns.** `4 + len` bytes. **Errors.** None. An empty body gives a header of zero, which `frame_body_len` refuses. **Side effects.** None. **Tests.** `frames_are_bounded_and_tags_closed` and `a_device_list_too_big_for_one_frame_loses_its_tail_not_the_whole_list` (wire.rs).

### `frame_body_len` (wire.rs) [@ANCHOR: relay_broker:relay_broker_frame_body_len]

**Purpose.** The body length announced by a 4-byte header, if it is within bounds. Run on the first four bytes of a frame before anything is allocated, so a hostile length costs only four bytes.

**Signature.** `pub fn frame_body_len(header: [u8; 4]) -> Result<usize, WireError>`

| Argument | Type and range | Meaning |
|---|---|---|
| `header` | `[u8; 4]` | The big-endian length prefix. |

**Returns.** The length, 1 to `MAX_FRAME` (4096). **Errors.** `WireError::BadLength(n)` for 0 or above 4096. **Side effects.** None. **Tests.** `frames_are_bounded_and_tags_closed` (wire.rs); `no_header_above_the_bound_is_accepted` (tests/fuzz_parser.rs).

### `Reader::bool` (wire.rs) [@ANCHOR: relay_broker:relay_broker_reader_bool]

**Purpose.** Reads one byte as a strict boolean: used for the hub flags (5 GHz, optional DNS present), `breaker_open`, `access_point` and `dhcp`. Anything but 0 or 1 is refused rather than read as truthy, so the closed schema stays closed. **Signature.** `fn bool(&mut self) -> Result<bool, WireError>` (private).

| Argument | Type and range | Meaning |
|---|---|---|
| (none) | | Reads one byte from the cursor via `u8`. |

**Returns.** `false` for 0, `true` for 1. **Errors.** `WireError::BadTag(byte)` for any other byte, `Truncated` on an empty remainder. **Side effects.** Advances the cursor. **Tests.** `flags_addresses_and_counts_are_read_exactly_or_refused` (wire.rs).

### `Reader::ipv4` (wire.rs) [@ANCHOR: relay_broker:relay_broker_reader_ipv4]

**Purpose.** Reads an IPv4 address as four bytes in network order: the hub address and the optional upstream DNS server of `HubStart`. **Signature.** `fn ipv4(&mut self) -> Result<Ipv4Addr, WireError>` (private).

| Argument | Type and range | Meaning |
|---|---|---|
| (none) | | Reads four bytes from the cursor. |

**Returns.** The `Ipv4Addr`; any four bytes are accepted here (range checks such as private-network membership belong to `hub::validate`). **Errors.** `WireError::Truncated` when fewer than four bytes remain. **Side effects.** Advances the cursor by four. **Tests.** `flags_addresses_and_counts_are_read_exactly_or_refused` (wire.rs).

### `Reader::u32` (wire.rs) [@ANCHOR: relay_broker:relay_broker_reader_u32]

**Purpose.** Reads a big-endian `u32`: the DHCP lease length in minutes of `HubStart`. **Signature.** `fn u32(&mut self) -> Result<u32, WireError>` (private).

| Argument | Type and range | Meaning |
|---|---|---|
| (none) | | Reads four bytes from the cursor. |

**Returns.** The value; its range is checked later by `hub::validate`. **Errors.** `WireError::Truncated` when fewer than four bytes remain. **Side effects.** Advances the cursor by four. **Tests.** `flags_addresses_and_counts_are_read_exactly_or_refused` (wire.rs).

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

### `hub::validate` (hub.rs) [@ANCHOR: relay_broker:relay_broker_hub_validate]

**Purpose.** Checks the hub settings and names the one that is wrong. Free text is limited to printable ASCII, so a value cannot add a line to a configuration file. `HubSettings` is the data structure: `interface` (a plain name of at most 15 characters from letters, digits, `_`, `-`, `.`), `ssid` (1 to 32 printable ASCII), `passphrase` (8 to 63 printable ASCII, so the hub is never open), `channel: u8` (1 to 13, or 36 to 165 when `five_ghz`), `five_ghz: bool`, `hub_address: Ipv4Addr` (private or link-local host address, not .0 or .255; the network is its /24), `relay_name` (one DNS label of letters, digits and hyphens, up to 63), `uplink_dns: Option<Ipv4Addr>` and `lease_minutes: u32` (1 to 1440).

**Signature.** `pub fn validate(s: &HubSettings) -> Result<(), String>`

| Argument | Type and range | Meaning |
|---|---|---|
| `s` | `&HubSettings` | The settings from the wire; borrowed. |

**Returns.** `Ok(())`. **Errors.** `Err(text)` naming the first invalid setting, in the order above; the text is sent back as the `Denied` reason. **Side effects.** None; pure. **Tests.** `settings_that_could_add_a_line_or_leave_the_hub_open_are_refused` (hub.rs).

### `hub::dnsmasq_conf` (hub.rs) [@ANCHOR: relay_broker:relay_broker_hub_dnsmasq_conf]

**Purpose.** Produces the `dnsmasq.conf` text for the hub's /24: DHCP with the pool .20 to .250, the hub as DNS server (option 6) and NTP server (option 42) and as gateway, the relay's name and `<name>.local` and the OS connectivity-probe names (Google, Apple, Microsoft, Firefox) resolving to the hub so phones open the console instead of declaring "no internet", and everything else forwarded to `uplink_dns` or, with none, refused at once (`no-resolv`).

**Signature.** `pub fn dnsmasq_conf(s: &HubSettings) -> Result<String, String>`

| Argument | Type and range | Meaning |
|---|---|---|
| `s` | `&HubSettings` | Validated first by `validate`. |

**Returns.** The configuration text from a fixed template. **Errors.** The `validate` message. **Side effects.** None; `HubRunner::start` writes the file (mode 0600). **Tests.** `the_dhcp_and_dns_config_points_clients_at_the_hub_for_time_names_and_probes` (hub.rs).

### `hub::takeover_dnsmasq_conf` (hub.rs) [@ANCHOR: relay_broker:relay_broker_hub_takeover_dnsmasq_conf]

**Purpose.** The `dnsmasq.conf` text for taking over DHCP on an existing network (phase 0 of the emergency-hub plan: the hub is a node of the user's own /24 and the router's DHCP is gone). DHCP for the same /24 from a high pool (.200 to .250, away from the router's own and static addresses), five-minute leases (`TAKEOVER_LEASE_MINUTES`) so clients move back quickly after the hand-back, the router as gateway, the hub as DNS and NTP server and the relay's name. dnsmasq pings an address before offering it, which keeps it from handing out one that a still-running machine holds.

**Signature.** `pub fn takeover_dnsmasq_conf(t: &TakeoverSettings) -> Result<String, String>`

| Argument | Type and range | Meaning |
|---|---|---|
| `t` | `&TakeoverSettings` | `interface`, `hub_address`, `gateway` (another host on the hub's /24), `relay_name`, `uplink_dns: Option<Ipv4Addr>`. |

**Returns.** The text. **Errors.** The `validate` message (run through placeholder wifi settings), or "the gateway must be another host address on the hub's /24", or "the hub and the gateway must be outside the takeover pool .200 to .250". **Side effects.** None; pure. **Tests.** `the_takeover_serves_the_same_subnet_from_a_high_pool_with_short_leases` (hub.rs).

### `write_private` (hub_runner.rs) [@ANCHOR: relay_broker:relay_broker_hub_runner_write_private]

**Purpose.** Writes a file readable by root only: the generated `hostapd.conf` carries the WPA passphrase, so it must not be world-readable. Used for both configuration files and the `interface` note.

**Signature.** `fn write_private(path: &Path, text: &str) -> io::Result<()>` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `path` | `&Path` | The file; created or truncated. |
| `text` | `&str` | The content. |

**Returns.** `Ok(())`. **Errors.** The `io::Error` from the open or write. **Side effects.** Creates the file with mode 0600 (the mode applies only on creation; an existing file keeps its mode). **Tests.** `start_runs_both_programs_and_stop_ends_them_and_removes_the_files` (hub_runner.rs).

### `HubRunner::run` (hub_runner.rs) [@ANCHOR: relay_broker:relay_broker_hub_runner_run]

**Purpose.** Runs one helper program (`ip`, `hostapd` or `dnsmasq`) to completion with a cleared environment and a fixed `PATH`; a non-zero exit is an error naming the program. `HubRunner { dir, hostapd, dnsmasq, ip }` holds the paths (defaults `/run/hams-relay-broker/hub`, `/usr/sbin/hostapd`, `/usr/sbin/dnsmasq`, `/usr/sbin/ip`).

**Signature.** `fn run(&self, program: &Path, args: &[&OsStr]) -> io::Result<()>` (private)

| Argument | Type and range | Meaning |
|---|---|---|
| `program` | `&Path` | One of the three fixed paths. |
| `args` | `&[&OsStr]` | A fixed argument list; no shell, so nothing is interpreted. |

**Returns.** `Ok(())` on exit status 0. **Errors.** The spawn error, or "`<program>` exited with ...". **Side effects.** Spawns and waits (blocking); both hub programs daemonise themselves, so the wait is short. **Tests.** `start_runs_both_programs_and_stop_ends_them_and_removes_the_files` (hub_runner.rs).

### `HubRunner::start` (hub_runner.rs) [@ANCHOR: relay_broker:relay_broker_hub_runner_start]

**Purpose.** Brings the hub up: stops any earlier hub, creates the 0700 state directory, writes the two configuration files and an `interface` note (mode 0600), gives the interface the hub's /24 address (`ip addr replace`, `ip link set up`) and starts `hostapd` and `dnsmasq`, each with its own pid file. A failure stops what was started.

**Signature.** `pub fn start(&self, hostapd_conf: &str, dnsmasq_conf: &str, interface: &str, address: Ipv4Addr) -> io::Result<()>`

| Argument | Type and range | Meaning |
|---|---|---|
| `hostapd_conf` | `&str` | The text from `hub::hostapd_conf`. |
| `dnsmasq_conf` | `&str` | The text from `hub::dnsmasq_conf`. |
| `interface` | `&str` | The validated wifi interface. |
| `address` | `Ipv4Addr` | The hub address, configured as `<address>/24`. |

**Returns.** `Ok(())`. **Errors.** Any `io::Error` from the directory, files or the helper programs; the earlier `stop` error also propagates. **Side effects.** Network interface configuration, two background daemons that outlive the broker's idle exit (their pid files are the only state). Needs root or equivalent capabilities. **Tests.** `start_runs_both_programs_and_stop_ends_them_and_removes_the_files` (hub_runner.rs).

### `HubRunner::stop` (hub_runner.rs) [@ANCHOR: relay_broker:relay_broker_hub_runner_stop]

**Purpose.** Stops both programs and removes the files and the interface address. Stopping a hub that is not running succeeds. A pid is trusted only while `/proc/<pid>/comm` still names the program, so a reused pid is never signalled.

**Signature.** `pub fn stop(&self) -> io::Result<()>`

**Arguments.** None. **Returns.** `Ok(())`. **Errors.** The `io::Error` of removing a file other than "not found"; failures to signal or to flush the address are ignored. **Side effects.** Sends `SIGTERM` to `hostapd` and `dnsmasq` whose pid files name a live process of that exact name; runs `ip addr flush dev <interface>` when the saved interface is a plain name; deletes `hostapd.conf`, `dnsmasq.conf`, `interface` and the two pid files. Does not wait for the daemons to exit. **Tests.** `start_runs_both_programs_and_stop_ends_them_and_removes_the_files` (hub_runner.rs).

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

### `LinuxOs::bind_udp` (os.rs) [@ANCHOR: relay_broker:relay_broker_linux_bind_udp]

**Purpose.** Binds a UDP socket on the wildcard address and returns the descriptor, so the relay can serve a privileged port (123, its time service) without any privilege of its own. Dual stack: an IPv6 socket with `IPV6_V6ONLY` off when the kernel has IPv6, plain IPv4 otherwise.

**Signature.** `fn bind_udp(&self, port: u16) -> io::Result<OwnedFd>`

| Argument | Type and range | Meaning |
|---|---|---|
| `port` | `u16` | The port; the broker has already checked that the config lists it. |

**Returns.** The bound, close-on-exec descriptor, owned by the caller. **Errors.** `io::Error` from `socket`, `setsockopt` or `bind` (address in use, permission denied for a privileged port when the broker has no `CAP_NET_BIND_SERVICE`); `EAFNOSUPPORT` on the IPv6 socket falls back to IPv4. **Side effects.** Takes the port system-wide until the descriptor is closed. **Tests.** `a_bound_udp_descriptor_is_a_real_socket_on_the_port_and_the_port_is_then_taken` (os.rs).


## Data structures

Every data structure of the broker, explained in full (ADR 0113): purpose, fields, who builds and reads it. Each sits under the anchor on its definition in `src/`.

### `AuditEvent` [@ANCHOR: relay_broker:audit_audit_event_struct]

**Purpose.** One audit record.

**Definition.** `pub struct AuditEvent` (struct) in `daemons/relay_broker/src/audit.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `uid` | `u32` | Peer UID (`u32::MAX` when unknown). |
| `pid` | `i32` | Peer PID. |
| `action` | `&'static str` | The action name (`open_device`, `list_devices`, `grant_group`, `provision_groups`, `peer_rejected`, `malformed`). |
| `outcome` | `&'static str` | `allowed`, `denied`, `dry_run`, `applied`, `rate_limited`, `failed`, `rejected`. |
| `detail` | `String` | Free text, bounded by the caller (a path, a reason). |

**Used by.** `record`, `record_at` (functions whose signature names it).

### `AuditLog` [@ANCHOR: relay_broker:audit_audit_log_struct]

**Purpose.** An open append-only log.

**Definition.** `pub struct AuditLog` (struct) in `daemons/relay_broker/src/audit.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `file` | `Option<File>` | The open log file (append mode, created with mode 0600); `None` when logging to a file is disabled (tests only). |
| `path` | `Option<PathBuf>` | Where the file lives, kept so it can be rotated to `<path>.1`; `None` with no file. |
| `size` | `u64` | Bytes in the file now (starts at the existing length, grows with every record). |
| `max_bytes` | `u64` | The rotation cap, `MAX_LOG_BYTES`; reaching it rotates the file. |
| `last_rejection` | `HashMap<u32, u64>` | UID to Unix time of the last `peer_rejected` line written, so a turned-away peer is logged at most once a minute. |

**Used by.** `open` (functions whose signature names it).

**Notes.** Created by `AuditLog::open`, written by `record`/`record_at`; owned by the `Broker` and used under the connection handling that owns the broker, so it needs no lock of its own. A write failure is reported to the caller but never stops the broker.

### `AllowList` [@ANCHOR: relay_broker:auth_allow_list_struct]

**Purpose.** The allow-list.

**Definition.** `pub struct AllowList` (struct) in `daemons/relay_broker/src/auth.rs`; derives `Debug`, `Clone`, `Default`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `uids` | `Vec<u32>` | UIDs allowed. |
| `gids` | `Vec<u32>` | GIDs allowed (the peer's primary GID). |

### `Broker` [@ANCHOR: relay_broker:broker_broker_struct]

**Purpose.** The broker state shared by every connection.

**Definition.** `pub struct Broker` (struct) in `daemons/relay_broker/src/broker.rs`.

| Field | Type | Meaning |
|---|---|---|
| `allow` | `AllowList` | Peers allowed in. |
| `policy` | `Policy` | The open policy. |
| `apply_groups` | `bool` | Whether group changes are applied or only reported. |
| `bind_udp_ports` | `Vec<u16>` | UDP ports `bind_udp` may bind (default: 123, the time service). |
| `hub_enabled` | `bool` | Whether the hub requests are served at all (default: refused). |
| `update_package` | `bool` | Whether `update_package` is served (default: refused). |
| `set_time` | `bool` | Whether `set_system_time` is served (default: refused). |
| `time_guard` | `TimeGuard` | The clock-step ceilings and circuit breaker (`TimeGuard`). |
| `rate` | `RateLimiter` | The per-UID token buckets (`RateLimiter`). |
| `audit` | `AuditLog` | The audit log every decision is written to (`AuditLog`). |
| `os` | `Box<dyn Os + Send>` | The operating-system layer (`Os`, normally `LinuxOs`; a fake in tests) that opens nodes, reads sysfs, edits groups and sets the clock. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Built once from `Config` by `Broker::new` and passed by `&mut` to `serve_connection` for each peer connection. The private members are not public API; the public fields are the policy the config set.

### `Args` [@ANCHOR: relay_broker:cli_args_struct]

**Purpose.** Command-line options.

**Definition.** `struct Args` (struct) in `daemons/relay_broker/src/cli.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `config` | `PathBuf` | Path of the root-owned config file (default `/etc/hams-relay-broker.conf`). |
| `socket` | `Option<PathBuf>` | Optional socket path overriding the default (`None` keeps the socket-activation descriptor). |
| `config_owner` | `u32` | The UID the config file must belong to (default 0, root). |

**Used by.** `parse_args` (functions whose signature names it).

**Notes.** Built by `parse_args` from argv; unknown options are an error. Private to the binary's command-line handling.

### `Config` [@ANCHOR: relay_broker:config_config_struct]

**Purpose.** The parsed configuration.

**Definition.** `pub struct Config` (struct) in `daemons/relay_broker/src/config.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `allow` | `AllowList` | Who may talk to the broker. |
| `policy` | `Policy` | The open policy. |
| `apply_groups` | `bool` | Whether `grant_group`/`provision_groups` change anything (default: report only). |
| `audit_log` | `Option<PathBuf>` | Where the audit log goes; `None` logs nowhere (tests only). |
| `bind_udp_ports` | `Vec<u16>` | UDP ports `bind_udp` may bind: the time service's 123 unless `bind_udp_port` lines replace it. |
| `hub_enabled` | `bool` | Whether `hub_start`/`hub_stop` are served (`hub = true`; default off, a hub is something the owner sets up). |
| `update_package` | `bool` | Whether `update_package` is served (`update_package = true`; default off, the packaged config turns it on). |
| `set_time` | `bool` | Whether `set_system_time` is served (`set_time = true`; default off, the packaged config turns it on). |
| `max_unproven_step_secs` | `u64` | The largest clock step (seconds) a request without a GPS source or two agreeing sources may make (`max_unproven_step_secs`). |
| `max_time_steps_per_hour` | `u32` | Clock steps allowed per hour before the circuit breaker trips (`max_time_steps_per_hour`). |
| `rate_per_minute` | `u32` | Requests per minute per peer. |
| `idle_exit_secs` | `u64` | Exit after this many idle seconds (socket activation restarts the broker on demand). |

**Used by.** `default`, `load`, `parse` (functions whose signature names it).

### `ConfigError` [@ANCHOR: relay_broker:config_config_error_struct]

**Purpose.** A configuration error, with the line number.

**Definition.** `pub struct ConfigError` (struct) in `daemons/relay_broker/src/config.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `line` | `usize` | 1-based line, 0 for file-level problems. |
| `message` | `String` | What was wrong. |

**Used by.** `load`, `parse` (functions whose signature names it).

### `HubSettings` [@ANCHOR: relay_broker:hub_hub_settings_struct]

**Purpose.** The owner's hub settings.

**Definition.** `pub struct HubSettings` (struct) in `daemons/relay_broker/src/hub.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `interface` | `String` | The wifi interface the access point uses, such as `wlan0`. |
| `ssid` | `String` | The network name, 1 to 32 printable ASCII characters, for example `HAMS-EMERGENCY-K6BP`. |
| `passphrase` | `String` | The WPA2 passphrase, 8 to 63 printable ASCII characters. The hub is never open. |
| `channel` | `u8` | 2.4 GHz channel 1 to 13, or a 5 GHz channel (36 to 165) when `five_ghz`. |
| `five_ghz` | `bool` | True for the 5 GHz band. |
| `hub_address` | `Ipv4Addr` | The hub's address; the network is its /24. |
| `relay_name` | `String` | The relay's DNS name, a single label such as `relay`; it resolves to the hub address. |
| `uplink_dns` | `Option<Ipv4Addr>` | The uplink's resolver, when there is an uplink; without one names other than the hub's fail at once. |
| `lease_minutes` | `u32` | Lease time in minutes (default 60). |

**Used by.** `dnsmasq_conf`, `good`, `hostapd_conf`, `validate` (functions whose signature names it).

### `TakeoverSettings` [@ANCHOR: relay_broker:hub_takeover_settings_struct]

**Purpose.** Phase 0 takeover (RELAY_STANDALONE_EMERGENCY_HUB.md section 1a): the hub is a node of the user's own /24 and the router's DHCP is gone.

**Definition.** `pub struct TakeoverSettings` (struct) in `daemons/relay_broker/src/hub.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `interface` | `String` | The bridge (or Ethernet) interface on the user's LAN. |
| `hub_address` | `Ipv4Addr` | The hub's own fixed address on that LAN; the subnet is its /24. |
| `gateway` | `Ipv4Addr` | The router, handed out as the default gateway so nothing changes for clients when it comes back. |
| `relay_name` | `String` | The relay's DNS name, one label. |
| `uplink_dns` | `Option<Ipv4Addr>` | The router's DNS server when it is still reachable; none means names other than the hub's fail at once. |

**Used by.** `takeover`, `takeover_dnsmasq_conf` (functions whose signature names it).

### `HubRunner` [@ANCHOR: relay_broker:hub_runner_hub_runner_struct]

**Purpose.** Where the hub's programs and files are.

**Definition.** `pub struct HubRunner` (struct) in `daemons/relay_broker/src/hub_runner.rs`; derives `Debug`, `Clone`.

| Field | Type | Meaning |
|---|---|---|
| `dir` | `PathBuf` | Configuration and pid files. |
| `hostapd` | `PathBuf` | The access-point program. |
| `dnsmasq` | `PathBuf` | The DHCP and DNS program. |
| `ip` | `PathBuf` | The `ip` program, used to give the wifi interface the hub's address. |

**Used by.** `default`, `fake` (functions whose signature names it).

### `UsbIdentity` [@ANCHOR: relay_broker:os_usb_identity_enum]

**Purpose.** What the OS layer could find out about the USB identity behind a device node.

**Definition.** `pub enum UsbIdentity` (enum) in `daemons/relay_broker/src/os.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Usb` | `(u16, u16, String)` | A USB device: vendor id, product id, product string. |
| `NotUsb` | - | Readable, and not USB (a legacy serial port, a PCI sound card, a virtual console), or a node name with no sysfs home. |
| `Unreadable` | - | The node's sysfs entry cannot be read (sysfs not mounted, hidden from a container). |

**Used by.** `usb_identity` (functions whose signature names it).

### `Peer` [@ANCHOR: relay_broker:os_peer_struct]

**Purpose.** The credentials of the process at the other end of a Unix socket.

**Definition.** `pub struct Peer` (struct) in `daemons/relay_broker/src/os.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `uid` | `u32` | Effective UID. |
| `gid` | `u32` | Effective GID. |
| `pid` | `i32` | Process id (informational; a pid can be reused). |

**Used by.** `peer_credentials` (functions whose signature names it).

### `NodeKind` [@ANCHOR: relay_broker:os_node_kind_enum]

**Purpose.** What a filesystem node is, as far as the policy cares.

**Definition.** `pub enum NodeKind` (enum) in `daemons/relay_broker/src/os.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Char` | - | A character device with its major and minor numbers. |
| `Block` | - | A block device (disk): always refused. |
| `Dir` | - | A directory. |
| `Symlink` | - | A symbolic link (checked with lstat, so never followed). |
| `Other` | - | Anything else (regular file, fifo, socket). |
| `Missing` | - | Does not exist or cannot be examined. |

**Used by.** `fd_kind`, `kind_of_mode`, `node_kind` (functions whose signature names it).

### `LinuxOs` [@ANCHOR: relay_broker:os_linux_os_struct]

**Purpose.** The Linux implementation: sysfs for enumeration, `/etc/group` for membership.

**Definition.** `pub struct LinuxOs` (struct) in `daemons/relay_broker/src/os.rs`; derives `Debug`, `Clone`.

| Field | Type | Meaning |
|---|---|---|
| `sysfs_root` | `PathBuf` | Normally `/sys`. |
| `group_file` | `PathBuf` | Normally `/etc/group`. |
| `hub` | `crate::hub_runner::HubRunner` | The stand-alone hub's programs and files. |
| `update_request` | `PathBuf` | Normally `/var/lib/hams-relay-broker/update-requested`: the file `update_package` creates; the relay package's root-owned `hams-relay-update.path` unit watches it and runs the package upgrade, because this unprivileged service cannot. |

**Used by.** `default` (functions whose signature names it).

### `Denial` [@ANCHOR: relay_broker:policy_denial_enum]

**Purpose.** Why an open was refused. Short, fixed, loggable.

**Definition.** `pub enum Denial` (enum) in `daemons/relay_broker/src/policy.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `NotAbsolute` | - | Not an absolute path. |
| `OutsideRoot` | - | Not under the device root. |
| `Traversal` | - | Contains `.`/`..` or an empty component. |
| `TooLong` | - | Longer than the wire allows or contains NUL. |
| `DenyListed` | - | Explicitly on the deny list. |
| `Symlink` | - | A symlink was found in the chain. |
| `NotCharDevice` | - | Not a character device (a disk, a file, a directory, a missing node). |
| `WrongClass` | - | A character device of another class than requested. |
| `ProtectedUsb` | - | The USB vendor:product is protected (for example the VNWA codec). |
| `UsbUnreadable` | - | The USB identity could not be read (sysfs unreadable) and `allow_unreadable_usb` is off. |

**Used by.** `check_fd`, `check_kind`, `check_node`, `check_path` (functions whose signature names it).

### `Policy` [@ANCHOR: relay_broker:policy_policy_struct]

**Purpose.** The policy table. Loaded from the root-owned config; defaults are the Linux conventions.

**Definition.** `pub struct Policy` (struct) in `daemons/relay_broker/src/policy.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `device_root` | `PathBuf` | Every allowed node lives under this directory (`/dev`). |
| `serial_majors` | `Vec<u32>` | Character-device majors that are serial ttys. |
| `audio_majors` | `Vec<u32>` | Character-device majors that are ALSA nodes. |
| `deny_usb` | `Vec<(u16, u16)>` | USB vendor:product pairs never handed out. |
| `allow_unreadable_usb` | `bool` | Hand out a node whose USB identity cannot be read (sysfs unreadable). Default false: the check fails closed, so `deny_usb` cannot be bypassed by hiding sysfs. The one machine that needs this (a container with no `/sys`, a VNWA-free host) sets `allow_unreadable_usb = true`. |
| `deny_paths` | `Vec<PathBuf>` | Exact paths never handed out, whatever they are. |

**Used by.** `default` (functions whose signature names it).

### `RateLimiter` [@ANCHOR: relay_broker:ratelimit_rate_limiter_struct]

**Purpose.** Token buckets keyed by UID.

**Definition.** `pub struct RateLimiter` (struct) in `daemons/relay_broker/src/ratelimit.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `capacity` | `u32` | Bucket size and burst: `per_minute`, at least 1. |
| `refill_every` | `Duration` | Time to earn one token back: 60 seconds divided by `capacity`. |
| `buckets` | `HashMap<u32, (u32, Instant, Instant)>` | uid -> (tokens, time up to which refills are accounted, time of the last request) |

**Used by.** `new` (functions whose signature names it).

**Notes.** Per-UID state is in the private `buckets` map (uid to tokens, time up to which refills are counted, time of the last request); at most 256 peers are tracked and the one that asked longest ago is dropped first. Time is passed in by the caller, so there is no clock inside.

### `Outcome` [@ANCHOR: relay_broker:server_outcome_enum]

**Purpose.** What happened on one connection, for the caller's bookkeeping and the tests.

**Definition.** `pub enum Outcome` (enum) in `daemons/relay_broker/src/server.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `PeerRejected` | - | The peer was not on the allow-list; nothing was read. |
| `Malformed` | - | The request did not parse; a `Malformed`/`UnsupportedVersion` answer was sent. |
| `Answered` | - | A request was handled and answered (allowed or denied by policy). |
| `IoError` | - | I/O failed (timeout, peer gone, oversize frame). |

**Used by.** `serve_connection` (functions whose signature names it).

### `TestDir` [@ANCHOR: relay_broker:testdir_test_dir_struct]

**Purpose.** Test-only scratch directory: wraps a fresh, empty directory under the system temp dir, named `relay_broker_<tag>_<pid>`, and removes the whole tree when dropped, also while a failing test unwinds.

**Definition.** `pub struct TestDir(PathBuf);` (struct) in `daemons/relay_broker/src/testdir.rs`.

**Used by.** `new` (functions whose signature names it).

**Notes.** Compiled only under `cfg(test)`. It dereferences to `Path`, so it is passed wherever a path is wanted.

### `TimeDenial` [@ANCHOR: relay_broker:time_guard_time_denial_enum]

**Purpose.** Why a step was refused. Short and loggable.

**Definition.** `pub enum TimeDenial` (enum) in `daemons/relay_broker/src/time_guard.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `BeforeBuildDate` | - | The target is earlier than [`BUILD_DATE_UNIX_SECS`]. |
| `UnprovenLargeStep` | - | A large step without a GPS source or two agreeing sources. |
| `BreakerOpen` | - | The circuit breaker is open. |

**Used by.** `check` (functions whose signature names it).

### `TimeGuard` [@ANCHOR: relay_broker:time_guard_time_guard_struct]

**Purpose.** The step history and the breaker.

**Definition.** `pub struct TimeGuard` (struct) in `daemons/relay_broker/src/time_guard.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `max_unproven_step_secs` | `u64` | Largest step (seconds) allowed without a GPS source or two agreeing sources. |
| `max_steps_per_hour` | `usize` | Steps allowed per rolling hour before the breaker trips. |
| `steps` | `VecDeque<Instant>` | Times of the completed steps in the last hour, oldest first; older ones are dropped by `expire`. |
| `breaker_until` | `Option<Instant>` | When set, every step is refused until that instant (one hour after the trip). |

**Used by.** `new` (functions whose signature names it).

**Notes.** `check` decides and trips the breaker; `record_step` counts a completed step. The caller supplies `now`, so tests need no clock.

### `WireError` [@ANCHOR: relay_broker:wire_wire_error_enum]

**Purpose.** A parse failure. The variant names what was wrong so a fuzzer finding reads directly.

**Definition.** `pub enum WireError` (enum) in `daemons/relay_broker/src/wire.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Truncated` | - | Fewer bytes than the schema needs at this point. |
| `Trailing` | - | Bytes left over after a complete message. |
| `BadTag` | `(u8)` | A tag, status or enum byte outside the closed set. |
| `BadString` | - | A string field longer than `MAX_STRING`, not UTF-8, or containing a NUL. |
| `BadLength` | `(u32)` | A frame length of zero or above `MAX_FRAME`. |
| `Version` | `(u8)` | The version byte is not `VERSION`. |
| `TooMany` | - | More list entries than `MAX_DEVICES`. |

**Used by.** `bool`, `decode`, `device_class_from_tag`, `finish`, `ipv4`, `string`, `take`, `u16` (functions whose signature names it).

### `DeviceClass` [@ANCHOR: relay_broker:wire_device_class_enum]

**Purpose.** Device classes the broker knows. The numbers are the wire encoding and are never reused.

**Definition.** `pub enum DeviceClass` (enum) in `daemons/relay_broker/src/wire.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`.

| Variant | Type | Meaning |
|---|---|---|
| `Serial` | `= 1` | A serial tty (CAT/PTT port). |
| `Audio` | `= 2` | An ALSA sound node (`/dev/snd/*`). |

**Used by.** `device_class_from_tag` (functions whose signature names it).

### `Group` [@ANCHOR: relay_broker:wire_group_enum]

**Purpose.** Groups the provisioning action may add the caller to. Closed set.

**Definition.** `pub enum Group` (enum) in `daemons/relay_broker/src/wire.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`.

| Variant | Type | Meaning |
|---|---|---|
| `Dialout` | `= 1` | Serial ports. |
| `Audio` | `= 2` | Sound devices. |

### `Request` [@ANCHOR: relay_broker:wire_request_enum]

**Purpose.** What a client may ask for. Nothing else exists.

**Definition.** `pub enum Request` (enum) in `daemons/relay_broker/src/wire.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `ListDevices` | - | Enumerate devices of one class, or all classes. |
| `OpenDevice` | - | Open a device node and return its descriptor. |
| `GrantGroup` | - | Add the caller to one group (dry run unless the config enables apply). |
| `ProvisionGroups` | - | Add the caller to every group the relay needs (dry run unless enabled). |
| `BindUdp` | - | Bind a UDP socket on a privileged port the config lists (123 for the relay's time service) and pass the descriptor. |
| `HubStart` | `(HubSettings)` | Start the stand-alone hub's access point and DHCP/DNS server from the validated settings and fixed templates. |
| `HubStop` | - | Stop the hub and remove its files. |
| `HubStatus` | - | Whether the hub's two programs are running. |
| `UpdatePackage` | - | Upgrade the installed relay package through the system package manager (a fixed command, no arguments from the caller; only with `update_package = true`). |
| `SetSystemTime` | - | Step the system clock to a UTC the relay decided on; the broker applies its own ceilings (`time_guard.rs`). |
| `TimeStatus` | - | Read-only: the broker's clock and its time-step limits. |

**Used by.** `decode`, `samples` (functions whose signature names it).

### `TimeSource` [@ANCHOR: relay_broker:wire_time_source_enum]

**Purpose.** The kind of source a clock step rests on.

**Definition.** `pub enum TimeSource` (enum) in `daemons/relay_broker/src/wire.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Gps` | `= 1` | A GPS receiver with a fix. |
| `Network` | `= 2` | A network time source. |
| `Other` | `= 3` | Another relay or a person. |

### `DeviceDescriptor` [@ANCHOR: relay_broker:wire_device_descriptor_struct]

**Purpose.** One enumerated device.

**Definition.** `pub struct DeviceDescriptor` (struct) in `daemons/relay_broker/src/wire.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `class` | `DeviceClass` | Class. |
| `path` | `String` | Device node path. |
| `vid` | `u16` | USB vendor id, 0 when not USB. |
| `pid` | `u16` | USB product id, 0 when not USB. |
| `product` | `String` | Product string from sysfs, possibly empty. |
| `manufacturer` | `String` | Manufacturer string from sysfs, possibly empty (the relay's port list shows it next to the product). |

### `Status` [@ANCHOR: relay_broker:wire_status_enum]

**Purpose.** Result code of a response.

**Definition.** `pub enum Status` (enum) in `daemons/relay_broker/src/wire.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Ok` | `= 0` | Done; the payload follows. |
| `Denied` | `= 1` | Refused by policy. |
| `RateLimited` | `= 2` | Too many requests from this peer. |
| `Malformed` | `= 3` | The request did not parse. |
| `UnsupportedVersion` | `= 4` | Wrong protocol version. |
| `Failed` | `= 5` | The action was attempted and failed (OS error). |

**Used by.** `error` (functions whose signature names it).

### `Response` [@ANCHOR: relay_broker:wire_response_enum]

**Purpose.** What the broker answers.

**Definition.** `pub enum Response` (enum) in `daemons/relay_broker/src/wire.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Devices` | `(Vec<DeviceDescriptor>)` | Answer to `ListDevices`. |
| `Opened` | - | Answer to `OpenDevice` and `BindUdp`; the descriptor is in the ancillary data of the same frame. |
| `GroupReport` | - | Answer to `GrantGroup`/`ProvisionGroups`. |
| `Hub` | - | Answer to `HubStart`, `HubStop` and `HubStatus`: which of the hub's programs are running now. |
| `Updated` | - | Answer to `UpdatePackage`: the package manager ran and exited successfully. |
| `TimeStatus` | - | Answer to `TimeStatus`. |
| `TimeSet` | - | Answer to `SetSystemTime`: the clock before and after. |
| `Error` | - | Any non-`Ok` status with a short reason. |

**Used by.** `decode`, `error` (functions whose signature names it).

### `Reader` [@ANCHOR: relay_broker:wire_reader_struct]

**Purpose.** A bounds-checked cursor over a byte slice.

**Definition.** `struct Reader<'a>` (struct) in `daemons/relay_broker/src/wire.rs`.

| Field | Type | Meaning |
|---|---|---|
| `buf` | `&'a [u8]` | The bytes being parsed. |
| `pos` | `usize` | Offset of the next unread byte; only moves forward and never passes `buf.len()`. |

**Notes.** Every read checks bounds and returns `WireError::Truncated` instead of panicking; used by the request and response parsers in this file.

