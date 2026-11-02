// SPDX-License-Identifier: AGPL-3.0-or-later
//! Kenwood `KY` (send CW text) per model (Bruce's decision of 2026-11-02): used only on a model
//! whose manual documents it, in that manual's form, width, character table and stop; every other
//! Kenwood-dialect model reports "not supported". Every description in `rigs/` goes through its
//! engine and the virtual rig, and the actor's closed transmit gate is checked. No radio is
//! involved and nothing is keyed.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::Duration;

use radioctl_core::capability::Capability;
use radioctl_core::description::{Family, KyForm, KyStop, RigDescription};
use radioctl_core::engine::ReplyKind;
use radioctl_core::error::ErrorKind;
use radioctl_core::op::Op;
use radioctl_core::rig::{spawn, RigOptions, TransmitPermit};
use radioctl_core::rigfile;
use radioctl_protocols::conformance::engine_for;
use radioctl_protocols::io::spawn_virtual_rig;
use radioctl_protocols::virtual_rig::VirtualRig;
use tokio::runtime::Handle;
use tokio::time::sleep;

const NOW: Duration = Duration::from_secs(10);

/// The models whose manuals document `KY` and a stop (the manuals are named in each file's
/// `kenwood.ky` fact): a second, independent copy of the decision.
const ENABLED: [&str; 5] = [
    "kenwood-ts-480",
    "kenwood-ts-590s",
    "kenwood-ts-590sg",
    "kenwood-ts-890s",
    "kenwood-ts-990s",
];

/// Documented but without a documented stop: recorded, not offered.
const DOCUMENTED_NO_STOP: [&str; 3] = ["kenwood-ts-570d", "kenwood-ts-570s", "kenwood-ts-870s"];

fn kenwood_files() -> Vec<RigDescription> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../rigs");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).expect("rigs directory") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "toml") {
            let text = std::fs::read_to_string(&path).expect("read");
            let desc = rigfile::parse(&text).expect("loads").description;
            if desc.family == Family::KenwoodAscii {
                out.push(desc);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn described(id: &str) -> RigDescription {
    kenwood_files()
        .into_iter()
        .find(|d| d.id == id)
        .unwrap_or_else(|| panic!("{id} has no file"))
}

// Tests [@ANCHOR: radioctl_core_ky_offers_send_cw]
// Tests [@ANCHOR: radioctl_core_rigfile_build_ky]
#[test]
fn exactly_the_models_with_a_documented_ky_and_stop_have_send_cw() {
    let files = kenwood_files();
    assert!(files.len() > 30);
    for desc in &files {
        let id = desc.id.as_str();
        let ky = desc.kenwood.as_ref().and_then(|k| k.ky);
        if id == "kenwood-r-5000" || id == "malachite-dsp" {
            // Receivers: no transmitter and no record.
            assert!(ky.is_none(), "{id}");
            continue;
        }
        let ky = ky.unwrap_or_else(|| panic!("{id} has no kenwood.ky record"));
        assert_eq!(
            desc.capabilities.has(Capability::SendCw),
            ENABLED.contains(&id),
            "{id}"
        );
        assert_eq!(ky.offers_send_cw(), ENABLED.contains(&id), "{id}");
        if DOCUMENTED_NO_STOP.contains(&id) {
            assert_eq!(ky.form, KyForm::Padded, "{id}");
            assert_eq!(ky.stop, KyStop::None, "{id}");
        } else if !ENABLED.contains(&id) {
            assert_eq!(ky.form, KyForm::Undocumented, "{id}");
        }
    }
    // A description that lists send-cw without a documented KY is refused at load.
    let text = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../rigs/kenwood-ts-2000.toml"),
    )
    .expect("read");
    let claimed = text.replacen("capabilities = [", "capabilities = [\"send-cw\", ", 1);
    assert!(rigfile::parse(&claimed).is_err());
}

// Tests [@ANCHOR: radioctl_kenwood_engine_ky]
// Tests [@ANCHOR: radioctl_ts890_engine_ky]
// Tests [@ANCHOR: radioctl_core_ky_encode_text]
// Tests [@ANCHOR: radioctl_core_ky_encode_stop]
#[test]
fn each_model_sends_the_form_its_manual_documents() {
    let bytes = |id: &str, op: Op| {
        let mut engine = engine_for(&described(id)).expect("engine");
        let request = engine.encode(&op).expect("encode");
        assert_eq!(request.reply, ReplyKind::None, "{id}");
        String::from_utf8(request.bytes).expect("ascii")
    };
    let padded = format!("KY {:<24};", "CQ TEST");
    assert_eq!(padded.len(), 28);
    assert_eq!(
        bytes("kenwood-ts-480", Op::SendCw("CQ TEST".into())),
        padded
    );
    assert_eq!(
        bytes("kenwood-ts-480", Op::StopCw),
        format!("KY {:24};", ""),
        "the TS-480 stops on an all-space field"
    );
    // Open question, pinned: Hamlib 4.6.2 (kenwood_send_morse) right-aligns the text for the
    // TS-590S (`KY %24s`) and pads on the right for the SG; the TS-590 guide only says the unused
    // characters are spaces. radioctl pads on the right; a bench run decides.
    for id in ["kenwood-ts-590s", "kenwood-ts-590sg", "kenwood-ts-990s"] {
        assert_eq!(
            bytes(id, Op::SendCw("cq test @".into())),
            format!("KY {:<24};", "cq test @")
        );
        assert_eq!(bytes(id, Op::StopCw), "KY0;", "{id}");
    }
    assert_eq!(
        bytes("kenwood-ts-890s", Op::SendCw("CQ TEST".into())),
        "KY2CQ TEST;"
    );
    assert_eq!(bytes("kenwood-ts-890s", Op::StopCw), "KY0;");
}

