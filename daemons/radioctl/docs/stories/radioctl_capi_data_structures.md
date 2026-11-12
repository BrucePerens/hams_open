# Data structures

### `Shared` [@ANCHOR: radioctl:radioctl_capi_ffi_shared]

**Purpose and location.** The state a runtime handle shares with every rig and rotator handle made from it. Defined in `daemons/radioctl/crates/radioctl-capi/src/ffi.rs:69` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `handle` | `Handle` | The tokio runtime handle that drives rigs. |
| `_owned` | `Option<tokio::runtime::Runtime>` | The runtime this library built, if it built one; `None` when the caller supplied a handle. |
| `rigs` | `AtomicUsize` | Count of open rig and rotator handles (atomic); a runtime cannot be freed while it is not zero. |

**Invariants, ownership and concurrency.** Held in an `Arc` by the runtime, rig and rotator handles, so it outlives all of them; the counter is atomic and no lock is held.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_host_supplied_runtime_owns_nothing_the_c_side_must_build (tests/ffi.rs)`.

### `RctlRuntime` [@ANCHOR: radioctl:radioctl_capi_ffi_rctl_runtime]

**Purpose and location.** The opaque runtime handle of the C interface (`rctl_runtime`): the tokio runtime that drives rigs. Defined in `daemons/radioctl/crates/radioctl-capi/src/ffi.rs:77` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `magic` | `u64` | A fixed number (`MAGIC_RUNTIME`) checked on every call so most stale or wrong handles are refused. |
| `shared` | `Arc<Shared>` | The shared state. |

**Invariants, ownership and concurrency.** Created by the C call that makes a runtime and freed by the call that frees it, which refuses while rigs are open. Crosses the C boundary as a raw pointer, so every function checks it for null and for the magic number.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `runtime_ref` (src/ffi.rs:233), `runtime_from_handle` (src/ffi.rs:294), `rctl_runtime_new` (src/ffi.rs:335), `rctl_runtime_free` (src/ffi.rs:366).

**See also:** test `a_host_supplied_runtime_owns_nothing_the_c_side_must_build (tests/ffi.rs)`.

### `Keep` [@ANCHOR: radioctl:radioctl_capi_ffi_keep]

**Purpose and location.** What a rig or rotator handle keeps alive besides the rig: the transport's connection monitor. Defined in `daemons/radioctl/crates/radioctl-capi/src/ffi.rs:82` as a private `enum`. It derives no traits.

| Variant | Definition | Meaning |
|---|---|---|
| `Nothing` | `Nothing` | No monitor (for a virtual or in-process transport); reads as connected. |
| `Net` | `Net(NetMonitor)` | A network transport's `NetMonitor`. |
| `Serial` | `Serial(SerialMonitor)` | A serial transport's `SerialMonitor`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_tcp_rig_through_the_c_api_is_unkeyed_on_reconnect_and_on_close (tests/ffi.rs)`.

### `RctlRig` [@ANCHOR: radioctl:radioctl_capi_ffi_rctl_rig]

**Purpose and location.** The opaque rig handle of the C interface (`rctl_rig`): one open rig. Defined in `daemons/radioctl/crates/radioctl-capi/src/ffi.rs:102` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `magic` | `u64` | A fixed number (`MAGIC_RIG`) checked on every call. |
| `shared` | `Arc<Shared>` | The shared runtime state; the handle counts in `rigs`. |
| `handle` | `RigHandle` | The Rust rig handle. |
| `keep` | `Keep` | The transport's monitor, kept alive for the life of the handle. |

**Invariants, ownership and concurrency.** Raw pointer on the C side; closing it decrements the rig count. Calls on one handle from several threads are serialized by the rig handle itself; the C interface documents what the caller may do concurrently (see the header).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `rig_ref` (src/ffi.rs:247), `rctl_rig_close` (src/ffi.rs:539), `rctl_rig_get_mode` (src/ffi.rs:623), `rctl_rig_set_mode` (src/ffi.rs:655), `rctl_rig_get_ptt` (src/ffi.rs:676), `rctl_rig_set_ptt` (src/ffi.rs:700), `rctl_rig_revoke_transmit` (src/ffi.rs:745), `rctl_rig_link_up` (src/ffi.rs:780).

**See also:** test `a_tcp_rig_through_the_c_api_is_unkeyed_on_reconnect_and_on_close (tests/ffi.rs)`.

### `RctlRotator` [@ANCHOR: radioctl:radioctl_capi_ffi_rctl_rotator]

**Purpose and location.** The opaque rotator handle of the C interface (`rctl_rotator`): one open rotator. Rotators count with rigs when a runtime is freed. Defined in `daemons/radioctl/crates/radioctl-capi/src/ffi.rs:111` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `magic` | `u64` | A fixed number (`MAGIC_ROTATOR`) checked on every call. |
| `shared` | `Arc<Shared>` | The shared runtime state. |
| `rotator` | `Rotator` | The Rust rotator handle. |
| `_keep` | `Keep` | Keeps the transport's monitor alive for the life of the handle. |
| `virtual_control` | `Option<VirtualRotatorControl>` | The served virtual rotator behind a virtual handle, stopped on close. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `rotator_ref` (src/ffi.rs:261), `rctl_rotator_close` (src/ffi.rs:965), `rctl_rotator_stop` (src/ffi.rs:1043), `rctl_rotator_park` (src/ffi.rs:1060).

**See also:** test `a_virtual_rotator_through_the_c_api_moves_stops_parks_and_refuses_out_of_travel (tests/ffi.rs)`.

### `Fail` [@ANCHOR: radioctl:radioctl_capi_ffi_fail]

**Purpose and location.** A failure on its way to a status code and an error message for `rctl_last_error`. Defined in `daemons/radioctl/crates/radioctl-capi/src/ffi.rs:122` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `code` | `i32` | The negative `RCTL_ERR_*` status code returned to the C caller. |
| `message` | `String` | The text stored as the thread's last error. |

**Invariants, ownership and concurrency.** Built by `fail`, converted to the return code by `run`, which also turns a caught panic into `RCTL_ERR_INTERNAL`; never crosses the C boundary itself.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `fail` (src/ffi.rs:128), `run` (src/ffi.rs:153), `from_rig_error` (src/ffi.rs:172), `from_station_error` (src/ffi.rs:195), `str_arg` (src/ffi.rs:214), `runtime_ref` (src/ffi.rs:233), `rig_ref` (src/ffi.rs:247), `rotator_ref` (src/ffi.rs:261).

**See also:** test `the_wrong_kind_of_handle_and_a_panic_are_errors_not_crashes (tests/ffi.rs)`.
