// SPDX-License-Identifier: AGPL-3.0-or-later
//! A failed `rctl_rotator_open_virtual` must not leave its loopback rotator server running.
//! Alone in its own test binary because it lowers the process file-descriptor limit.
#![allow(clippy::expect_used, clippy::panic, unsafe_code)]

use std::ffi::CString;
use std::ptr;

use radioctl_capi::ffi::*;

fn fds() -> Vec<usize> {
    let mut all: Vec<usize> = std::fs::read_dir("/proc/self/fd")
        .expect("fd dir")
        .filter_map(|e| {
            let e = e.ok()?;
            // The directory handle being read is not one of the process's own descriptors.
            if std::fs::read_link(e.path()).ok()?.starts_with("/proc") {
                return None;
            }
            e.file_name().to_str()?.parse().ok()
        })
        .collect();
    all.sort_unstable();
    all
}

/// The limit that leaves room for exactly `spare` more descriptors: the lowest free numbers.
fn limit_for(spare: usize) -> usize {
    let used = fds();
    let mut free = Vec::new();
    let mut n = 0;
    while free.len() < spare {
        if !used.contains(&n) {
            free.push(n);
        }
        n += 1;
    }
    free.last().map_or(0, |last| last + 1)
}

// Tests [@ANCHOR: radioctl_capi_rotator_open_virtual]
#[test]
fn failed_virtual_open_stops_the_loopback_server() {
    let mut rt: *mut RctlRuntime = ptr::null_mut();
    // SAFETY: a valid out pointer.
    assert_eq!(unsafe { rctl_runtime_new(&mut rt) }, RCTL_OK);
    let model = CString::new("yaesu-g-5500-gs232b").expect("no NUL");
    // Warm up so lazily created descriptors exist, then allow exactly one more: the listener
    // takes it and the client socket of the link fails with EMFILE.
    let mut warm: *mut RctlRotator = ptr::null_mut();
    // SAFETY: live runtime, NUL-terminated model, valid out pointer.
    assert_eq!(
        unsafe { rctl_rotator_open_virtual(rt, model.as_ptr(), &mut warm) },
        RCTL_OK
    );
    // SAFETY: the handle was just opened.
    assert_eq!(unsafe { rctl_rotator_close(warm) }, RCTL_OK);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let before = fds().len();
    let mut old = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: plain libc calls with valid pointers.
    unsafe {
        assert_eq!(libc::getrlimit(libc::RLIMIT_NOFILE, &mut old), 0);
        let low = libc::rlimit {
            rlim_cur: limit_for(1) as libc::rlim_t,
            rlim_max: old.rlim_max,
        };
        assert_eq!(libc::setrlimit(libc::RLIMIT_NOFILE, &low), 0);
    }
    let mut rot: *mut RctlRotator = ptr::null_mut();
    // SAFETY: live runtime, NUL-terminated model, valid out pointer.
    let status = unsafe { rctl_rotator_open_virtual(rt, model.as_ptr(), &mut rot) };
    std::thread::sleep(std::time::Duration::from_millis(300));
    // SAFETY: restoring the limit read above.
    unsafe { assert_eq!(libc::setrlimit(libc::RLIMIT_NOFILE, &old), 0) };
    let after = fds().len();
    assert_ne!(
        status, RCTL_OK,
        "the open must fail with no descriptors left"
    );
    assert!(rot.is_null());
    assert_eq!(after, before, "the loopback listener was left open");
}
