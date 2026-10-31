// SPDX-License-Identifier: AGPL-3.0-or-later
//! The exported functions. This is the only module that contains `unsafe` code: raw pointers cross
//! the boundary here and are checked for null, for the magic number, and for valid UTF-8 before
//! anything is done with them.
#![allow(unsafe_code)]

use std::cell::RefCell;
use std::ffi::{c_char, c_int, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use radioctl_core::capability::Capability;
use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::net::{open_link, NetConfig, NetMonitor, RawFraming};
use radioctl_core::op::Op;
use radioctl_core::rig::{spawn, Freshness, RigHandle, RigOptions, TransmitPermit};
use radioctl_core::rigfile;
use radioctl_core::serial::{open_serial, LinkStatus, PortTarget, SerialConfig, SerialMonitor};
use radioctl_core::units::{Hz, Mode, Vfo};
use radioctl_protocols::conformance::engine_for;
use radioctl_protocols::io::spawn_virtual_rig;
use radioctl_protocols::virtual_rig::VirtualRig;
use tokio::runtime::Handle;

/// The version of this ABI. It changes only when an existing function changes incompatibly.
pub const RCTL_ABI_VERSION: u32 = 1;

/// Success.
pub const RCTL_OK: i32 = 0;
/// A required pointer argument was null.
pub const RCTL_ERR_NULL: i32 = -1;
/// An argument was invalid (not UTF-8, empty, out of range, not a known name).
pub const RCTL_ERR_INVALID_ARG: i32 = -2;
/// The handle is not a live handle of this library.
pub const RCTL_ERR_BAD_HANDLE: i32 = -3;
/// The rig or its description does not support the operation.
pub const RCTL_ERR_UNSUPPORTED: i32 = -4;
/// The link to the rig is down.
pub const RCTL_ERR_LINK_DOWN: i32 = -5;
/// The rig did not answer in time.
pub const RCTL_ERR_TIMEOUT: i32 = -6;
/// The rig refused the command.
pub const RCTL_ERR_REJECTED: i32 = -7;
/// A transmit was refused because the transmit gate is closed: call `rctl_rig_permit_transmit`.
pub const RCTL_ERR_TX_GATE_CLOSED: i32 = -8;
/// The object is in use (a runtime with open rigs, a command queue that is full).
pub const RCTL_ERR_BUSY: i32 = -9;
/// The caller's buffer is too small.
pub const RCTL_ERR_BUFFER_TOO_SMALL: i32 = -10;
/// The rig sent something that could not be understood.
pub const RCTL_ERR_PROTOCOL: i32 = -11;
/// An internal error, including a caught panic.
pub const RCTL_ERR_INTERNAL: i32 = -12;

const MAGIC_RUNTIME: u64 = 0x5243_544C_5254_4D45; // "RCTLRTME"
const MAGIC_RIG: u64 = 0x5243_544C_5249_4720; // "RCTLRIG "
const MAX_TEXT: usize = 1 << 20;
const MAX_WHO: usize = 256;

struct Shared {
    handle: Handle,
    /// The runtime this library built, if it did.
    _owned: Option<tokio::runtime::Runtime>,
    rigs: AtomicUsize,
}

/// An opaque runtime handle (`rctl_runtime`): the tokio runtime that drives rigs.
pub struct RctlRuntime {
    magic: u64,
    shared: Arc<Shared>,
}

enum Keep {
    Nothing,
    Net(NetMonitor),
    Serial(SerialMonitor),
}

impl Keep {
    // [@ANCHOR: radioctl_capi_keep_connected]
    /// Whether the transport, when it reports a status, says it is connected.
    fn connected(&self) -> bool {
        let status = match self {
            Keep::Nothing => return true,
            Keep::Net(m) => m.status(),
            Keep::Serial(m) => m.status(),
        };
        matches!(status, LinkStatus::Connected { .. })
    }
}

/// An opaque rig handle (`rctl_rig`): one open rig.
pub struct RctlRig {
    magic: u64,
    shared: Arc<Shared>,
    handle: RigHandle,
    keep: Keep,
}

/// A failure on its way to a status code and an error message.
struct Fail {
    code: i32,
    message: String,
}

// [@ANCHOR: radioctl_capi_fail]
fn fail(code: i32, message: impl Into<String>) -> Fail {
    Fail {
        code,
        message: message.into(),
    }
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

// [@ANCHOR: radioctl_capi_set_error]
/// Records the message for this thread (interior NULs are replaced).
fn set_error(message: &str) {
    let clean: String = message
        .chars()
        .map(|c| if c == '\0' { ' ' } else { c })
        .collect();
    let text = CString::new(clean).unwrap_or_default();
    LAST_ERROR.with(|e| *e.borrow_mut() = text);
}

// [@ANCHOR: radioctl_capi_run]
/// Runs one entry point's body: panics are caught, the error message is set or cleared, and the
/// status code is returned.
fn run(body: impl FnOnce() -> Result<(), Fail>) -> i32 {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(())) => {
            set_error("");
            RCTL_OK
        }
        Ok(Err(f)) => {
            set_error(&f.message);
            f.code
        }
        Err(_) => {
            set_error("internal error: a panic was caught at the C boundary");
            RCTL_ERR_INTERNAL
        }
    }
}

