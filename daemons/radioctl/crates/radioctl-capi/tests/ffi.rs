// SPDX-License-Identifier: AGPL-3.0-or-later
//! The exported functions called from Rust as a C caller would call them, against a virtual rig
//! on a loopback socket that can be cut: the transmit gate (closed by default, opened only by the
//! permit call, closed again on close), unkeying on close and after a reconnect, runtime ownership,
//! a host-supplied runtime, and the guards (null, wrong handle type, a panic caught at the
//! boundary). Nothing is keyed except the virtual rig; nothing leaves the machine.
#![allow(clippy::expect_used, clippy::panic, unsafe_code)]

use std::ffi::{c_char, c_int, CStr, CString};
use std::ptr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use radioctl_capi::ffi::*;
use radioctl_core::description::RigDescription;
use radioctl_protocols::virtual_rig::VirtualRig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

const KX3: &str = include_str!("../../../rigs/elecraft-kx3.toml");
const TS2000: &str = include_str!("../../../rigs/kenwood-ts-2000.toml");

fn last_error() -> String {
    // SAFETY: rctl_last_error never returns null and the string is valid until the next call.
    unsafe { CStr::from_ptr(rctl_last_error()) }
        .to_string_lossy()
        .into_owned()
}

fn c(text: &str) -> CString {
    CString::new(text).expect("no NUL")
}

/// A virtual Kenwood on a killable TCP listener.
struct Server {
    rig: Arc<Mutex<VirtualRig>>,
    task: Option<JoinHandle<()>>,
    addr: std::net::SocketAddr,
}

impl Server {
    async fn start() -> Server {
        let rig = VirtualRig::from_description(&RigDescription::ts2000()).expect("rig");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let mut server = Server {
            rig: Arc::new(Mutex::new(rig)),
            task: None,
            addr: listener.local_addr().expect("addr"),
        };
        server.serve(listener);
        server
    }

    fn serve(&mut self, listener: TcpListener) {
        let rig = Arc::clone(&self.rig);
        self.task = Some(tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buffer = [0u8; 256];
                while let Ok(n) = stream.read(&mut buffer).await {
                    if n == 0 {
                        break;
                    }
                    let reply = rig
                        .lock()
                        .expect("rig")
                        .feed(buffer.get(..n).unwrap_or(&[]), Duration::ZERO);
                    if !reply.is_empty() && stream.write_all(&reply).await.is_err() {
                        break;
                    }
                }
            }
        }));
    }

    async fn kill(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
    }

    async fn restart(&mut self) {
        let listener = TcpListener::bind(self.addr).await.expect("rebind");
        self.serve(listener);
    }

    fn keyed(&self) -> bool {
        self.rig.lock().expect("rig").state().ptt
    }
}

