// SPDX-License-Identifier: AGPL-3.0-or-later
//! Transcript-replay conformance: a rig description, its engine, its virtual rig and a transcript
//! must all agree, byte for byte. The same description file builds the engine and the test double,
//! so a driver and its simulator cannot drift apart without a step failing here.

use std::time::Duration;

use radioctl_core::description::{Family, KenwoodDialect, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind};
use radioctl_core::error::RigError;
use radioctl_core::transcript::{Expect, Panel, Transcript};

use crate::civ::CivEngine;
use crate::elecraft::ElecraftEngine;
use crate::kenwood::KenwoodEngine;
use crate::kenwood_ts890::Ts890Engine;
use crate::virtual_flex::{FlexPanel, VirtualFlex};
use crate::virtual_rig::{PanelChange, VirtualRig};
use crate::yaesu::YaesuEngine;
use crate::yaesu_legacy::YaesuLegacyEngine;

const NOW: Duration = Duration::from_secs(1);

// [@ANCHOR: radioctl_conformance_engine_for]
/// The engine for a description's protocol family.
pub fn engine_for(description: &RigDescription) -> Result<Box<dyn ProtocolEngine>, RigError> {
    Ok(match description.family {
        Family::IcomCiv => Box::new(CivEngine::new(description.clone())?),
        Family::KenwoodAscii
            if description
                .kenwood
                .as_ref()
                .is_some_and(|k| k.dialect == KenwoodDialect::Ts890) =>
        {
            Box::new(Ts890Engine::new(description.clone())?)
        }
        Family::KenwoodAscii => Box::new(KenwoodEngine::new(description.clone())?),
        Family::ElecraftAscii => Box::new(ElecraftEngine::new(description.clone())?),
        Family::YaesuNewcat => Box::new(YaesuEngine::new(description.clone())?),
        Family::YaesuLegacy => Box::new(YaesuLegacyEngine::new(description.clone())?),
        Family::KenwoodHh => Box::new(crate::kenwood_hh::KenwoodHhEngine::new(
            description.clone(),
        )?),
        Family::Flex => Box::new(crate::flex::FlexEngine::new(description.clone())?),
        Family::Hpsdr => Box::new(crate::hpsdr_net::HpsdrEngine::new(description.clone())?),
        Family::SerialVendor => crate::serial_vendor::engine_for(description)?,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

// [@ANCHOR: radioctl_conformance_expected_inbound]
fn expected_inbound(expect: Expect) -> Inbound {
    match expect {
        Expect::Observation(value) => Inbound::Observation(value),
        Expect::Ack => Inbound::Ack,
        Expect::Nak => Inbound::Nak,
    }
}

// [@ANCHOR: radioctl_conformance_panel_change]
fn panel_change(panel: Panel) -> PanelChange {
    match panel {
        Panel::Frequency(hz) => PanelChange::Frequency(hz),
        Panel::Mode(mode) => PanelChange::Mode(mode),
        Panel::Ptt(on) => PanelChange::Ptt(on),
        Panel::Signal(level) => PanelChange::Signal(level),
        Panel::Split(on) => PanelChange::Split(on),
        Panel::RitOffset(hz) => PanelChange::RitOffset(hz),
    }
}

/// The radio's stand-in a transcript replays against: the family's virtual radio, or, for an
/// OpenHPSDR description, a stand-in for the `hpsdr_io` bridge the engine really talks to.
enum StandIn {
    Rig(Box<VirtualRig>),
    Flex(Box<VirtualFlex>),
    Vendor(Box<dyn crate::serial_vendor::VendorTwin>),
    HpsdrBridge(BridgeStandIn),
}

/// What the `hpsdr_io` bridge answers on its line protocol, with no radio behind it: frequency,
/// PTT and attenuator are kept; forward power reads 0 and SWR 1.0 (`10` tenths), as the bridge
/// reports with no forward power. Transcripts for the ANAN files are of this protocol, not of
/// the radio's UDP wire (chapter 7 of the manual).
struct BridgeStandIn {
    freq: u64,
    ptt: bool,
    att_db: u64,
}

impl BridgeStandIn {
    // [@ANCHOR: radioctl_conformance_bridge_stand_in_feed]
    /// Answers one request line as `hpsdr_io::handle_line` does.
    fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut out = String::new();
        for line in String::from_utf8_lossy(bytes).lines() {
            let mut parts = line.split_whitespace();
            let answer = match (
                parts.next(),
                parts.next().and_then(|v| v.parse::<u64>().ok()),
            ) {
                (Some("GF"), _) => format!(
                    "F {}
",
                    self.freq
                ),
                (Some("GP"), _) => format!(
                    "P {}
",
                    u8::from(self.ptt)
                ),
                (Some("GW"), _) => "W 0
"
                .to_string(),
                (Some("GR"), _) => "R 10
"
                .to_string(),
                (Some("GA"), _) => format!(
                    "A {}
",
                    self.att_db
                ),
                (Some("A"), Some(db)) if db <= 31 => {
                    self.att_db = db;
                    "OK
"
                    .to_string()
                }
                (Some("F"), Some(hz)) if u32::try_from(hz).is_ok() => {
                    self.freq = hz;
                    "OK
"
                    .to_string()
                }
                (Some("P"), Some(on @ (0 | 1))) => {
                    self.ptt = on == 1;
                    "OK
"
                    .to_string()
                }
                _ => "ERR command
"
                .to_string(),
            };
            out.push_str(&answer);
        }
        out.into_bytes()
    }
}

impl StandIn {
    // [@ANCHOR: radioctl_conformance_stand_in_for]
    /// The stand-in for a description's family.
    fn for_description(description: &RigDescription) -> Result<StandIn, RigError> {
        Ok(match description.family {
            Family::Flex => StandIn::Flex(Box::new(VirtualFlex::from_description(description)?)),
            Family::SerialVendor => StandIn::Vendor(crate::serial_vendor::twin_for(description)?),
            Family::Hpsdr => StandIn::HpsdrBridge(BridgeStandIn {
                freq: description.default_frequency.0,
                ptt: false,
                att_db: 0,
            }),
            _ => StandIn::Rig(Box::new(VirtualRig::from_description(description)?)),
        })
    }

    // [@ANCHOR: radioctl_conformance_stand_in_prologue]
    /// What the radio sends before it is asked anything (a FlexRadio's `V` and `H` lines).
    fn prologue(&self) -> Vec<u8> {
        match self {
            StandIn::Flex(radio) => radio.greeting(),
            StandIn::Rig(_) | StandIn::Vendor(_) | StandIn::HpsdrBridge(_) => Vec::new(),
        }
    }

    // [@ANCHOR: radioctl_conformance_stand_in_feed]
    /// The radio's answer to the controller's bytes.
    fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        match self {
            StandIn::Rig(rig) => rig.feed(bytes, NOW),
            StandIn::Flex(radio) => radio.feed(bytes, NOW),
            StandIn::Vendor(twin) => twin.feed(bytes, NOW),
            StandIn::HpsdrBridge(bridge) => bridge.feed(bytes),
        }
    }

    // [@ANCHOR: radioctl_conformance_stand_in_front_panel]
    /// Turns a knob at the radio; `Err` names a knob this stand-in has not got.
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match self {
            StandIn::Rig(rig) => Ok(rig.front_panel(panel_change(panel))),
            StandIn::Flex(radio) => {
                let change = match panel {
                    Panel::Frequency(hz) => FlexPanel::Tune(hz),
                    Panel::Mode(mode) => FlexPanel::SetMode(mode),
                    Panel::Ptt(on) => FlexPanel::Mox(on),
                    other => return Err(format!("a FlexRadio has no {other:?} knob here")),
                };
                Ok(radio.front_panel(change))
            }
            StandIn::Vendor(twin) => twin.front_panel(panel),
            StandIn::HpsdrBridge(_) => Err("the hpsdr_io bridge pushes nothing unasked".into()),
        }
    }
}