// [@ANCHOR: radioctl_capi_from_rig_error]
/// A rig error as a status code.
fn from_rig_error(e: &RigError) -> Fail {
    let code = match e.kind {
        ErrorKind::Timeout => RCTL_ERR_TIMEOUT,
        ErrorKind::Rejected => RCTL_ERR_REJECTED,
        ErrorKind::Unsupported => RCTL_ERR_UNSUPPORTED,
        ErrorKind::LinkDown | ErrorKind::Cancelled => RCTL_ERR_LINK_DOWN,
        ErrorKind::TransmitGateClosed => RCTL_ERR_TX_GATE_CLOSED,
        ErrorKind::Malformed => RCTL_ERR_PROTOCOL,
        ErrorKind::QueueFull => RCTL_ERR_BUSY,
    };
    let message = if code == RCTL_ERR_TX_GATE_CLOSED {
        format!(
            "{}: the transmit gate is closed; call rctl_rig_permit_transmit first",
            e.detail
        )
    } else {
        e.detail.clone()
    };
    fail(code, message)
}

// [@ANCHOR: radioctl_capi_str_arg]
/// A borrowed C string as `&str`: not null, UTF-8, at most `max` bytes.
///
/// # Safety
/// `ptr` is null or points to a NUL-terminated string that stays valid for the call.
unsafe fn str_arg<'a>(ptr: *const c_char, what: &str, max: usize) -> Result<&'a str, Fail> {
    if ptr.is_null() {
        return Err(fail(RCTL_ERR_NULL, format!("{what} is null")));
    }
    // SAFETY: the caller guarantees a valid NUL-terminated string.
    let text = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| fail(RCTL_ERR_INVALID_ARG, format!("{what} is not UTF-8")))?;
    if text.len() > max {
        return Err(fail(RCTL_ERR_INVALID_ARG, format!("{what} is too long")));
    }
    Ok(text)
}

// [@ANCHOR: radioctl_capi_runtime_ref]
/// A live runtime handle as a reference.
///
/// # Safety
/// `ptr` is null or a pointer returned by this library that has not been freed.
unsafe fn runtime_ref<'a>(ptr: *const RctlRuntime) -> Result<&'a RctlRuntime, Fail> {
    // SAFETY: see above; the magic number catches most stale handles.
    let rt = unsafe { ptr.as_ref() }.ok_or_else(|| fail(RCTL_ERR_NULL, "runtime is null"))?;
    if rt.magic != MAGIC_RUNTIME {
        return Err(fail(RCTL_ERR_BAD_HANDLE, "not a live runtime handle"));
    }
    Ok(rt)
}

// [@ANCHOR: radioctl_capi_rig_ref]
/// A live rig handle as a reference.
///
/// # Safety
/// `ptr` is null or a pointer returned by this library that has not been freed.
unsafe fn rig_ref<'a>(ptr: *const RctlRig) -> Result<&'a RctlRig, Fail> {
    // SAFETY: see above; the magic number catches most stale handles.
    let rig = unsafe { ptr.as_ref() }.ok_or_else(|| fail(RCTL_ERR_NULL, "rig is null"))?;
    if rig.magic != MAGIC_RIG {
        return Err(fail(RCTL_ERR_BAD_HANDLE, "not a live rig handle"));
    }
    Ok(rig)
}

