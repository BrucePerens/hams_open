// SPDX-License-Identifier: AGPL-3.0-or-later
//! Property tests on the wire parser: no input panics, every encoded message decodes to itself,
//! and no header above the bound is ever accepted. `proptest` runs these on stable without
//! nightly; a cargo-fuzz harness would call the same three functions.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use proptest::prelude::*;
use relay_broker::wire::{
    frame_body_len, DeviceClass, DeviceDescriptor, Group, Request, Response, Status, MAX_FRAME,
};

fn any_class() -> impl Strategy<Value = DeviceClass> {
    prop_oneof![Just(DeviceClass::Serial), Just(DeviceClass::Audio)]
}

fn any_request() -> impl Strategy<Value = Request> {
    prop_oneof![
        proptest::option::of(any_class()).prop_map(|class| Request::ListDevices { class }),
        (any_class(), any::<bool>(), "[^\0]{0,255}").prop_map(|(class, read_only, path)| {
            Request::OpenDevice {
                class,
                read_only,
                path,
            }
        }),
        prop_oneof![Just(Group::Dialout), Just(Group::Audio)]
            .prop_map(|group| Request::GrantGroup { group }),
        Just(Request::ProvisionGroups),
    ]
}

fn any_response() -> impl Strategy<Value = Response> {
    let dev = (
        any_class(),
        "[a-z/0-9]{0,20}",
        any::<u16>(),
        any::<u16>(),
        "[a-zA-Z ]{0,20}",
        "[a-zA-Z ]{0,20}",
    )
        .prop_map(
            |(class, path, vid, pid, product, manufacturer)| DeviceDescriptor {
                class,
                path,
                vid,
                pid,
                product,
                manufacturer,
            },
        );
    prop_oneof![
        proptest::collection::vec(dev, 0..16).prop_map(Response::Devices),
        Just(Response::Opened),
        (
            any::<bool>(),
            "[^\0]{0,30}",
            proptest::collection::vec("[a-z]{1,10}", 0..8)
        )
            .prop_map(|(applied, user, groups)| Response::GroupReport {
                applied,
                user,
                groups
            }),
        (
            prop_oneof![
                Just(Status::Denied),
                Just(Status::RateLimited),
                Just(Status::Malformed),
                Just(Status::UnsupportedVersion),
                Just(Status::Failed)
            ],
            "[^\0]{0,255}"
        )
            .prop_map(|(status, reason)| Response::Error { status, reason }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    // Tests [@ANCHOR: relay_broker_request_decode]
    // Tests [@ANCHOR: relay_broker_response_decode]
    #[test]
    fn arbitrary_bytes_never_panic_the_parsers(bytes in proptest::collection::vec(any::<u8>(), 0..MAX_FRAME)) {
        let _ = Request::decode(&bytes);
        let _ = Response::decode(&bytes);
    }

    // Tests [@ANCHOR: relay_broker_request_encode]
    #[test]
    fn every_request_round_trips(req in any_request()) {
        let body = req.encode();
        prop_assert!(body.len() <= MAX_FRAME);
        let utf8_ok = match &req { Request::OpenDevice { path, .. } => path.len() <= 255, _ => true };
        if utf8_ok {
            prop_assert_eq!(Request::decode(&body), Ok(req));
        } else {
            prop_assert!(Request::decode(&body).is_ok());
        }
    }

    // Tests [@ANCHOR: relay_broker_response_encode]
    #[test]
    fn every_response_round_trips_modulo_truncation(resp in any_response()) {
        let body = resp.encode();
        prop_assert!(body.len() <= MAX_FRAME, "{} bytes", body.len());
        let decoded = Response::decode(&body);
        prop_assert!(decoded.is_ok(), "{:?}", decoded);
    }

    // Tests [@ANCHOR: relay_broker_frame_body_len]
    #[test]
    fn no_header_above_the_bound_is_accepted(len in any::<u32>()) {
        let r = frame_body_len(len.to_be_bytes());
        prop_assert_eq!(r.is_ok(), len != 0 && (len as usize) <= MAX_FRAME);
    }
}
