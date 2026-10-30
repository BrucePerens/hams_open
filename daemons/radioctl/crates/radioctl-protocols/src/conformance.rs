// SPDX-License-Identifier: AGPL-3.0-or-later
//! Transcript-replay conformance: a rig description, its engine, its virtual rig and a transcript
//! must all agree, byte for byte. The same description file builds the engine and the test double,
//! so a driver and its simulator cannot drift apart without a step failing here.

use std::time::Duration;

use radioctl_core::description::{Family, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind};
use radioctl_core::error::RigError;
use radioctl_core::transcript::{Expect, Panel, Transcript};

use crate::civ::CivEngine;
use crate::kenwood::KenwoodEngine;
use crate::virtual_rig::{PanelChange, VirtualRig};

const NOW: Duration = Duration::from_secs(1);

// [@ANCHOR: radioctl_conformance_engine_for]
/// The engine for a description's protocol family.
pub fn engine_for(description: &RigDescription) -> Result<Box<dyn ProtocolEngine>, RigError> {
    Ok(match description.family {
        Family::IcomCiv => Box::new(CivEngine::new(description.clone())?),
        Family::KenwoodAscii => Box::new(KenwoodEngine::new(description.clone())?),
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn expected_inbound(expect: Expect) -> Inbound {
    match expect {
        Expect::Observation(value) => Inbound::Observation(value),
        Expect::Ack => Inbound::Ack,
        Expect::Nak => Inbound::Nak,
    }
}

fn panel_change(panel: Panel) -> PanelChange {
    match panel {
        Panel::Frequency(hz) => PanelChange::Frequency(hz),
        Panel::Mode(mode) => PanelChange::Mode(mode),
        Panel::Ptt(on) => PanelChange::Ptt(on),
        Panel::Signal(level) => PanelChange::Signal(level),
    }
}

// [@ANCHOR: radioctl_conformance_replay]
/// Replays `transcript` against the engine and the virtual rig built from `description`.
///
/// Per solicited step: the engine must encode the op to exactly `tx`, with a reply expectation
/// that matches whether `rx` is empty; the virtual rig fed `tx` must answer exactly `rx`; the
/// engine fed `rx` must decode exactly `expect`. Per unsolicited step: the virtual rig turning
/// the knob must send exactly `rx`, and the engine must decode it as `expect`. Returns the number
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
        VirtualRig::from_description(description),
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
                let bytes = rig.front_panel(panel_change(*panel));
                if bytes != step.rx {
                    problems.push(format!(
                        "step {index}: virtual rig sent {} but transcript has {}",
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
    rig: &mut VirtualRig,
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
    let answer = rig.feed(&step.tx, NOW);
    if answer != step.rx {
        problems.push(format!(
            "step {index}: virtual rig answered {} but transcript has {}",
            hex(&answer),
            hex(&step.rx)
        ));
    }
    true
}

fn check_decoding(
    engine: &mut dyn ProtocolEngine,
    step: &radioctl_core::transcript::Step,
    index: usize,
    problems: &mut Vec<String>,
) {
    let decoded = engine.feed(&step.rx, NOW);
    let wanted: Vec<Inbound> = step.expect.map(expected_inbound).into_iter().collect();
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
}
