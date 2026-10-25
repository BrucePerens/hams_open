//! libusb-1.0 implementation of [`Transport`], including isochronous streaming.

use crate::transport::{PacketSink, Transport};
use crate::{model_for_pid, DeviceInfo, Error, VID_SDRPLAY};
use libusb1_sys as ffi;
use libusb1_sys::constants::*;
use std::ffi::CStr;
use std::os::raw::{c_int, c_uint, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const CTRL_TIMEOUT_MS: c_uint = 1000;
const EP_IN: u8 = 0x81;
const N_TRANSFERS: usize = 16;
const N_PACKETS: usize = 32;

fn err_name(code: c_int) -> String {
    unsafe { CStr::from_ptr(ffi::libusb_error_name(code)).to_string_lossy().into_owned() }
}

pub(crate) fn usb_err(what: &str, code: c_int) -> Error {
    Error::Usb { what: what.to_string(), code, name: err_name(code) }
}

/// Owned libusb context.
struct Context(*mut ffi::libusb_context);
unsafe impl Send for Context {}
unsafe impl Sync for Context {}
impl Context {
    fn new() -> Result<Arc<Context>, Error> {
        let mut c = ptr::null_mut();
        let r = unsafe { ffi::libusb_init(&mut c) };
        if r != 0 {
            return Err(usb_err("libusb_init", r));
        }
        Ok(Arc::new(Context(c)))
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        unsafe { ffi::libusb_exit(self.0) }
    }
}

fn with_devices<T>(ctx: &Context, mut f: impl FnMut(*mut ffi::libusb_device, &ffi::libusb_device_descriptor) -> Option<T>) -> Result<Vec<T>, Error> {
    let mut list = ptr::null();
    let n = unsafe { ffi::libusb_get_device_list(ctx.0, &mut list) };
    if n < 0 {
        return Err(usb_err("libusb_get_device_list", n as c_int));
    }
    let mut out = Vec::new();
    for i in 0..n as usize {
        let dev = unsafe { *list.add(i) };
        let mut d: ffi::libusb_device_descriptor = unsafe { std::mem::zeroed() };
        if unsafe { ffi::libusb_get_device_descriptor(dev, &mut d) } != 0 {
            continue;
        }
        if let Some(v) = f(dev, &d) {
            out.push(v);
        }
    }
    unsafe { ffi::libusb_free_device_list(list, 1) };
    Ok(out)
}

fn info_of(dev: *mut ffi::libusb_device, d: &ffi::libusb_device_descriptor) -> DeviceInfo {
    DeviceInfo {
        bus: unsafe { ffi::libusb_get_bus_number(dev) },
        address: unsafe { ffi::libusb_get_device_address(dev) },
        vid: d.idVendor,
        pid: d.idProduct,
        bcd_device: d.bcdDevice,
        model: model_for_pid(d.idProduct),
    }
}

/// List every SDRplay-VID device on the system (descriptors only; nothing is opened).
pub fn list_devices() -> Result<Vec<DeviceInfo>, Error> {
    let ctx = Context::new()?;
    with_devices(&ctx, |dev, d| (d.idVendor == VID_SDRPLAY).then(|| info_of(dev, d)))
}

struct Shared {
    sink: Mutex<PacketSink>,
    running: AtomicBool,
    active: AtomicUsize,
    packet_errors: AtomicU64,
    pkt_len: usize,
}

struct IsoStream {
    shared: Arc<Shared>,
    transfers: Vec<*mut ffi::libusb_transfer>,
    _buffers: Vec<Vec<u8>>,
    thread: Option<JoinHandle<()>>,
}
unsafe impl Send for IsoStream {}

extern "system" fn iso_callback(t: *mut ffi::libusb_transfer) {
    unsafe {
        let shared = &*((*t).user_data as *const Shared);
        let status = (*t).status;
        if status == LIBUSB_TRANSFER_COMPLETED {
            let n = (*t).num_iso_packets as usize;
            let descs = std::slice::from_raw_parts((*t).iso_packet_desc.as_ptr(), n);
            if let Ok(mut sink) = shared.sink.lock() {
                for (i, d) in descs.iter().enumerate() {
                    if d.status != LIBUSB_TRANSFER_COMPLETED {
                        shared.packet_errors.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    if d.actual_length == 0 {
                        continue;
                    }
                    let p = std::slice::from_raw_parts((*t).buffer.add(i * shared.pkt_len), d.actual_length as usize);
                    (sink)(p);
                }
            }
        }
        let resubmit = shared.running.load(Ordering::Acquire)
            && status != LIBUSB_TRANSFER_CANCELLED
            && status != LIBUSB_TRANSFER_NO_DEVICE;
        if !(resubmit && ffi::libusb_submit_transfer(t) == 0) {
            shared.active.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

/// A claimed RSP over libusb.
pub struct UsbTransport {
    ctx: Arc<Context>,
    handle: *mut ffi::libusb_device_handle,
    pub info: DeviceInfo,
    alt: u8,
    stream: Option<IsoStream>,
    released: bool,
}
unsafe impl Send for UsbTransport {}

struct CtxPtr(*mut ffi::libusb_context);
unsafe impl Send for CtxPtr {}

impl UsbTransport {
    /// Open the device at (`bus`, `address`), or the first SDRplay device with `pid` if `None`,
    /// and claim interface 0. Only descriptors are read before the caller checks the state.
    pub fn open(pid: u16, at: Option<(u8, u8)>) -> Result<UsbTransport, Error> {
        let ctx = Context::new()?;
        let mut found: Option<(*mut ffi::libusb_device, DeviceInfo)> = None;
        with_devices(&ctx, |dev, d| {
            let info = info_of(dev, d);
            let hit = d.idVendor == VID_SDRPLAY
                && d.idProduct == pid
                && at.map_or(true, |(b, a)| b == info.bus && a == info.address);
            if hit && found.is_none() {
                unsafe { ffi::libusb_ref_device(dev) };
                found = Some((dev, info));
            }
            None::<()>
        })?;
        let (dev, info) = found.ok_or(Error::NotFound)?;
        let mut handle = ptr::null_mut();
        let r = unsafe { ffi::libusb_open(dev, &mut handle) };
        unsafe { ffi::libusb_unref_device(dev) };
        if r != 0 {
            return Err(usb_err("libusb_open", r));
        }
        unsafe { ffi::libusb_set_auto_detach_kernel_driver(handle, 1) };
        let r = unsafe { ffi::libusb_claim_interface(handle, 0) };
        if r != 0 {
            unsafe { ffi::libusb_close(handle) };
            return Err(usb_err("libusb_claim_interface", r));
        }
        Ok(UsbTransport { ctx, handle, info, alt: 0, stream: None, released: false })
    }

    /// Serial number string (string descriptor 1, present only when the device is running).
    pub fn serial(&self) -> Option<String> {
        let mut buf = [0u8; 64];
        let n = unsafe { ffi::libusb_get_string_descriptor_ascii(self.handle, 1, buf.as_mut_ptr(), buf.len() as c_int) };
        (n > 0).then(|| String::from_utf8_lossy(&buf[..n as usize]).into_owned())
    }

    /// Isochronous packets that completed with an error status so far.
    pub fn packet_errors(&self) -> u64 {
        self.stream.as_ref().map_or(0, |s| s.shared.packet_errors.load(Ordering::Relaxed))
    }
}

impl Transport for UsbTransport {
    fn vendor_out(&mut self, request: u8, value: u16, index: u16) -> Result<(), Error> {
        let r = unsafe { ffi::libusb_control_transfer(self.handle, 0x40, request, value, index, ptr::null_mut(), 0, CTRL_TIMEOUT_MS) };
        if r < 0 {
            return Err(usb_err(&format!("vendor OUT 0x{request:02x}"), r));
        }
        Ok(())
    }

    fn vendor_in(&mut self, request: u8, value: u16, index: u16, buf: &mut [u8]) -> Result<usize, Error> {
        let r = unsafe {
            ffi::libusb_control_transfer(self.handle, 0xc0, request, value, index, buf.as_mut_ptr(), buf.len() as u16, CTRL_TIMEOUT_MS)
        };
        if r < 0 {
            return Err(usb_err(&format!("vendor IN 0x{request:02x}"), r));
        }
        Ok(r as usize)
    }

    fn set_alt(&mut self, alt: u8) -> Result<(), Error> {
        // Alt 3 (bulk) hung the device once (protocol document 1.10): never select it.
        if alt > 2 {
            return Err(Error::Unsupported(format!("alternate setting {alt} is not used by this driver")));
        }
        let r = unsafe { ffi::libusb_set_interface_alt_setting(self.handle, 0, alt as c_int) };
        if r != 0 {
            return Err(usb_err("set_interface_alt_setting", r));
        }
        self.alt = alt;
        Ok(())
    }

    fn start_iso(&mut self, sink: PacketSink) -> Result<(), Error> {
        if self.stream.is_some() {
            return Err(Error::InvalidArgument("stream already running".into()));
        }
        // Alt 1: high-bandwidth endpoint, 3 x 1024 per microframe; alt 2: 1 x 1024.
        let pkt_len = match self.alt {
            1 => 3 * 1024,
            2 => 1024,
            a => return Err(Error::InvalidArgument(format!("no isochronous endpoint in alt {a}"))),
        };
        let shared = Arc::new(Shared {
            sink: Mutex::new(sink),
            running: AtomicBool::new(true),
            active: AtomicUsize::new(0),
            packet_errors: AtomicU64::new(0),
            pkt_len,
        });
        let mut buffers: Vec<Vec<u8>> = (0..N_TRANSFERS).map(|_| vec![0u8; pkt_len * N_PACKETS]).collect();
        let mut transfers = Vec::new();
        for b in buffers.iter_mut() {
            let t = unsafe { ffi::libusb_alloc_transfer(N_PACKETS as c_int) };
            if t.is_null() {
                break;
            }
            unsafe {
                ffi::libusb_fill_iso_transfer(
                    t, self.handle, EP_IN, b.as_mut_ptr(), b.len() as c_int, N_PACKETS as c_int,
                    iso_callback, Arc::as_ptr(&shared) as *mut c_void, 1000,
                );
                ffi::libusb_set_iso_packet_lengths(t, pkt_len as c_uint);
            }
            transfers.push(t);
        }
        let mut submit_err = None;
        for &t in &transfers {
            shared.active.fetch_add(1, Ordering::AcqRel);
            let r = unsafe { ffi::libusb_submit_transfer(t) };
            if r != 0 {
                shared.active.fetch_sub(1, Ordering::AcqRel);
                submit_err = Some(usb_err("submit isochronous transfer", r));
                break;
            }
        }
        let ctx = CtxPtr(self.ctx.0);
        let sh = shared.clone();
        let _keep = self.ctx.clone();
        let thread = std::thread::Builder::new()
            .name("rsp_open-usb".into())
            .spawn(move || {
                let ctx = ctx;
                let _keep = _keep;
                let mut stop_seen: Option<Instant> = None;
                loop {
                    if sh.active.load(Ordering::Acquire) == 0 {
                        break;
                    }
                    if !sh.running.load(Ordering::Acquire) {
                        let t0 = *stop_seen.get_or_insert_with(Instant::now);
                        if t0.elapsed() > Duration::from_secs(3) {
                            break;
                        }
                    }
                    let tv = libc::timeval { tv_sec: 0, tv_usec: 50_000 };
                    unsafe { ffi::libusb_handle_events_timeout_completed(ctx.0, &tv, ptr::null_mut()) };
                }
            })
            .map_err(|e| Error::InvalidArgument(format!("cannot start USB thread: {e}")))?;
        self.stream = Some(IsoStream { shared, transfers, _buffers: buffers, thread: Some(thread) });
        if let Some(e) = submit_err {
            let _ = self.stop_iso();
            return Err(e);
        }
        Ok(())
    }

    fn stop_iso(&mut self) -> Result<(), Error> {
        let Some(mut s) = self.stream.take() else { return Ok(()) };
        s.shared.running.store(false, Ordering::Release);
        for &t in &s.transfers {
            unsafe { ffi::libusb_cancel_transfer(t) };
        }
        if let Some(th) = s.thread.take() {
            let _ = th.join();
        }
        if s.shared.active.load(Ordering::Acquire) == 0 {
            for &t in &s.transfers {
                unsafe { ffi::libusb_free_transfer(t) };
            }
        } else {
            // Transfers still owned by libusb: leak them (and their buffers) rather than free
            // memory the kernel may still write.
            std::mem::forget(s._buffers);
            std::mem::forget(s.shared);
            return Err(Error::Stream("isochronous transfers did not finish cancelling".into()));
        }
        Ok(())
    }

    fn release(&mut self) -> Result<(), Error> {
        if self.released {
            return Ok(());
        }
        self.stop_iso()?;
        let r = unsafe { ffi::libusb_release_interface(self.handle, 0) };
        unsafe { ffi::libusb_close(self.handle) };
        self.released = true;
        if r != 0 && r != LIBUSB_ERROR_NO_DEVICE {
            return Err(usb_err("release_interface", r));
        }
        Ok(())
    }
}

impl Drop for UsbTransport {
    fn drop(&mut self) {
        let _ = self.release();
    }
}