fn wait(what: &str, mut check: impl FnMut() -> bool) {
    for _ in 0..400 {
        if check() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("never happened: {what}");
}

// Tests [@ANCHOR: radioctl_capi_run]
// Tests [@ANCHOR: radioctl_capi_set_error]
// Tests [@ANCHOR: radioctl_capi_fail]
// Tests [@ANCHOR: radioctl_capi_from_rig_error]
// Tests [@ANCHOR: radioctl_capi_str_arg]
// Tests [@ANCHOR: radioctl_capi_runtime_ref]
// Tests [@ANCHOR: radioctl_capi_rig_ref]
// Tests [@ANCHOR: radioctl_capi_out]
// Tests [@ANCHOR: radioctl_capi_vfo]
// Tests [@ANCHOR: radioctl_capi_open_with]
// Tests [@ANCHOR: radioctl_capi_options]
// Tests [@ANCHOR: radioctl_capi_rig_open_tcp]
// Tests [@ANCHOR: radioctl_capi_rig_set_ptt]
// Tests [@ANCHOR: radioctl_capi_rig_permit_transmit]
// Tests [@ANCHOR: radioctl_capi_rig_close]
#[test]
fn a_tcp_rig_through_the_c_api_is_unkeyed_on_reconnect_and_on_close() {
    let tokio_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let mut server = tokio_rt.block_on(Server::start());
    let addr = c(&server.addr.to_string());
    let toml = c(TS2000);
    // SAFETY (whole block): every pointer is valid for the call or null on purpose.
    unsafe {
        let mut rt: *mut RctlRuntime = ptr::null_mut();
        assert_eq!(rctl_runtime_new(&mut rt), RCTL_OK);
        let mut rig: *mut RctlRig = ptr::null_mut();
        assert_eq!(
            rctl_rig_open_tcp(rt, toml.as_ptr(), addr.as_ptr(), &mut rig),
            RCTL_OK,
            "{}",
            last_error()
        );
        let mut hz = 0u64;
        assert_eq!(rctl_rig_set_frequency(rig, 0, 14_074_000), RCTL_OK);
        assert_eq!(rctl_rig_get_frequency(rig, 0, &mut hz), RCTL_OK);
        assert_eq!(hz, 14_074_000);

        // Closed by default: nothing keys.
        assert_eq!(rctl_rig_set_ptt(rig, 1), RCTL_ERR_TX_GATE_CLOSED);
        assert!(last_error().contains("rctl_rig_permit_transmit"));
        assert!(!server.keyed());
        assert_eq!(
            rctl_rig_permit_transmit(rig, c("ffi test").as_ptr()),
            RCTL_OK
        );
        assert_eq!(rctl_rig_set_ptt(rig, 1), RCTL_OK);
        wait("keyed", || server.keyed());

        // The network dies while keyed; the library knows.
        tokio_rt.block_on(server.kill());
        let mut up: c_int = 1;
        wait("link down", || {
            assert_eq!(rctl_rig_link_up(rig, &mut up), RCTL_OK);
            up == 0
        });
        assert_eq!(
            rctl_rig_get_frequency(rig, 0, &mut hz),
            RCTL_ERR_LINK_DOWN,
            "{}",
            last_error()
        );
        assert!(server.keyed());

        // It comes back; the reconnect unkeys before anything else, with no call from C.
        tokio_rt.block_on(server.restart());
        wait("unkeyed by the reconnect", || !server.keyed());
        wait("link up", || {
            assert_eq!(rctl_rig_link_up(rig, &mut up), RCTL_OK);
            up == 1
        });
        assert_eq!(rctl_rig_get_frequency(rig, 0, &mut hz), RCTL_OK);

        // Key again, then close: the radio is unkeyed on the way out and the gate is shut.
        assert_eq!(rctl_rig_set_ptt(rig, 1), RCTL_OK);
        wait("keyed again", || server.keyed());
        assert_eq!(rctl_rig_close(rig), RCTL_OK);
        wait("unkeyed by close", || !server.keyed());
        assert_eq!(rctl_runtime_free(rt), RCTL_OK);
    }
    tokio_rt.block_on(server.kill());
}

// Tests [@ANCHOR: radioctl_capi_runtime_from_handle]
// Tests [@ANCHOR: radioctl_capi_rig_open_virtual]
// Tests [@ANCHOR: radioctl_capi_rig_revoke_transmit]
// Tests [@ANCHOR: radioctl_capi_rig_transmit_permitted]
#[test]
fn a_host_supplied_runtime_owns_nothing_the_c_side_must_build() {
    let host = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("runtime");
    let rt = runtime_from_handle(host.handle().clone());
    let toml = c(KX3);
    // SAFETY (whole block): valid pointers, or null on purpose.
    unsafe {
        let mut rig: *mut RctlRig = ptr::null_mut();
        assert_eq!(rctl_rig_open_virtual(rt, toml.as_ptr(), &mut rig), RCTL_OK);
        let mut open: c_int = -1;
        assert_eq!(rctl_rig_transmit_permitted(rig, &mut open), RCTL_OK);
        assert_eq!(open, 0, "the gate starts closed");
        assert_eq!(rctl_rig_permit_transmit(rig, c("host").as_ptr()), RCTL_OK);
        assert_eq!(rctl_rig_transmit_permitted(rig, &mut open), RCTL_OK);
        assert_eq!(open, 1);
        assert_eq!(rctl_rig_revoke_transmit(rig), RCTL_OK);
        assert_eq!(rctl_rig_transmit_permitted(rig, &mut open), RCTL_OK);
        assert_eq!(open, 0);
        assert_eq!(rctl_runtime_free(rt), RCTL_ERR_BUSY);
        assert_eq!(rctl_rig_close(rig), RCTL_OK);
        assert_eq!(rctl_runtime_free(rt), RCTL_OK);
    }
}

// Tests [@ANCHOR: radioctl_capi_run]
#[test]
fn the_wrong_kind_of_handle_and_a_panic_are_errors_not_crashes() {
    let toml = c(KX3);
    // SAFETY (whole block): valid pointers, or deliberately mismatched live handles.
    unsafe {
        let mut rt: *mut RctlRuntime = ptr::null_mut();
        assert_eq!(rctl_runtime_new(&mut rt), RCTL_OK);
        let mut rig: *mut RctlRig = ptr::null_mut();
        assert_eq!(rctl_rig_open_virtual(rt, toml.as_ptr(), &mut rig), RCTL_OK);
        // A rig where a runtime is expected, and the reverse.
        assert_eq!(
            rctl_runtime_free(rig.cast::<RctlRuntime>()),
            RCTL_ERR_BAD_HANDLE
        );
        let mut hz = 0u64;
        assert_eq!(
            rctl_rig_get_frequency(rt.cast::<RctlRig>(), 0, &mut hz),
            RCTL_ERR_BAD_HANDLE
        );
        // A null out-pointer, a bad vfo, a non-UTF-8 name.
        assert_eq!(
            rctl_rig_get_frequency(rig, 0, ptr::null_mut()),
            RCTL_ERR_NULL
        );
        assert_eq!(
            rctl_rig_get_frequency(rig, 2, &mut hz),
            RCTL_ERR_INVALID_ARG
        );
        let bad: [c_char; 3] = [-1, -2, 0];
        assert_eq!(rctl_rig_set_mode(rig, bad.as_ptr()), RCTL_ERR_INVALID_ARG);
        assert!(last_error().contains("UTF-8"));
        assert_eq!(
            rctl_rig_permit_transmit(rig, c("a\nb").as_ptr()),
            RCTL_ERR_INVALID_ARG
        );

        // Calling a blocking function from inside the runtime's own async context panics in
        // tokio; the boundary turns that into an error code.
        let host = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let rig_addr = rig as usize;
        let code = host.block_on(async move {
            let mut hz = 0u64;
            rctl_rig_get_frequency(rig_addr as *mut RctlRig, 0, &mut hz)
        });
        assert_eq!(code, RCTL_ERR_INTERNAL);
        assert!(last_error().contains("panic"));
        // The rig still works afterwards.
        assert_eq!(rctl_rig_get_frequency(rig, 0, &mut hz), RCTL_OK);
        assert_eq!(rctl_rig_close(rig), RCTL_OK);
        assert_eq!(rctl_runtime_free(rt), RCTL_OK);
    }
}

// Tests [@ANCHOR: radioctl_capi_rig_open_serial]
#[test]
fn a_serial_open_that_cannot_succeed_is_a_clean_error() {
    let toml = c(KX3);
    // SAFETY (whole block): valid pointers.
    unsafe {
        let mut rt: *mut RctlRuntime = ptr::null_mut();
        assert_eq!(rctl_runtime_new(&mut rt), RCTL_OK);
        let mut rig: *mut RctlRig = ptr::null_mut();
        let path = c("/dev/null/not-a-port");
        assert_eq!(
            rctl_rig_open_serial(rt, toml.as_ptr(), path.as_ptr(), 38_400, &mut rig),
            RCTL_ERR_LINK_DOWN
        );
        assert!(rig.is_null());
        assert_eq!(rctl_runtime_free(rt), RCTL_OK);
    }
}
