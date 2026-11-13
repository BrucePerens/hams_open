# radioctl function anchors, batch 55

Function anchors for the `radioctl-capi` crate, the C ABI of radioctl, all in `ffi.rs`: the status codes and helpers that keep panics and raw pointers inside the boundary, the runtime calls, the rig calls (open, close, frequency, mode, PTT: push-to-talk, transmit gate, link state) and the rotator calls. The exported functions are `extern "C"` with the `rctl_` prefix; the private helpers are the Rust side of the same rules. Conventions: calls return an `int` status (0 is `RCTL_OK`, negative values are `RCTL_ERR_*`), results come back through out-pointers that are written only on success, strings passed in are borrowed NUL-terminated UTF-8, and the last error message is per thread (`rctl_last_error`). Calls block the calling thread on the runtime and must not be made from the runtime's own tasks. Handles are used by one thread at a time; each carries a magic number that catches most stale handles.

Transmit safety in this batch: a rig opened through this API starts with the transmit gate closed. The only exported function that can key a transmitter is `rctl_rig_set_ptt` with a nonzero argument, and it is refused with `RCTL_ERR_TX_GATE_CLOSED` until `rctl_rig_permit_transmit` has been called on that rig with a non-empty `granted_by`. Unkeying is always allowed. `rctl_rig_close` unkeys (best effort) and closes the gate before releasing the rig. Frequency and mode changes are not transmits and work with the gate closed. Rotator calls never key a transmitter. The API exposes no raw command and no Morse.

## `crates/radioctl-capi/src/ffi.rs`

### `fn connected(&self) -> bool` [@ANCHOR: radioctl:radioctl_capi_keep_connected]

Method on the private `Keep` enum, which holds the transport monitor that a rig or rotator handle keeps alive. It answers whether the transport, if there is one, reports itself connected. `Keep::Nothing` (the virtual rig) returns true; `Keep::Net` and `Keep::Serial` ask their monitor for its `LinkStatus` and return true only for `LinkStatus::Connected`. `self` is borrowed and nothing is changed. No errors, no panics, no blocking. `rctl_rig_link_up` combines this with the actor's own link flag, so a reconnecting transport reads as down.

### `fn fail(code: i32, message: impl Into<String>) -> Fail` [@ANCHOR: radioctl:radioctl_capi_fail]

Builds the private `Fail` value (status code plus message) that every helper returns on error and that `run` turns into a status code and the thread's last-error text. `code` is one of the `RCTL_ERR_*` constants; `message` is any `Into<String>` and is taken by value. Pure, infallible, no I/O, no panics. It is never exported and never crosses the C boundary; C callers only see the code and `rctl_last_error`.

### `fn set_error(message: &str)` [@ANCHOR: radioctl:radioctl_capi_set_error]

Stores the message that `rctl_last_error` will return on the calling thread. `message` is borrowed; interior NUL characters are replaced by spaces so the text can be a C string, and the copy is kept in a thread-local `CString`. The previous message of that thread is overwritten and its pointer becomes invalid. Other threads are not affected. No errors and no blocking; it is called by `run` after every entry point that uses it, with an empty string on success.

### `fn run(body: impl FnOnce() -> Result<(), Fail>) -> i32` [@ANCHOR: radioctl:radioctl_capi_run]

The wrapper inside which every fallible exported function runs its body, so that no Rust panic unwinds into C. `body` is called once under `catch_unwind`. On `Ok` it clears the thread's error message and returns `RCTL_OK` (0); on `Err(Fail)` it records the message and returns the failure's code; on a caught panic it records a fixed internal-error message and returns `RCTL_ERR_INTERNAL`. It does no I/O itself and has no locks; the body may block the calling thread. `rctl_abi_version`, `rctl_version` and `rctl_last_error` do not use it, so they leave the error message untouched.

### `fn from_rig_error(e: &RigError) -> Fail` [@ANCHOR: radioctl:radioctl_capi_from_rig_error]