// [@ANCHOR: radioctl_capi_out]
/// Checks an out-pointer.
fn out<T>(ptr: *mut T, what: &str) -> Result<*mut T, Fail> {
    if ptr.is_null() {
        Err(fail(RCTL_ERR_NULL, format!("{what} is null")))
    } else {
        Ok(ptr)
    }
}

// [@ANCHOR: radioctl_capi_vfo]
/// 0 is VFO A and 1 is VFO B.
fn vfo(code: c_int) -> Result<Vfo, Fail> {
    match code {
        0 => Ok(Vfo::A),
        1 => Ok(Vfo::B),
        _ => Err(fail(RCTL_ERR_INVALID_ARG, "vfo is 0 (A) or 1 (B)")),
    }
}

/// A protocol engine behind a box, so one `spawn` serves every family.
struct BoxedEngine(Box<dyn ProtocolEngine>);

impl ProtocolEngine for BoxedEngine {
    // [@ANCHOR: radioctl_capi_boxed_description]
    fn description(&self) -> &RigDescription {
        self.0.description()
    }

    // [@ANCHOR: radioctl_capi_boxed_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        self.0.encode(op)
    }

    // [@ANCHOR: radioctl_capi_boxed_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        self.0.feed(bytes, now)
    }
}

// [@ANCHOR: radioctl_capi_runtime_from_handle]
/// A runtime object for a Rust host that already has a tokio runtime and wants a C caller to use
/// it. The returned pointer is owned by the caller and freed with `rctl_runtime_free`; this
/// library does not own the underlying runtime. Not part of the C header.
pub fn runtime_from_handle(handle: Handle) -> *mut RctlRuntime {
    Box::into_raw(Box::new(RctlRuntime {
        magic: MAGIC_RUNTIME,
        shared: Arc::new(Shared {
            handle,
            _owned: None,
            rigs: AtomicUsize::new(0),
        }),
    }))
}

/// The ABI version, [`RCTL_ABI_VERSION`].
#[no_mangle]
// [@ANCHOR: radioctl_capi_abi_version]
pub extern "C" fn rctl_abi_version() -> u32 {
    RCTL_ABI_VERSION
}

/// The library version as a static NUL-terminated string.
#[no_mangle]
// [@ANCHOR: radioctl_capi_version]
pub extern "C" fn rctl_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// The message of the last failed call on this thread, or an empty string. Valid until the next
/// call into the library on the same thread. Never null.
#[no_mangle]
// [@ANCHOR: radioctl_capi_last_error]
pub extern "C" fn rctl_last_error() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().as_ptr())
}

/// Builds a runtime that the library owns, with two worker threads. Only with the
/// `owned-runtime` feature. Free it with `rctl_runtime_free` after closing every rig.
///
/// # Safety
/// `out_runtime` must be a valid pointer to a `rctl_runtime *`.
#[cfg(feature = "owned-runtime")]
#[no_mangle]
// [@ANCHOR: radioctl_capi_runtime_new]
pub unsafe extern "C" fn rctl_runtime_new(out_runtime: *mut *mut RctlRuntime) -> i32 {
    run(|| {
        let slot = out(out_runtime, "out_runtime")?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("radioctl-capi")
            .enable_all()
            .build()
            .map_err(|e| fail(RCTL_ERR_INTERNAL, format!("cannot build a runtime: {e}")))?;
        let boxed = Box::new(RctlRuntime {
            magic: MAGIC_RUNTIME,
            shared: Arc::new(Shared {
                handle: runtime.handle().clone(),
                _owned: Some(runtime),
                rigs: AtomicUsize::new(0),
            }),
        });
        // SAFETY: `slot` was checked for null and the caller promises it is writable.
        unsafe { *slot = Box::into_raw(boxed) };
        Ok(())
    })
}

/// Frees a runtime. Refused with `RCTL_ERR_BUSY` while rigs opened on it are still open. Passing
/// null is an error, not a no-op.
///
/// # Safety
/// `runtime` must be a pointer returned by this library and not yet freed, and no other thread may
/// be using it.
#[no_mangle]
// [@ANCHOR: radioctl_capi_runtime_free]
pub unsafe extern "C" fn rctl_runtime_free(runtime: *mut RctlRuntime) -> i32 {
    run(|| {
        // SAFETY: the caller's contract.
        let rt = unsafe { runtime_ref(runtime) }?;
        if rt.shared.rigs.load(Ordering::Acquire) > 0 {
            return Err(fail(
                RCTL_ERR_BUSY,
                "rigs are still open on this runtime; close them first",
            ));
        }
        // SAFETY: the pointer came from Box::into_raw and the magic number says it is live.
        let mut boxed = unsafe { Box::from_raw(runtime) };
        boxed.magic = 0;
        drop(boxed);
        Ok(())
    })
}