// [@ANCHOR: radioctl_conformance_replay]
/// Replays `transcript` against the engine and the virtual rig built from `description`.
///
/// Per solicited step: the engine must encode the op to exactly `tx`, with a reply expectation
/// that matches whether `rx` is empty; the virtual rig fed `tx` must answer exactly `rx`; the
/// engine fed `rx` must decode exactly `expect`. Per unsolicited step: the virtual rig turning
/// the knob must send exactly `rx`, and the engine must decode it as `expect`. A step with
/// neither op nor knob but with `rx` is the radio speaking first (a prologue): the virtual rig's
/// prologue must be exactly `rx` and the engine must decode it as `expect`. Returns the number
/// of steps replayed, or every mismatch found.
pub fn replay(description: &RigDescription, transcript: &Transcript) -> Result<usize, Vec<String>> {
    let mut problems = Vec::new();
    if transcript.rig != description.id {
        problems.push(format!(
            "transcript is for {:?}, description is {:?}",
            transcript.rig, description.id
        ));
        return Err(problems);
    }
    let (mut engine, mut rig) = match (
        engine_for(description),
        StandIn::for_description(description),
    ) {
        (Ok(engine), Ok(rig)) => (engine, rig),
        (Err(e), _) | (_, Err(e)) => return Err(vec![e.to_string()]),
    };
    for (index, step) in transcript.steps.iter().enumerate() {
        let sent = match (&step.op, &step.panel) {
            (Some(op), _) => {
                replay_solicited(engine.as_mut(), &mut rig, op, step, index, &mut problems)
            }
            (None, Some(panel)) => {
                match rig.front_panel(*panel) {
                    Ok(bytes) if bytes == step.rx => {}
                    Ok(bytes) => problems.push(format!(
                        "step {index}: virtual rig sent {} but transcript has {}",
                        hex(&bytes),
                        hex(&step.rx)
                    )),
                    Err(why) => problems.push(format!("step {index}: {why}")),
                }
                true
            }
            (None, None) if !step.rx.is_empty() => {
                let bytes = rig.prologue();
                if bytes != step.rx {
                    problems.push(format!(
                        "step {index}: virtual rig's prologue is {} but transcript has {}",
                        hex(&bytes),
                        hex(&step.rx)
                    ));
                }
                true
            }
            (None, None) => false,
        };
        if sent {
            check_decoding(engine.as_mut(), step, index, &mut problems);
        }
    }
    if problems.is_empty() {
        Ok(transcript.steps.len())
    } else {
        Err(problems)
    }
}