Maps a radioctl-core `RigError` to the C status code and message. Timeout becomes `RCTL_ERR_TIMEOUT`; Rejected and TransmitFrequencyUnknown become `RCTL_ERR_REJECTED`; Unsupported becomes `RCTL_ERR_UNSUPPORTED`; LinkDown and Cancelled become `RCTL_ERR_LINK_DOWN`; TransmitGateClosed becomes `RCTL_ERR_TX_GATE_CLOSED`; Malformed becomes `RCTL_ERR_PROTOCOL`; QueueFull becomes `RCTL_ERR_BUSY`. The message is the error's detail text, and for a closed gate it is extended with a hint to call `rctl_rig_permit_transmit` first. `e` is borrowed; pure, no I/O, no panics.

### `unsafe fn str_arg<'a>(ptr: *const c_char, what: &str, max: usize) -> Result<&'a str, Fail>` [@ANCHOR: radioctl:radioctl_capi_str_arg]

Turns a borrowed C string argument into a Rust `&str` without copying. `ptr` must be null or point to a NUL-terminated string that stays valid and unmodified for the duration of the call; the library never keeps it. `what` is the argument's name, used in messages; `max` is the longest accepted length in bytes. Null gives `RCTL_ERR_NULL` (message `<what> is null`); bytes that are not valid UTF-8 or a length over `max` give `RCTL_ERR_INVALID_ARG`. An empty string is accepted here; callers that forbid it check themselves. No ownership transfers, no blocking.

### `unsafe fn runtime_ref<'a>(ptr: *const RctlRuntime) -> Result<&'a RctlRuntime, Fail>` [@ANCHOR: radioctl:radioctl_capi_runtime_ref]

Checks a runtime handle given by C and returns a reference to it. `ptr` must be null or a pointer returned by this library that has not been freed. Null gives `RCTL_ERR_NULL`; a struct whose magic number is not the runtime magic gives `RCTL_ERR_BAD_HANDLE` (the magic is zeroed on free, so most use-after-free mistakes are caught, but this is a best-effort check and not a guarantee, since the memory may already be reused). Ownership stays with the caller. No locks, no blocking.

### `unsafe fn rig_ref<'a>(ptr: *const RctlRig) -> Result<&'a RctlRig, Fail>` [@ANCHOR: radioctl:radioctl_capi_rig_ref]

Checks a rig handle given by C and returns a reference to it. `ptr` must be null or a pointer returned by an `rctl_rig_open_*` call that has not been closed. Null gives `RCTL_ERR_NULL` (`rig is null`); a wrong magic number gives `RCTL_ERR_BAD_HANDLE` (`not a live rig handle`). The check is best effort because a freed pointer may already point at reused memory. Ownership stays with the caller; every rig entry point calls this first. No blocking, no side effects.

### `fn out<T>(ptr: *mut T, what: &str) -> Result<*mut T, Fail>` [@ANCHOR: radioctl:radioctl_capi_out]

Validates an out-pointer argument. `ptr` is the caller's destination for a result and `what` names it for the message. A null pointer gives `RCTL_ERR_NULL` with the message `<what> is null`; otherwise the same pointer is returned for the caller to write through in an unsafe block. It checks only for null: alignment and writability are the C caller's promise. No ownership transfer, no side effects.

### `fn vfo(code: c_int) -> Result<Vfo, Fail>` [@ANCHOR: radioctl:radioctl_capi_vfo]

Converts the C integer VFO (variable frequency oscillator, the tuning control) selector into the core `Vfo`: 0 is VFO A, 1 is VFO B. Any other value gives `RCTL_ERR_INVALID_ARG` with the message `vfo is 0 (A) or 1 (B)`. Pure, no I/O, no panics. Used by `rctl_rig_get_frequency` and `rctl_rig_set_frequency`.

### `pub fn runtime_from_handle(handle: Handle) -> *mut RctlRuntime` [@ANCHOR: radioctl:radioctl_capi_runtime_from_handle]

A Rust-only constructor (not in the C header) for a host that already runs a tokio runtime and wants C callers to open rigs on it. `handle` is the tokio `Handle` of that runtime; it is moved in and kept. The result is a heap-allocated `RctlRuntime` whose ownership passes to the caller, who must release it with `rctl_runtime_free` once every rig and rotator opened on it is closed. The library does not own the underlying runtime, so freeing never shuts it down. The open-rig count starts at zero. Cannot fail. Calls made through it block the calling thread on that runtime, so they must not be made from the runtime's own worker tasks.