#[test]
fn text_outside_the_models_field_or_table_is_refused_and_nothing_is_sent() {
    let refused = |id: &str, text: &str| {
        let mut engine = engine_for(&described(id)).expect("engine");
        let err = engine
            .encode(&Op::SendCw(text.into()))
            .expect_err("refused");
        assert_eq!(err.kind, ErrorKind::Unsupported, "{id} {text:?}");
    };
    for id in ENABLED {
        refused(id, "");
        refused(id, "   ");
        refused(id, &"A".repeat(25));
        refused(id, "CQ;FA00007000000");
        refused(id, "caf\u{e9}");
        refused(id, "A\u{4}B");
        refused(id, "A\nB");
    }
    // The TS-480 table has neither lower case nor @.
    refused("kenwood-ts-480", "cq");
    refused("kenwood-ts-480", "@");
    let mut engine = engine_for(&described("kenwood-ts-480")).expect("engine");
    assert!(engine.encode(&Op::SendCw("A".repeat(24))).is_ok());
    // The prosign symbols of the guides are in every table.
    assert!(engine.encode(&Op::SendCw("[_<#>]\\%".into())).is_ok());
}

#[test]
fn every_other_model_reports_not_supported_for_send_cw_and_stop() {
    for desc in kenwood_files() {
        if ENABLED.contains(&desc.id.as_str()) {
            continue;
        }
        let mut engine = engine_for(&desc).expect("engine");
        for op in [Op::SendCw("CQ".into()), Op::StopCw] {
            let err = engine.encode(&op).expect_err("not supported");
            assert_eq!(err.kind, ErrorKind::Unsupported, "{} {op:?}", desc.id);
        }
    }
}

// Tests [@ANCHOR: radioctl_virtual_kenwood_ky]
#[test]
fn the_virtual_rigs_take_the_documented_forms_and_stops_and_refuse_the_rest() {
    for id in ENABLED {
        let desc = described(id);
        let mut engine = engine_for(&desc).expect("engine");
        let mut rig = VirtualRig::from_description(&desc).expect("rig");
        let send = engine.encode(&Op::SendCw("CQ TEST".into())).expect("send");
        assert!(rig.feed(&send.bytes, NOW).is_empty(), "{id}");
        assert_eq!(rig.state().cw_sent, vec!["CQ TEST".to_string()], "{id}");
        assert_eq!(rig.feed(b"KY;", NOW), b"KY0;", "{id}: buffer has room");
        let stop = engine.encode(&Op::StopCw).expect("stop");
        assert!(rig.feed(&stop.bytes, NOW).is_empty(), "{id}");
        assert!(rig.state().cw_sent.is_empty(), "{id}");
        assert_eq!(rig.state().cw_stops, 1, "{id}");
        // A field of the wrong width and the other model's stop are refused.
        assert_eq!(rig.feed(b"KY CQ;", NOW), b"?;", "{id}");
        let other_stop: &[u8] = if id == "kenwood-ts-480" {
            b"KY0;"
        } else {
            b"KY                        ;"
        };
        if id == "kenwood-ts-480" || id.ends_with("590s") || id.ends_with("590sg") {
            assert_eq!(rig.feed(other_stop, NOW), b"?;", "{id}");
        }
        assert!(rig.state().cw_sent.is_empty(), "{id}");
    }
    // A model whose manuals document no KY answers `?;` to every form.
    let desc = described("kenwood-ts-940s");
    let mut rig = VirtualRig::from_description(&desc).expect("rig");
    for wire in [&b"KY;"[..], b"KY0;", b"KY CQ                      ;"] {
        assert_eq!(rig.feed(wire, NOW), b"?;");
    }
    assert!(rig.state().cw_sent.is_empty());
}

// Tests [@ANCHOR: radioctl_core_handle_open_gate]
// Tests [@ANCHOR: radioctl_kenwood_engine_ky]
// Tests [@ANCHOR: radioctl_ts890_engine_ky]
#[tokio::test(start_paused = true)]
async fn send_cw_text_is_a_transmit_action_gated_like_other_keying() {
    for id in ENABLED {
        let desc = described(id);
        let rig = VirtualRig::from_description(&desc).expect("rig");
        let (stream, control) = spawn_virtual_rig(&Handle::current(), rig, Duration::ZERO);
        let engine = engine_for(&desc).expect("engine");
        let mut options = RigOptions::default();
        options.poll.enabled = false;
        let handle = spawn(&Handle::current(), engine, stream, options).expect("spawn");
        let before = control.bytes_received();
        let err = handle
            .execute(Op::SendCw("CQ".into()))
            .await
            .expect_err("gate closed");
        assert_eq!(err.kind, ErrorKind::TransmitGateClosed, "{id}");
        sleep(Duration::from_millis(50)).await;
        assert_eq!(control.bytes_received(), before, "{id}: nothing sent");
        assert!(control.state().cw_sent.is_empty(), "{id}");
        // The stop is allowed with the gate closed, as the unkey is.
        handle.execute(Op::StopCw).await.expect("stop");
        assert_eq!(control.state().cw_stops, 1, "{id}");
        // Open, the text reaches the rig; an over-long one is refused before any byte.
        handle.open_transmit_gate(TransmitPermit::new("kenwood ky test"));
        handle
            .execute(Op::SendCw("CQ".into()))
            .await
            .expect("sent with the gate open");
        assert_eq!(control.state().cw_sent, vec!["CQ".to_string()], "{id}");
        let sent = control.bytes_received();
        let long = handle.execute(Op::SendCw("A".repeat(40))).await;
        assert_eq!(long.expect_err("too long").kind, ErrorKind::Unsupported);
        assert_eq!(control.bytes_received(), sent, "{id}: refused unsent");
    }
}
