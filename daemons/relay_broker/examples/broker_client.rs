// SPDX-License-Identifier: AGPL-3.0-or-later
//! A test client for a running broker.
//! `broker_client SOCKET list [serial|audio]`, `broker_client SOCKET open serial|audio PATH [ro]`,
//! `broker_client SOCKET grant dialout|audio`, `broker_client SOCKET provision`.
//! For `open`, the received descriptor's device numbers are printed, proving the pass worked.
#![allow(clippy::expect_used, clippy::panic)]

use std::os::fd::AsFd;
use std::path::Path;

use relay_broker::client::connect_and_call;
use relay_broker::os::{LinuxOs, Os};
use relay_broker::wire::{DeviceClass, Group, Request, Response};

fn class(s: &str) -> Option<DeviceClass> {
    match s {
        "serial" => Some(DeviceClass::Serial),
        "audio" => Some(DeviceClass::Audio),
        _ => None,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let usage = "usage: broker_client SOCKET list [serial|audio] | open serial|audio PATH [ro] | grant dialout|audio | provision";
    let socket = args.get(1).expect(usage);
    let req = match (
        args.get(2).map(String::as_str),
        args.get(3),
        args.get(4),
        args.get(5),
    ) {
        (Some("list"), c, None, None) => Request::ListDevices {
            class: c.and_then(|c| class(c)),
        },
        (Some("open"), Some(c), Some(path), ro) => Request::OpenDevice {
            class: class(c).expect(usage),
            read_only: ro.is_some_and(|r| r == "ro"),
            path: path.clone(),
        },
        (Some("grant"), Some(g), None, None) => Request::GrantGroup {
            group: match g.as_str() {
                "dialout" => Group::Dialout,
                "audio" => Group::Audio,
                _ => panic!("{usage}"),
            },
        },
        (Some("provision"), None, None, None) => Request::ProvisionGroups,
        _ => panic!("{usage}"),
    };
    let (resp, fd) = connect_and_call(Path::new(socket), &req).expect("broker call");
    match resp {
        Response::Devices(devs) => {
            for d in devs {
                println!(
                    "{:?} {} {:04x}:{:04x} {}",
                    d.class, d.path, d.vid, d.pid, d.product
                );
            }
        }
        Response::Opened => {
            let fd = fd.expect("descriptor");
            println!("opened: {:?}", LinuxOs::default().fd_kind(fd.as_fd()));
        }
        Response::GroupReport {
            applied,
            user,
            groups,
        } => {
            println!(
                "{user}: {} {}",
                if applied { "added" } else { "would add" },
                groups.join(",")
            );
        }
        Response::Error { status, reason } => {
            eprintln!("{status:?}: {reason}");
            std::process::exit(1);
        }
    }
}