// [@ANCHOR: radioctl_conformance_replay_solicited]
fn replay_solicited(
    engine: &mut dyn ProtocolEngine,
    rig: &mut StandIn,
    op: &radioctl_core::op::Op,
    step: &radioctl_core::transcript::Step,
    index: usize,
    problems: &mut Vec<String>,
) -> bool {
    match engine.encode(op) {
        Err(error) => problems.push(format!("step {index}: engine refused {op:?}: {error}")),
        Ok(request) => {
            if request.bytes != step.tx {
                problems.push(format!(
                    "step {index}: engine encoded {} but transcript has {}",
                    hex(&request.bytes),
                    hex(&step.tx)
                ));
            }
            if (request.reply == ReplyKind::None) != step.rx.is_empty() {
                problems.push(format!(
                    "step {index}: reply expectation {:?} disagrees with rx",
                    request.reply
                ));
            }
        }
    }
    let answer = rig.feed(&step.tx);
    if answer != step.rx {
        problems.push(format!(
            "step {index}: virtual rig answered {} but transcript has {}",
            hex(&answer),
            hex(&step.rx)
        ));
    }
    true
}

// [@ANCHOR: radioctl_conformance_check_decoding]
fn check_decoding(
    engine: &mut dyn ProtocolEngine,
    step: &radioctl_core::transcript::Step,
    index: usize,
    problems: &mut Vec<String>,
) {
    let decoded = engine.feed(&step.rx, NOW);
    let wanted: Vec<Inbound> = step
        .expect
        .into_iter()
        .chain(step.expect_more.iter().copied())
        .map(expected_inbound)
        .collect();
    if decoded != wanted {
        problems.push(format!(
            "step {index}: engine decoded {decoded:?} but transcript expects {wanted:?}"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::transcript;

    const GOOD: &str = r#"
schema = 1
rig = "icom-ic-7300"
origin = "unit test"
[[step]]
op = { kind = "get-frequency", vfo = "a" }
tx = "fe fe 94 e0 03 fd"
rx = "fe fe e0 94 03 00 40 07 14 00 fd"
expect = { kind = "frequency", vfo = "a", hz = 14074000 }
"#;

    // Tests [@ANCHOR: radioctl_conformance_replay]
    // Tests [@ANCHOR: radioctl_conformance_replay_solicited]
    // Tests [@ANCHOR: radioctl_conformance_engine_for]
    // Tests [@ANCHOR: radioctl_conformance_check_decoding]
    // Tests [@ANCHOR: radioctl_conformance_expected_inbound]
    // Tests [@ANCHOR: radioctl_conformance_panel_change]
    #[test]
    fn a_consistent_transcript_replays_and_each_kind_of_disagreement_is_named() {
        let desc = RigDescription::ic7300();
        let good = transcript::parse(GOOD).expect("parses");
        assert_eq!(replay(&desc, &good), Ok(1));

        let wrong_tx = transcript::parse(
            &GOOD.replace("tx = \"fe fe 94 e0 03 fd\"", "tx = \"fe fe 94 e0 04 fd\""),
        )
        .expect("parses");
        let problems = replay(&desc, &wrong_tx).expect_err("must fail");
        assert!(
            problems.iter().any(|p| p.contains("engine encoded")),
            "{problems:?}"
        );
        assert!(
            problems.iter().any(|p| p.contains("virtual rig answered")),
            "{problems:?}"
        );

        let wrong_value =
            transcript::parse(&GOOD.replace("hz = 14074000", "hz = 14074001")).expect("parses");
        let problems = replay(&desc, &wrong_value).expect_err("must fail");
        assert!(
            problems.iter().any(|p| p.contains("engine decoded")),
            "{problems:?}"
        );

        let other_rig =
            transcript::parse(&GOOD.replace("icom-ic-7300", "kenwood-ts-2000")).expect("parses");
        assert!(replay(&desc, &other_rig).expect_err("must fail")[0].contains("transcript is for"));
    }

    // Tests [@ANCHOR: radioctl_conformance_stand_in_for]
    // Tests [@ANCHOR: radioctl_conformance_stand_in_prologue]
    // Tests [@ANCHOR: radioctl_conformance_stand_in_front_panel]
    // Tests [@ANCHOR: radioctl_conformance_bridge_stand_in_feed]
    #[test]
    fn the_stand_ins_name_a_wrong_prologue_a_missing_knob_and_a_refused_line() {
        let flex = radioctl_core::rigfile::parse(include_str!("../../../rigs/flex-6400.toml"))
            .expect("flex file")
            .description;
        let wrong_prologue = transcript::parse(
            r#"
schema = 1
rig = "flex-6400"
origin = "unit test"
[[step]]
rx_text = "V9.9.9.9\nH00000001\n"
[[step]]
panel = { kind = "signal", value = 3 }
rx_text = "S0|nothing\n"
"#,
        )
        .expect("parses");
        let problems = replay(&flex, &wrong_prologue).expect_err("must fail");
        assert!(
            problems.iter().any(|p| p.contains("prologue is")),
            "{problems:?}"
        );
        assert!(
            problems.iter().any(|p| p.contains("no Signal(3) knob")),
            "{problems:?}"
        );
        let anan = radioctl_core::rigfile::parse(include_str!("../../../rigs/anan-100.toml"))
            .expect("anan file")
            .description;
        let mut bridge = StandIn::for_description(&anan).expect("stand-in");
        assert!(bridge.prologue().is_empty());
        assert_eq!(bridge.feed(b"GF\n"), b"F 7100000\n");
        assert_eq!(bridge.feed(b"A 40\n"), b"ERR command\n", "past 31 dB");
        assert_eq!(bridge.feed(b"P 1\nGP\nP 0\nGP\n"), b"OK\nP 1\nOK\nP 0\n");
        assert!(bridge.front_panel(Panel::Ptt(true)).is_err());
    }
}