### `pub extern "C" fn rctl_abi_version() -> u32` [@ANCHOR: radioctl:radioctl_capi_abi_version]

C: `uint32_t rctl_abi_version(void)`. Returns `RCTL_ABI_VERSION`, currently 1, which changes only when an existing function changes incompatibly. A C caller should compare it with the version it was built against before using anything else. No arguments, no pointers, no allocation, no errors, thread safe, never blocks, and it does not touch the thread's last-error message. It cannot key a transmitter.

### `pub extern "C" fn rctl_version() -> *const c_char` [@ANCHOR: radioctl:radioctl_capi_version]

C: `const char *rctl_version(void)`. Returns the crate version (from Cargo) as a static NUL-terminated string. The pointer is owned by the library, never null, valid for the life of the process and must not be freed or written. No arguments, no errors, thread safe, never blocks, and it leaves the last-error message alone. It cannot key a transmitter.

### `pub extern "C" fn rctl_last_error() -> *const c_char` [@ANCHOR: radioctl:radioctl_capi_last_error]

C: `const char *rctl_last_error(void)`. Returns the message of the most recent call on the calling thread that used the error wrapper, or an empty string after a success or before any call. The error state is per thread, so a message from one thread is never seen by another and calls on different threads do not overwrite each other. The pointer is owned by the library, is never null, and is valid only until the next library call that sets the message on the same thread; copy it if needed. Do not free it. It never blocks and does not itself change the message. It cannot key a transmitter.

### `pub unsafe extern "C" fn rctl_runtime_new(out_runtime: *mut *mut RctlRuntime) -> i32` [@ANCHOR: radioctl:radioctl_capi_runtime_new]

C: `int rctl_runtime_new(rctl_runtime **out_runtime)`. Only present with the `owned-runtime` feature (on by default). Builds a multi-thread tokio runtime with two worker threads named `radioctl-capi` and writes a new handle to `*out_runtime`; the caller owns it and must release it with `rctl_runtime_free` after closing every rig and rotator. Errors: null `out_runtime` gives `RCTL_ERR_NULL`; a failure to build the runtime gives `RCTL_ERR_INTERNAL`; a panic gives `RCTL_ERR_INTERNAL`. `*out_runtime` is untouched on failure. Creates threads but otherwise returns promptly. It cannot key a transmitter.

### `pub unsafe extern "C" fn rctl_runtime_free(runtime: *mut RctlRuntime) -> i32` [@ANCHOR: radioctl:radioctl_capi_runtime_free]

C: `int rctl_runtime_free(rctl_runtime *runtime)`. Frees a runtime handle and, for a runtime built by `rctl_runtime_new`, shuts the owned tokio runtime down when the last reference goes. Null is an error (`RCTL_ERR_NULL`), not a no-op; a pointer with the wrong magic gives `RCTL_ERR_BAD_HANDLE`; if any rig or rotator opened on it is still open it returns `RCTL_ERR_BUSY` and the handle stays valid. After success the pointer is dangling and must not be used again; freeing twice is undefined behaviour. No other thread may be using the handle. Do not call it from inside the runtime's own tasks. It cannot key a transmitter.

### `fn open_with(rt: &RctlRuntime, toml: &str, slot: *mut *mut RctlRig, connect: impl FnOnce(&Handle, RigDescription) -> Result<(RigHandle, Keep), Fail>) -> Result<(), Fail>` [@ANCHOR: radioctl:radioctl_capi_open_with]