// [@ANCHOR: radioctl_capi_open_with]
/// Common part of the open calls: parse the description, build the engine, and run `connect`
/// to get the actor's handle.
fn open_with(
    rt: &RctlRuntime,
    toml: &str,
    slot: *mut *mut RctlRig,
    connect: impl FnOnce(&Handle, RigDescription) -> Result<(RigHandle, Keep), Fail>,
) -> Result<(), Fail> {
    let slot = out(slot, "out_rig")?;
    let description = rigfile::parse(toml)
        .map_err(|e| fail(RCTL_ERR_INVALID_ARG, format!("rig description: {e}")))?
        .description;
    let (handle, keep) = connect(&rt.shared.handle, description)?;
    rt.shared.rigs.fetch_add(1, Ordering::AcqRel);
    let boxed = Box::new(RctlRig {
        magic: MAGIC_RIG,
        shared: Arc::clone(&rt.shared),
        handle,
        keep,
    });
    // SAFETY: `slot` was checked for null and the caller promises it is writable.
    unsafe { *slot = Box::into_raw(boxed) };
    Ok(())
}

// [@ANCHOR: radioctl_capi_options]
/// Actor options for every rig opened here: default timeouts, the default transmit time limit.
fn options() -> RigOptions {
    RigOptions {
        default_timeout: Duration::from_secs(3),
        ..RigOptions::default()
    }
}

/// Opens a rig on a virtual radio built from the description (no hardware, nothing on the air).
/// `rigfile_toml` is the text of a rig description file. The transmit gate starts closed.
///
/// # Safety
/// `runtime` must be a live runtime handle, `rigfile_toml` a NUL-terminated string and `out_rig` a
/// writable `rctl_rig *`.
#[no_mangle]
// [@ANCHOR: radioctl_capi_rig_open_virtual]
pub unsafe extern "C" fn rctl_rig_open_virtual(
    runtime: *mut RctlRuntime,
    rigfile_toml: *const c_char,
    out_rig: *mut *mut RctlRig,
) -> i32 {
    run(|| {
        // SAFETY: the caller's contract.
        let rt = unsafe { runtime_ref(runtime) }?;
        // SAFETY: the caller's contract.
        let toml = unsafe { str_arg(rigfile_toml, "rigfile_toml", MAX_TEXT) }?;
        open_with(rt, toml, out_rig, |handle, description| {
            let virtual_rig =
                VirtualRig::from_description(&description).map_err(|e| from_rig_error(&e))?;
            let engine = BoxedEngine(engine_for(&description).map_err(|e| from_rig_error(&e))?);
            let (stream, _control) = spawn_virtual_rig(handle, virtual_rig, Duration::ZERO);
            let rig = spawn(handle, engine, stream, options()).map_err(|e| from_rig_error(&e))?;
            Ok((rig, Keep::Nothing))
        })
    })
}

/// Opens a rig reached over TCP (a network serial server, a radio's own CAT port, a FlexRadio),
/// reconnecting with backoff and unkeying the radio on every reconnect. `address` is `host:port`.
/// The transmit gate starts closed.
///
/// # Safety
/// As for `rctl_rig_open_virtual`; `address` is a NUL-terminated string.
#[no_mangle]
// [@ANCHOR: radioctl_capi_rig_open_tcp]
pub unsafe extern "C" fn rctl_rig_open_tcp(
    runtime: *mut RctlRuntime,
    rigfile_toml: *const c_char,
    address: *const c_char,
    out_rig: *mut *mut RctlRig,
) -> i32 {
    run(|| {
        // SAFETY: the caller's contract.
        let rt = unsafe { runtime_ref(runtime) }?;
        // SAFETY: the caller's contract.
        let toml = unsafe { str_arg(rigfile_toml, "rigfile_toml", MAX_TEXT) }?;
        // SAFETY: the caller's contract.
        let address = unsafe { str_arg(address, "address", 255) }?.to_string();
        open_with(rt, toml, out_rig, |handle, description| {
            let engine = BoxedEngine(engine_for(&description).map_err(|e| from_rig_error(&e))?);
            let link = rt
                .shared
                .handle
                .block_on(open_link(
                    handle,
                    NetConfig::new(address),
                    Box::new(RawFraming),
                ))
                .map_err(|e| from_rig_error(&e))?;
            let mut opts = options();
            opts.link_watch = Some(link.watch());
            let (stream, monitor) = link.into_parts();
            let rig = spawn(handle, engine, stream, opts).map_err(|e| from_rig_error(&e))?;
            Ok((rig, Keep::Net(monitor)))
        })
    })
}

