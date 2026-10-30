// SPDX-License-Identifier: AGPL-3.0-or-later
//! Wire transcripts for conformance tests: what a controller sends (`tx`), what the rig answers
//! (`rx`) and what the engine must make of it. A transcript says where it came from in `origin`;
//! a hand-written one is labelled as such and proves self-consistency, not fidelity to a radio.

use serde::Deserialize;

use crate::error::{ErrorKind, RigError};
use crate::op::{Observation, Op};
use crate::units::{Hz, Mode, Vfo};

/// What the engine must report for the rig's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    /// A value.
    Observation(Observation),
    /// The rig accepted the command.
    Ack,
    /// The rig refused the command.
    Nak,
}

/// A knob turned at the radio itself (an unsolicited step).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    /// VFO A moved.
    Frequency(Hz),
    /// The mode changed.
    Mode(Mode),
    /// The PTT switch moved.
    Ptt(bool),
    /// The signal strength changed.
    Signal(u16),
}

/// One exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// The command, if the step starts with one.
    pub op: Option<Op>,
    /// Bytes the controller writes; empty for an unsolicited step.
    pub tx: Vec<u8>,
    /// Bytes the rig answers; empty when it does not answer.
    pub rx: Vec<u8>,
    /// What the engine must decode from `rx`.
    pub expect: Option<Expect>,
    /// For an unsolicited step, the knob that makes the virtual rig send `rx`.
    pub panel: Option<Panel>,
}