Shared body of the three rig open calls. It checks the out-slot for null (`RCTL_ERR_NULL`), parses `toml` as a rig description file (a parse failure is `RCTL_ERR_INVALID_ARG` with the parser's message), then runs the `connect` closure, which builds the engine and transport and spawns the rig actor, and returns its error unchanged. On success the runtime's open-handle count is incremented, a boxed `RctlRig` is created and its pointer is written to `*slot`; ownership goes to the caller, who closes it with `rctl_rig_close`. The rig is opened with the transmit gate closed. The closure may block on the runtime while connecting.

### `fn options() -> RigOptions` [@ANCHOR: radioctl:radioctl_capi_options]

Builds the actor options used for every rig opened through the C API: the library defaults for everything except the per-command timeout, which is set to 3 seconds. The default transmit time limit of the core options is kept, so a rig that is keyed through this API is still subject to the actor's maximum transmit time. No arguments, no I/O, no errors. The TCP and serial open calls then add a link watch to the returned value.

### `pub unsafe extern "C" fn rctl_rig_open_virtual(runtime: *mut RctlRuntime, rigfile_toml: *const c_char, out_rig: *mut *mut RctlRig) -> i32` [@ANCHOR: radioctl:radioctl_capi_rig_open_virtual]

C: `int rctl_rig_open_virtual(rctl_runtime *runtime, const char *rigfile_toml, rctl_rig **out_rig)`. Opens a rig on a simulated radio built from the description, with nothing on the air. `runtime` is a live handle; `rigfile_toml` is borrowed NUL-terminated UTF-8 text of a rig description file, at most 1 MiB; `out_rig` receives a handle the caller owns and must close with `rctl_rig_close`. Errors: `RCTL_ERR_NULL`, `RCTL_ERR_BAD_HANDLE`, `RCTL_ERR_INVALID_ARG` (not UTF-8, too long, bad description), plus mapped rig errors if the engine or virtual rig cannot be built. Blocks the calling thread briefly. The transmit gate starts closed, and since the radio is virtual keying it cannot reach a real transmitter.

### `pub unsafe extern "C" fn rctl_rig_open_tcp(runtime: *mut RctlRuntime, rigfile_toml: *const c_char, address: *const c_char, out_rig: *mut *mut RctlRig) -> i32` [@ANCHOR: radioctl:radioctl_capi_rig_open_tcp]

C: `int rctl_rig_open_tcp(rctl_runtime *runtime, const char *rigfile_toml, const char *address, rctl_rig **out_rig)`. Opens a rig over TCP (a serial server, a radio's own CAT (computer-aided transceiver control) port, a FlexRadio). `address` is borrowed `host:port` text of at most 255 bytes; `rigfile_toml` as for the virtual open; `out_rig` receives a handle the caller owns. The link reconnects with backoff, the link status feeds the actor, and the radio is unkeyed on every reconnect. Errors: null or bad handles, `RCTL_ERR_INVALID_ARG`, and mapped rig errors from `open_link` (such as `RCTL_ERR_LINK_DOWN` or `RCTL_ERR_TIMEOUT`). It blocks the calling thread while the first connection is made. The gate starts closed, so this call cannot key the radio; keying needs `rctl_rig_permit_transmit` and then `rctl_rig_set_ptt`.

### `pub unsafe extern "C" fn rctl_rig_open_serial(runtime: *mut RctlRuntime, rigfile_toml: *const c_char, path: *const c_char, baud: u32, out_rig: *mut *mut RctlRig) -> i32` [@ANCHOR: radioctl:radioctl_capi_rig_open_serial]

C: `int rctl_rig_open_serial(rctl_runtime *runtime, const char *rigfile_toml, const char *path, uint32_t baud, rctl_rig **out_rig)`. Opens a rig on a serial device path (at most 4096 bytes, such as `/dev/ttyUSB0`) at `baud`, 8N1. The port is reopened if it disappears, the radio is unkeyed when it returns and the modem control lines are made inactive after every open. A `baud` of 0 gives `RCTL_ERR_INVALID_ARG`; other errors are as for the TCP open, with mapped rig errors for a port that cannot be opened. Strings are borrowed; `out_rig` receives an owned handle. Blocks while the port is opened. The gate starts closed, so this call does not key the radio.

### `pub unsafe extern "C" fn rctl_rig_close(rig: *mut RctlRig) -> i32` [@ANCHOR: radioctl:radioctl_capi_rig_close]

C: `int rctl_rig_close(rctl_rig *rig)`. Ends a rig handle. If the rig can set PTT (push to talk) it first sends an unkey command, waiting at most 2 seconds and ignoring the outcome, then closes the transmit gate, then frees the handle and releases the actor and transport; the runtime's open count drops by one. Null is `RCTL_ERR_NULL` (not a no-op); a wrong magic is `RCTL_ERR_BAD_HANDLE`. After success the pointer is dangling; closing twice is undefined behaviour, and no other thread may use the handle. Blocks the calling thread up to about 2 seconds. It only ever unkeys; it cannot key a transmitter.

### `pub unsafe extern "C" fn rctl_rig_get_frequency(rig: *mut RctlRig, vfo_code: c_int, out_hz: *mut u64) -> i32` [@ANCHOR: radioctl:radioctl_capi_rig_get_frequency]

C: `int rctl_rig_get_frequency(rctl_rig *rig, int vfo, uint64_t *out_hz)`. Reads a VFO frequency in hertz from the radio itself (a fresh read, not a cached value). `vfo` is 0 for A or 1 for B; anything else is `RCTL_ERR_INVALID_ARG`. `out_hz` is written only on success. Other errors: `RCTL_ERR_NULL`, `RCTL_ERR_BAD_HANDLE`, and mapped rig errors (timeout, link down, unsupported, protocol). Blocks the calling thread until the radio answers or the 3 second default timeout. Calls on one rig are serialised by the actor. Read only; it cannot key a transmitter.

### `pub unsafe extern "C" fn rctl_rig_set_frequency(rig: *mut RctlRig, vfo_code: c_int, hz: u64) -> i32` [@ANCHOR: radioctl:radioctl_capi_rig_set_frequency]

C: `int rctl_rig_set_frequency(rctl_rig *rig, int vfo, uint64_t hz)`. Sets a VFO frequency in hertz. `vfo` is 0 (A) or 1 (B); `hz` of 0 is refused with `RCTL_ERR_INVALID_ARG`, as is any other vfo value. Other errors are mapped rig errors: `RCTL_ERR_UNSUPPORTED`, `RCTL_ERR_REJECTED`, `RCTL_ERR_TIMEOUT`, `RCTL_ERR_LINK_DOWN`. Blocks until the command completes or times out. It is idempotent. Changing frequency is not a transmit and works with the gate closed, so it does not key the radio, but it does change where a later transmission would go.

### `pub unsafe extern "C" fn rctl_rig_get_mode(rig: *mut RctlRig, buf: *mut c_char, len: usize) -> i32` [@ANCHOR: radioctl:radioctl_capi_rig_get_mode]

C: `int rctl_rig_get_mode(rctl_rig *rig, char *buf, size_t len)`. Reads the mode from the radio (fresh) and writes its lower-case name (`usb`, `lsb`, `cw`, `cw-r`, `am`, `fm`, `rtty` and others) into `buf` as a NUL-terminated string. `buf` is caller-owned and must be writable for `len` bytes. If the name plus its NUL does not fit in `len` the call returns `RCTL_ERR_BUFFER_TOO_SMALL` with the needed size in the error message and writes nothing. Other errors: `RCTL_ERR_NULL`, `RCTL_ERR_BAD_HANDLE`, mapped rig errors. Blocks until the radio answers or times out. Read only; cannot key a transmitter.

### `pub unsafe extern "C" fn rctl_rig_set_mode(rig: *mut RctlRig, name: *const c_char) -> i32` [@ANCHOR: radioctl:radioctl_capi_rig_set_mode]

C: `int rctl_rig_set_mode(rctl_rig *rig, const char *name)`. Sets the mode by name, the same names `rctl_rig_get_mode` returns. `name` is borrowed NUL-terminated UTF-8, at most 32 bytes, and is matched after lower-casing. An unknown name, non-UTF-8 or an over-long name is `RCTL_ERR_INVALID_ARG`; null pointers are `RCTL_ERR_NULL`; other errors are mapped rig errors (unsupported mode, rejected, timeout, link down). Blocks until the command completes. Idempotent. It is not a transmit and works with the gate closed; it cannot key the radio.

### `pub unsafe extern "C" fn rctl_rig_get_ptt(rig: *mut RctlRig, out_on: *mut c_int) -> i32` [@ANCHOR: radioctl:radioctl_capi_rig_get_ptt]

C: `int rctl_rig_get_ptt(rctl_rig *rig, int *out_on)`. Asks the radio whether it is transmitting and writes 1 or 0 to `*out_on` (written only on success). Errors: `RCTL_ERR_NULL`, `RCTL_ERR_BAD_HANDLE`, and mapped rig errors (unsupported, timeout, link down, protocol). Blocks until the radio answers or times out. Read only; it neither keys the radio nor depends on the transmit gate.

### `pub unsafe extern "C" fn rctl_rig_set_ptt(rig: *mut RctlRig, on: c_int) -> i32` [@ANCHOR: radioctl:radioctl_capi_rig_set_ptt]

C: `int rctl_rig_set_ptt(rctl_rig *rig, int on)`. The only call that can key a transmitter. `on` nonzero keys, zero unkeys. Keying is refused with `RCTL_ERR_TX_GATE_CLOSED` unless `rctl_rig_permit_transmit` was called on this rig, and may be refused by the actor for other reasons (for example `RCTL_ERR_REJECTED` when the transmit frequency is unknown, `RCTL_ERR_UNSUPPORTED`, `RCTL_ERR_LINK_DOWN`, `RCTL_ERR_TIMEOUT`). Unkeying is always allowed, even with the gate closed or the link in doubt. Blocks until the radio confirms or the timeout passes. Repeating the same value is harmless. The actor's transmit time limit still applies after keying.

### `pub unsafe extern "C" fn rctl_rig_permit_transmit(rig: *mut RctlRig, granted_by: *const c_char) -> i32` [@ANCHOR: radioctl:radioctl_capi_rig_permit_transmit]

C: `int rctl_rig_permit_transmit(rctl_rig *rig, const char *granted_by)`. Opens this rig's transmit gate; nothing else in the C API does. `granted_by` is borrowed NUL-terminated UTF-8 of at most 256 bytes naming who decided that transmitting is allowed (an operator, an attestation, a test); it is recorded in the permit. Empty or whitespace-only text, or text containing control characters, is `RCTL_ERR_INVALID_ARG`; null is `RCTL_ERR_NULL`. It does not itself key the radio and does not block on the radio. Calling it again replaces the permit. The gate stays open until `rctl_rig_revoke_transmit` or `rctl_rig_close`.

### `pub unsafe extern "C" fn rctl_rig_revoke_transmit(rig: *mut RctlRig) -> i32` [@ANCHOR: radioctl:radioctl_capi_rig_revoke_transmit]

C: `int rctl_rig_revoke_transmit(rctl_rig *rig)`. Closes the transmit gate again, so later keying is refused with `RCTL_ERR_TX_GATE_CLOSED`. Always allowed, idempotent, and it does not unkey a radio that is already transmitting: call `rctl_rig_set_ptt(rig, 0)` for that. Errors are only `RCTL_ERR_NULL` and `RCTL_ERR_BAD_HANDLE`. It does not block on the radio and cannot key a transmitter.

### `pub unsafe extern "C" fn rctl_rig_transmit_permitted(rig: *mut RctlRig, out_open: *mut c_int) -> i32` [@ANCHOR: radioctl:radioctl_capi_rig_transmit_permitted]

C: `int rctl_rig_transmit_permitted(rctl_rig *rig, int *out_open)`. Reports whether the rig's transmit gate is open: 1 when a permit is held, 0 when closed. `out_open` must be writable and is written only on success. Errors: `RCTL_ERR_NULL` and `RCTL_ERR_BAD_HANDLE`. Local state only, no radio traffic, does not block. Read only; it cannot key a transmitter, and a 1 does not mean the radio is transmitting.

### `pub unsafe extern "C" fn rctl_rig_link_up(rig: *mut RctlRig, out_up: *mut c_int) -> i32` [@ANCHOR: radioctl:radioctl_capi_rig_link_up]

C: `int rctl_rig_link_up(rctl_rig *rig, int *out_up)`. Reports whether the link to the radio is up: 1 only if the actor says the link is up and the transport monitor, when there is one, reports connected; otherwise 0. `out_up` is written only on success. Errors: `RCTL_ERR_NULL`, `RCTL_ERR_BAD_HANDLE`. Local state, no radio traffic, does not block. A virtual rig has no transport monitor, so it follows the actor alone. Read only; cannot key a transmitter.

### `fn from_station_error(e: &StationError) -> Fail` [@ANCHOR: radioctl:radioctl_capi_from_station_error]

Maps a radioctl-station error from the rotator layer to a C status code. Config and Unknown kinds become `RCTL_ERR_INVALID_ARG`; Refused, KeyedUnknown and NotConfirmed become `RCTL_ERR_REJECTED`; Timeout becomes `RCTL_ERR_TIMEOUT`; Driver becomes `RCTL_ERR_LINK_DOWN` when its detail text contains the word `link` and `RCTL_ERR_PROTOCOL` otherwise; Store becomes `RCTL_ERR_INTERNAL`. The message is the error's detail. `e` is borrowed; pure, no I/O. The substring test on `link` is a heuristic on the text, not a typed kind.

### `unsafe fn rotator_ref<'a>(ptr: *const RctlRotator) -> Result<&'a RctlRotator, Fail>` [@ANCHOR: radioctl:radioctl_capi_rotator_ref]

Checks a rotator handle given by C and returns a reference. `ptr` must be null or a pointer returned by an `rctl_rotator_open_*` call and not closed. Null gives `RCTL_ERR_NULL` (`rotator is null`); a wrong magic gives `RCTL_ERR_BAD_HANDLE` (`not a live rotator handle`). Best effort only for freed memory. Ownership stays with the caller; every rotator entry point calls it first. No blocking, no side effects.

### `fn rotator_description(id: &str) -> Result<RotatorDescription, Fail>` [@ANCHOR: radioctl:radioctl_capi_rotator_description]

Looks up the shipped description of a rotator model by `id` (an id from `data/station/rotators.toml`, for example `yaesu-g-5500-gs232b`) among the built-in descriptions and returns a copy. `id` is borrowed and compared exactly. An unknown id gives `RCTL_ERR_INVALID_ARG` with the message `unknown rotator model`. No I/O beyond the built-in data, no blocking.

### `fn rotator_open_with(rt: &RctlRuntime, model: &str, slot: *mut *mut RctlRotator, connect: impl FnOnce(&Handle, RotatorDescription) -> Result<(Rotator, Keep, Option<VirtualRotatorControl>), Fail>) -> Result<(), Fail>` [@ANCHOR: radioctl:radioctl_capi_rotator_open_with]

Shared body of the three rotator open calls. It checks the out-slot for null (`RCTL_ERR_NULL`), finds the model description (`RCTL_ERR_INVALID_ARG` if unknown), runs `connect` to get the `Rotator`, the transport monitor and the optional served virtual rotator control, and returns the closure's error unchanged. On success it counts the rotator with the rigs on the runtime (so `rctl_runtime_free` is refused while it is open), boxes an `RctlRotator` and writes it to `*slot`; the caller owns it and closes it with `rctl_rotator_close`. May block on the runtime while connecting.

### `pub unsafe extern "C" fn rctl_rotator_open_virtual(runtime: *mut RctlRuntime, model: *const c_char, out_rotator: *mut *mut RctlRotator) -> i32` [@ANCHOR: radioctl:radioctl_capi_rotator_open_virtual]

C: `int rctl_rotator_open_virtual(rctl_runtime *runtime, const char *model, rctl_rotator **out_rotator)`. Opens a rotator of the given model on a simulated rotator that the library serves on a loopback socket (127.0.0.1, an ephemeral port), with no hardware. `model` is borrowed UTF-8, at most 255 bytes; `out_rotator` receives an owned handle for `rctl_rotator_close`, which also stops the served virtual rotator. Errors: null or bad handles, `RCTL_ERR_INVALID_ARG` for an unknown model, `RCTL_ERR_INTERNAL` if the socket cannot be bound, mapped rig errors if the link cannot open. Blocks during setup. It involves no transmitter.

### `pub unsafe extern "C" fn rctl_rotator_open_tcp(runtime: *mut RctlRuntime, model: *const c_char, address: *const c_char, out_rotator: *mut *mut RctlRotator) -> i32` [@ANCHOR: radioctl:radioctl_capi_rotator_open_tcp]

C: `int rctl_rotator_open_tcp(rctl_runtime *runtime, const char *model, const char *address, rctl_rotator **out_rotator)`. Opens a rotator of the given model over TCP (a rotctld, or a serial server in front of a controller), reconnecting with backoff. `address` is borrowed `host:port` text of at most 255 bytes; `model` as for the virtual open. `out_rotator` receives an owned handle. Errors: `RCTL_ERR_NULL`, `RCTL_ERR_BAD_HANDLE`, `RCTL_ERR_INVALID_ARG`, and mapped link errors from the connect. Blocks during the first connection. A rotator is not a transmitter and this call does not key anything.

### `pub unsafe extern "C" fn rctl_rotator_open_serial(runtime: *mut RctlRuntime, model: *const c_char, path: *const c_char, baud: u32, out_rotator: *mut *mut RctlRotator) -> i32` [@ANCHOR: radioctl:radioctl_capi_rotator_open_serial]

C: `int rctl_rotator_open_serial(rctl_runtime *runtime, const char *model, const char *path, uint32_t baud, rctl_rotator **out_rotator)`. Opens a rotator of the given model on a serial device path (at most 4096 bytes) at `baud`, 8N1, reopening the port if it disappears. A `baud` of 0 is `RCTL_ERR_INVALID_ARG`; other errors as for the TCP open. All strings are borrowed; `out_rotator` receives an owned handle. Blocks while the port opens. It does not key a transmitter.

### `pub unsafe extern "C" fn rctl_rotator_close(rotator: *mut RctlRotator) -> i32` [@ANCHOR: radioctl:radioctl_capi_rotator_close]

C: `int rctl_rotator_close(rctl_rotator *rotator)`. Closes a rotator: it is told to stop, waiting at most 2 seconds and ignoring the outcome, a served virtual rotator (from the virtual open) is stopped, and the handle and transport are released; the runtime's open count drops by one. Null is `RCTL_ERR_NULL`; a wrong magic is `RCTL_ERR_BAD_HANDLE`. After success the pointer is dangling, closing twice is undefined behaviour, and no other thread may use the handle. Blocks up to about 2 seconds plus the virtual rotator shutdown. No transmitter is involved.

### `pub unsafe extern "C" fn rctl_rotator_set_position(rotator: *mut RctlRotator, az: f32, el: f32) -> i32` [@ANCHOR: radioctl:radioctl_capi_rotator_set_position]

C: `int rctl_rotator_set_position(rctl_rotator *rotator, float az, float el)`. Moves the rotator to azimuth `az` and elevation `el`, both in degrees. A position outside the model's travel is refused with `RCTL_ERR_REJECTED` before anything is written. Other errors: null or bad handle, and mapped station errors (timeout, link down, protocol). Blocks until the rotator has accepted the command; it does not wait for the move to finish. Repeating the call re-sends the target. It does not key a transmitter and no transmit gate applies.

### `pub unsafe extern "C" fn rctl_rotator_get_position(rotator: *mut RctlRotator, out_az: *mut f32, out_el: *mut f32) -> i32` [@ANCHOR: radioctl:radioctl_capi_rotator_get_position]

C: `int rctl_rotator_get_position(rctl_rotator *rotator, float *out_az, float *out_el)`. Asks the rotator for its current azimuth and elevation in degrees and writes both, only on success. Both pointers must be non-null and writable; null gives `RCTL_ERR_NULL`. Other errors are a bad handle and mapped station errors (timeout, link down, protocol). Blocks until the rotator answers or times out. Read only; no transmitter involved.

### `pub unsafe extern "C" fn rctl_rotator_stop(rotator: *mut RctlRotator) -> i32` [@ANCHOR: radioctl:radioctl_capi_rotator_stop]

C: `int rctl_rotator_stop(rctl_rotator *rotator)`. Tells the rotator to stop moving. No interlock applies, so it is always attempted. Errors: null or bad handle, and mapped station errors (timeout, link down, protocol) if the command fails. Blocks until the rotator accepts the command or times out. Idempotent. It does not key a transmitter.

### `pub unsafe extern "C" fn rctl_rotator_park(rotator: *mut RctlRotator) -> i32` [@ANCHOR: radioctl:radioctl_capi_rotator_park]

C: `int rctl_rotator_park(rctl_rotator *rotator)`. Sends the rotator to its park position. A model that has no park position is refused with `RCTL_ERR_REJECTED`. Other errors: null or bad handle and mapped station errors. Blocks until the command is accepted, not until the rotator arrives. Idempotent. It does not key a transmitter and no transmit gate applies.