/// Opens a rig on a serial port (a device path such as `/dev/ttyUSB0`) at `baud`, 8N1, reopening
/// the port if it disappears and unkeying the radio when it comes back. The modem control lines
/// are made inactive after every open. The transmit gate starts closed.
///
/// # Safety
/// As for `rctl_rig_open_virtual`; `path` is a NUL-terminated string.
#[no_mangle]
// [@ANCHOR: radioctl_capi_rig_open_serial]
pub unsafe extern "C" fn rctl_rig_open_serial(
    runtime: *mut RctlRuntime,
    rigfile_toml: *const c_char,
    path: *const c_char,
    baud: u32,
    out_rig: *mut *mut RctlRig,
) -> i32 {
    run(|| {
        // SAFETY: the caller's contract.
        let rt = unsafe { runtime_ref(runtime) }?;
        // SAFETY: the caller's contract.
        let toml = unsafe { str_arg(rigfile_toml, "rigfile_toml", MAX_TEXT) }?;
        // SAFETY: the caller's contract.
        let path = unsafe { str_arg(path, "path", 4096) }?.to_string();
        if baud == 0 {
            return Err(fail(RCTL_ERR_INVALID_ARG, "baud must be positive"));
        }
        open_with(rt, toml, out_rig, |handle, description| {
            let engine = BoxedEngine(engine_for(&description).map_err(|e| from_rig_error(&e))?);
            let config = SerialConfig::new(PortTarget::Path(path.into()), baud);
            let link = rt
                .shared
                .handle
                .block_on(open_serial(handle, config))
                .map_err(|e| from_rig_error(&e))?;
            let mut opts = options();
            opts.link_watch = Some(link.watch());
            let (stream, monitor) = link.into_parts();
            let rig = spawn(handle, engine, stream, opts).map_err(|e| from_rig_error(&e))?;
            Ok((rig, Keep::Serial(monitor)))
        })
    })
}

/// Closes a rig. The radio is unkeyed first (best effort, bounded), then the actor and the
/// transport are released. Passing null is an error, not a no-op.
///
/// # Safety
/// `rig` must be a pointer returned by an `rctl_rig_open_*` call and not yet closed, and no other
/// thread may be using it.
#[no_mangle]
// [@ANCHOR: radioctl_capi_rig_close]
pub unsafe extern "C" fn rctl_rig_close(rig: *mut RctlRig) -> i32 {
    run(|| {
        // SAFETY: the caller's contract.
        let r = unsafe { rig_ref(rig) }?;
        if r.handle.capabilities().has(Capability::SetPtt) {
            let _ = r.shared.handle.block_on(async {
                tokio::time::timeout(
                    Duration::from_secs(2),
                    r.handle
                        .execute_within(Op::SetPtt(false), Duration::from_secs(2)),
                )
                .await
            });
        }
        r.handle.close_transmit_gate();
        // SAFETY: the pointer came from Box::into_raw and the magic number says it is live.
        let mut boxed = unsafe { Box::from_raw(rig) };
        boxed.magic = 0;
        boxed.shared.rigs.fetch_sub(1, Ordering::AcqRel);
        drop(boxed);
        Ok(())
    })
}