/// A transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    /// The rig description id it applies to.
    pub rig: String,
    /// Where the bytes came from.
    pub origin: String,
    /// The exchanges, in order, against one rig that starts in its default state.
    pub steps: Vec<Step>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Doc {
    schema: u32,
    rig: String,
    origin: String,
    step: Vec<StepSpec>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StepSpec {
    op: Option<OpSpec>,
    #[serde(default)]
    tx: String,
    #[serde(default)]
    rx: String,
    expect: Option<ExpectSpec>,
    panel: Option<PanelSpec>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum OpSpec {
    GetFrequency { vfo: String },
    SetFrequency { vfo: String, hz: u64 },
    GetMode,
    SetMode { mode: String },
    GetPtt,
    SetPtt { on: bool },
    GetSignal,
    GetPower,
    SendCw { text: String },
    SetTransceive { on: bool },
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum ExpectSpec {
    Frequency { vfo: String, hz: u64 },
    Mode { mode: String },
    Ptt { on: bool },
    Signal { value: u16 },
    Power { value: u16 },
    Ack,
    Nak,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum PanelSpec {
    Frequency { hz: u64 },
    Mode { mode: String },
    Ptt { on: bool },
    Signal { value: u16 },
}

fn bad(detail: impl Into<String>) -> RigError {
    RigError::new(ErrorKind::Malformed, detail)
}

// [@ANCHOR: radioctl_core_transcript_hex]
/// Parses hex bytes written with optional spaces (`fe fe 94` or `fefe94`).
pub fn parse_hex(text: &str) -> Result<Vec<u8>, RigError> {
    let digits: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    if !digits.len().is_multiple_of(2) || !digits.iter().all(char::is_ascii_hexdigit) {
        return Err(bad(format!("not an even number of hex digits: {text:?}")));
    }
    let mut out = Vec::new();
    for pair in digits.chunks(2) {
        let text: String = pair.iter().collect();
        out.push(u8::from_str_radix(&text, 16).map_err(|e| bad(e.to_string()))?);
    }
    Ok(out)
}

fn vfo(name: &str) -> Result<Vfo, RigError> {
    Vfo::from_name(name).ok_or_else(|| bad(format!("unknown vfo {name:?}")))
}

fn mode(name: &str) -> Result<Mode, RigError> {
    Mode::from_name(name).ok_or_else(|| bad(format!("unknown mode {name:?}")))
}

// [@ANCHOR: radioctl_core_transcript_build_op]
fn build_op(spec: OpSpec) -> Result<Op, RigError> {
    Ok(match spec {
        OpSpec::GetFrequency { vfo: v } => Op::GetFrequency(vfo(&v)?),
        OpSpec::SetFrequency { vfo: v, hz } => Op::SetFrequency(vfo(&v)?, Hz(hz)),
        OpSpec::GetMode => Op::GetMode,
        OpSpec::SetMode { mode: m } => Op::SetMode(mode(&m)?),
        OpSpec::GetPtt => Op::GetPtt,
        OpSpec::SetPtt { on } => Op::SetPtt(on),
        OpSpec::GetSignal => Op::GetSignal,
        OpSpec::GetPower => Op::GetPower,
        OpSpec::SendCw { text } => Op::SendCw(text),
        OpSpec::SetTransceive { on } => Op::SetTransceive(on),
    })
}

// [@ANCHOR: radioctl_core_transcript_build_expect]
fn build_expect(spec: ExpectSpec) -> Result<Expect, RigError> {
    Ok(match spec {
        ExpectSpec::Frequency { vfo: v, hz } => {
            Expect::Observation(Observation::Frequency(vfo(&v)?, Hz(hz)))
        }
        ExpectSpec::Mode { mode: m } => Expect::Observation(Observation::Mode(mode(&m)?)),
        ExpectSpec::Ptt { on } => Expect::Observation(Observation::Ptt(on)),
        ExpectSpec::Signal { value } => Expect::Observation(Observation::Signal(value)),
        ExpectSpec::Power { value } => Expect::Observation(Observation::Power(value)),
        ExpectSpec::Ack => Expect::Ack,
        ExpectSpec::Nak => Expect::Nak,
    })
}

// [@ANCHOR: radioctl_core_transcript_build_panel]
fn build_panel(spec: PanelSpec) -> Result<Panel, RigError> {
    Ok(match spec {
        PanelSpec::Frequency { hz } => Panel::Frequency(Hz(hz)),
        PanelSpec::Mode { mode: m } => Panel::Mode(mode(&m)?),
        PanelSpec::Ptt { on } => Panel::Ptt(on),
        PanelSpec::Signal { value } => Panel::Signal(value),
    })
}

// [@ANCHOR: radioctl_core_transcript_parse]
/// Parses and validates a transcript file. A step needs either an `op` with its `tx`, or a
/// `panel` (unsolicited); an `expect` needs an `rx`; `origin` may not be empty.
pub fn parse(text: &str) -> Result<Transcript, RigError> {
    let doc: Doc = toml::from_str(text).map_err(|e| bad(format!("transcript: {e}")))?;
    if doc.schema != 1 {
        return Err(bad(format!(
            "transcript schema {} is not supported",
            doc.schema
        )));
    }
    if doc.origin.trim().is_empty() || doc.step.is_empty() {
        return Err(bad("a transcript needs an origin and at least one step"));
    }
    let mut steps = Vec::new();
    for (index, spec) in doc.step.into_iter().enumerate() {
        let step = Step {
            op: spec.op.map(build_op).transpose()?,
            tx: parse_hex(&spec.tx)?,
            rx: parse_hex(&spec.rx)?,
            expect: spec.expect.map(build_expect).transpose()?,
            panel: spec.panel.map(build_panel).transpose()?,
        };
        let solicited = step.op.is_some() && !step.tx.is_empty() && step.panel.is_none();
        let unsolicited = step.op.is_none() && step.tx.is_empty() && step.panel.is_some();
        if !solicited && !unsolicited {
            return Err(bad(format!(
                "step {index}: needs either op and tx, or only a panel"
            )));
        }
        if step.expect.is_some() && step.rx.is_empty() {
            return Err(bad(format!("step {index}: expect without rx")));
        }
        steps.push(step);
    }
    Ok(Transcript {
        rig: doc.rig,
        origin: doc.origin,
        steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
schema = 1
rig = "icom-ic-7300"
origin = "hand-written test"

[[step]]
op = { kind = "get-frequency", vfo = "a" }
tx = "fe fe 94 e0 03 fd"
rx = "fe fe e0 94 03 00 40 07 14 00 fd"
expect = { kind = "frequency", vfo = "a", hz = 14074000 }

[[step]]
op = { kind = "set-ptt", on = false }
tx = "fefe94e01c0000fd"
rx = "fefee094fbfd"
expect = { kind = "ack" }

[[step]]
panel = { kind = "frequency", hz = 7000000 }
rx = "fe fe 00 94 00 00 00 00 07 00 fd"
expect = { kind = "frequency", vfo = "a", hz = 7000000 }
"#;

    // Tests [@ANCHOR: radioctl_core_transcript_hex]
    #[test]
    fn hex_with_and_without_spaces() {
        assert_eq!(parse_hex("fe FD 0a").expect("hex"), vec![0xFE, 0xFD, 0x0A]);
        assert_eq!(parse_hex("").expect("empty"), Vec::<u8>::new());
        assert!(parse_hex("f").is_err());
        assert!(parse_hex("zz").is_err());
    }

    // Tests [@ANCHOR: radioctl_core_transcript_parse]
    // Tests [@ANCHOR: radioctl_core_transcript_build_expect]
    // Tests [@ANCHOR: radioctl_core_transcript_build_op]
    // Tests [@ANCHOR: radioctl_core_transcript_build_panel]
    #[test]
    fn a_transcript_parses_into_typed_steps() {
        let transcript = parse(GOOD).expect("parses");
        assert_eq!(transcript.steps.len(), 3);
        let first = transcript.steps.first().expect("step");
        assert_eq!(first.op, Some(Op::GetFrequency(Vfo::A)));
        assert_eq!(
            first.expect,
            Some(Expect::Observation(Observation::Frequency(
                Vfo::A,
                Hz(14_074_000)
            )))
        );
        assert_eq!(
            transcript.steps.get(1).and_then(|s| s.expect),
            Some(Expect::Ack)
        );
        assert_eq!(
            transcript.steps.get(2).and_then(|s| s.panel),
            Some(Panel::Frequency(Hz(7_000_000)))
        );
    }

    // Tests [@ANCHOR: radioctl_core_transcript_parse]
    #[test]
    fn malformed_transcripts_are_rejected() {
        let reject = |text: String| {
            assert_eq!(
                parse(&text).expect_err("rejected").kind,
                ErrorKind::Malformed
            )
        };
        reject(GOOD.replace("origin = \"hand-written test\"", "origin = \" \""));
        reject(GOOD.replace("vfo = \"a\", hz = 14074000", "vfo = \"z\", hz = 1"));
        reject(GOOD.replace("tx = \"fefe94e01c0000fd\"", "tx = \"\""));
        reject(GOOD.replace("rx = \"fefee094fbfd\"\n", ""));
        reject(GOOD.replace("schema = 1", "schema = 3"));
        reject(GOOD.replace("kind = \"ack\"", "kind = \"maybe\""));
        reject("schema = 1\nrig = \"x\"\norigin = \"y\"\nstep = []".to_string());
    }
}