/// Reads a VFO's frequency in hertz (`vfo` is 0 for A, 1 for B). Asks the radio.
///
/// # Safety
/// `rig` must be a live rig handle and `out_hz` a writable `uint64_t`.
#[no_mangle]
// [@ANCHOR: radioctl_capi_rig_get_frequency]
pub unsafe extern "C" fn rctl_rig_get_frequency(
    rig: *mut RctlRig,
    vfo_code: c_int,
    out_hz: *mut u64,
) -> i32 {
    run(|| {
        // SAFETY: the caller's contract.
        let r = unsafe { rig_ref(rig) }?;
        let slot = out(out_hz, "out_hz")?;
        let v = vfo(vfo_code)?;
        let got = r
            .shared
            .handle
            .block_on(r.handle.frequency(v, Freshness::Fresh))
            .map_err(|e| from_rig_error(&e))?;
        // SAFETY: `slot` was checked for null and the caller promises it is writable.
        unsafe { *slot = got.value.0 };
        Ok(())
    })
}

/// Sets a VFO's frequency in hertz. Changing frequency is not a transmit; it works with the gate
/// closed.
///
/// # Safety
/// `rig` must be a live rig handle.
#[no_mangle]
// [@ANCHOR: radioctl_capi_rig_set_frequency]
pub unsafe extern "C" fn rctl_rig_set_frequency(
    rig: *mut RctlRig,
    vfo_code: c_int,
    hz: u64,
) -> i32 {
    run(|| {
        // SAFETY: the caller's contract.
        let r = unsafe { rig_ref(rig) }?;
        let v = vfo(vfo_code)?;
        if hz == 0 {
            return Err(fail(RCTL_ERR_INVALID_ARG, "frequency must be positive"));
        }
        r.shared
            .handle
            .block_on(r.handle.set_frequency(v, Hz(hz)))
            .map_err(|e| from_rig_error(&e))
    })
}

/// Reads the mode as its lower-case name (`usb`, `lsb`, `cw`, `cw-r`, `am`, `fm`, `rtty`, ...)
/// into `buf` as a NUL-terminated string.
///
/// # Safety
/// `rig` must be a live rig handle and `buf` writable for `len` bytes.
#[no_mangle]
// [@ANCHOR: radioctl_capi_rig_get_mode]
pub unsafe extern "C" fn rctl_rig_get_mode(rig: *mut RctlRig, buf: *mut c_char, len: usize) -> i32 {
    run(|| {
        // SAFETY: the caller's contract.
        let r = unsafe { rig_ref(rig) }?;
        let buf = out(buf, "buf")?;
        let got = r
            .shared
            .handle
            .block_on(r.handle.mode(Freshness::Fresh))
            .map_err(|e| from_rig_error(&e))?;
        let name = got.value.name();
        if name.len() + 1 > len {
            return Err(fail(
                RCTL_ERR_BUFFER_TOO_SMALL,
                format!("the mode name needs {} bytes", name.len() + 1),
            ));
        }
        // SAFETY: `buf` is writable for `len` bytes and `name.len() + 1 <= len`.
        unsafe {
            std::ptr::copy_nonoverlapping(name.as_ptr(), buf.cast::<u8>(), name.len());
            *buf.add(name.len()) = 0;
        }
        Ok(())
    })
}

/// Sets the mode by name (see `rctl_rig_get_mode`).
///
/// # Safety
/// `rig` must be a live rig handle and `name` a NUL-terminated string.
#[no_mangle]
// [@ANCHOR: radioctl_capi_rig_set_mode]
pub unsafe extern "C" fn rctl_rig_set_mode(rig: *mut RctlRig, name: *const c_char) -> i32 {
    run(|| {
        // SAFETY: the caller's contract.
        let r = unsafe { rig_ref(rig) }?;
        // SAFETY: the caller's contract.
        let name = unsafe { str_arg(name, "name", 32) }?;
        let mode = Mode::from_name(&name.to_ascii_lowercase())
            .ok_or_else(|| fail(RCTL_ERR_INVALID_ARG, format!("unknown mode \"{name}\"")))?;
        r.shared
            .handle
            .block_on(r.handle.set_mode(mode))
            .map_err(|e| from_rig_error(&e))
    })
}

/// Reads whether the radio is transmitting (`*out_on` is 1 or 0).
///
/// # Safety
/// `rig` must be a live rig handle and `out_on` a writable `int`.
#[no_mangle]
// [@ANCHOR: radioctl_capi_rig_get_ptt]
pub unsafe extern "C" fn rctl_rig_get_ptt(rig: *mut RctlRig, out_on: *mut c_int) -> i32 {
    run(|| {
        // SAFETY: the caller's contract.
        let r = unsafe { rig_ref(rig) }?;
        let slot = out(out_on, "out_on")?;
        let got = r
            .shared
            .handle
            .block_on(r.handle.ptt(Freshness::Fresh))
            .map_err(|e| from_rig_error(&e))?;
        // SAFETY: `slot` was checked for null and the caller promises it is writable.
        unsafe { *slot = c_int::from(got.value) };
        Ok(())
    })
}

/// Keys (`on` nonzero) or unkeys (`on` zero) the radio. **Keying is refused with
/// `RCTL_ERR_TX_GATE_CLOSED` unless `rctl_rig_permit_transmit` was called on this rig.** Unkeying is
/// always allowed, even with the gate closed or the link in doubt.
///
/// # Safety
/// `rig` must be a live rig handle.
#[no_mangle]
// [@ANCHOR: radioctl_capi_rig_set_ptt]
pub unsafe extern "C" fn rctl_rig_set_ptt(rig: *mut RctlRig, on: c_int) -> i32 {
    run(|| {
        // SAFETY: the caller's contract.
        let r = unsafe { rig_ref(rig) }?;
        r.shared
            .handle
            .block_on(r.handle.set_ptt(on != 0))
            .map_err(|e| from_rig_error(&e))
    })
}

/// The explicit permit call: opens this rig's transmit gate. `granted_by` names who decided that
/// transmitting is allowed (an operator, an attestation, a test); it is required, non-empty, at
/// most 256 bytes, and recorded. Nothing else in this API opens the gate.
///
/// # Safety
/// `rig` must be a live rig handle and `granted_by` a NUL-terminated string.
#[no_mangle]
// [@ANCHOR: radioctl_capi_rig_permit_transmit]
pub unsafe extern "C" fn rctl_rig_permit_transmit(
    rig: *mut RctlRig,
    granted_by: *const c_char,
) -> i32 {
    run(|| {
        // SAFETY: the caller's contract.
        let r = unsafe { rig_ref(rig) }?;
        // SAFETY: the caller's contract.
        let who = unsafe { str_arg(granted_by, "granted_by", MAX_WHO) }?;
        if who.trim().is_empty() || who.chars().any(char::is_control) {
            return Err(fail(
                RCTL_ERR_INVALID_ARG,
                "granted_by must say who decided, in plain text",
            ));
        }
        r.handle.open_transmit_gate(TransmitPermit::new(who));
        Ok(())
    })
}

/// Closes the transmit gate again. Always allowed.
///
/// # Safety
/// `rig` must be a live rig handle.
#[no_mangle]
// [@ANCHOR: radioctl_capi_rig_revoke_transmit]
pub unsafe extern "C" fn rctl_rig_revoke_transmit(rig: *mut RctlRig) -> i32 {
    run(|| {
        // SAFETY: the caller's contract.
        let r = unsafe { rig_ref(rig) }?;
        r.handle.close_transmit_gate();
        Ok(())
    })
}

/// Whether the transmit gate is open (`*out_open` is 1 or 0).
///
/// # Safety
/// `rig` must be a live rig handle and `out_open` a writable `int`.
#[no_mangle]
// [@ANCHOR: radioctl_capi_rig_transmit_permitted]
pub unsafe extern "C" fn rctl_rig_transmit_permitted(
    rig: *mut RctlRig,
    out_open: *mut c_int,
) -> i32 {
    run(|| {
        // SAFETY: the caller's contract.
        let r = unsafe { rig_ref(rig) }?;
        let slot = out(out_open, "out_open")?;
        // SAFETY: `slot` was checked for null and the caller promises it is writable.
        unsafe { *slot = c_int::from(r.handle.transmit_permit().is_some()) };
        Ok(())
    })
}

/// Whether the link to the radio is up (`*out_up` is 1 or 0).
///
/// # Safety
/// `rig` must be a live rig handle and `out_up` a writable `int`.
#[no_mangle]
// [@ANCHOR: radioctl_capi_rig_link_up]
pub unsafe extern "C" fn rctl_rig_link_up(rig: *mut RctlRig, out_up: *mut c_int) -> i32 {
    run(|| {
        // SAFETY: the caller's contract.
        let r = unsafe { rig_ref(rig) }?;
        let slot = out(out_up, "out_up")?;
        // SAFETY: `slot` was checked for null and the caller promises it is writable.
        unsafe { *slot = c_int::from(r.handle.link_up() && r.keep.connected()) };
        Ok(())
    })
}
