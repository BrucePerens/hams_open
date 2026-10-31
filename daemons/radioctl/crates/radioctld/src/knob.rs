// SPDX-License-Identifier: AGPL-3.0-or-later
//! The self-test's one interactive check: turn the VFO knob and see whether the radio reports the
//! change on its own (Elecraft/Kenwood/Yaesu auto-information, Icom CI-V transceive, Flex
//! subscriptions). It runs only at a terminal and only for a description that claims
//! `Capability::Transceive`. It sends nothing to the radio: it reads the rig's event stream and
//! counts only events whose source is `Transceive`, so the answers to our own polls and commands
//! are never taken for the knob. It never turns the radio's setting on by itself.

use std::io::Write;
use std::time::{Duration, Instant};

use radioctl_core::capability::Capability;
use radioctl_core::description::RigDescription;
use radioctl_core::event::Source;
use radioctl_core::op::{Field, Observation};
use radioctl_core::rig::RigHandle;

use crate::selftest::{CheckResult, Verdict};

/// The check's name in the report and the upload.
pub const KNOB_CHECK: &str = "unsolicited status (turn the VFO knob)";
/// How long the volunteer has to turn the knob.
pub const KNOB_WAIT: Duration = Duration::from_secs(15);
/// What is said when nothing arrived: a hint about the radio's menu, never the explanation alone.
pub const NOT_SEEN: &str = "No unsolicited update was seen. Some radios need this turned on in the menu (for example the KX3's auto-info/transceive setting, or CI-V Transceive on an Icom); if yours has it, enable it and run this again. If it is on, this is a problem in our software.";

/// What watching the knob found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnobOutcome {
    /// The radio reported a frequency change by itself; the change in hertz.
    Seen(i64),
    /// Nothing arrived within the wait.
    NotSeen,
    /// The volunteer pressed Enter.
    Skipped,
}

// [@ANCHOR: radioctld_knob_claims_unsolicited]
/// True when the description claims the rig reports changes on its own.
pub fn claims_unsolicited(description: &RigDescription) -> bool {
    description.capabilities.has(Capability::Transceive)
}

// [@ANCHOR: radioctld_knob_watch]
/// Watches the rig's event stream for up to `wait` for a frequency change whose source is
/// `Transceive`. `enter` is polled about ten times a second and returns true once the volunteer
/// pressed Enter. Sends nothing to the radio.
pub async fn watch_knob(
    rig: &RigHandle,
    wait: Duration,
    enter: &mut dyn FnMut() -> bool,
) -> KnobOutcome {
    let mut events = rig.events();
    let end = Instant::now() + wait;
    while Instant::now() < end {
        if enter() {
            return KnobOutcome::Skipped;
        }
        let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(100), events.next()).await
        else {
            continue;
        };
        if event.source != Source::Transceive {
            continue;
        }
        for change in &event.changes {
            if let (Field::Frequency(_), Observation::Frequency(_, new)) =
                (change.field, change.new)
            {
                let old = match change.old {
                    Some(Observation::Frequency(_, hz)) => hz.0 as i64,
                    _ => new.0 as i64,
                };
                return KnobOutcome::Seen(new.0 as i64 - old);
            }
        }
    }
    KnobOutcome::NotSeen
}

// [@ANCHOR: radioctld_knob_result]
/// The report line for an outcome. A knob that was not seen counts as a check that did not pass
/// (the report's standing note says whose problem that is); one the volunteer skipped does not.
pub fn knob_result(outcome: &KnobOutcome, millis: u128) -> CheckResult {
    let verdict = match outcome {
        KnobOutcome::Seen(delta) => Verdict::Pass(format!(
            "frequency changed by {delta:+} Hz, reported by the radio without being asked"
        )),
        KnobOutcome::NotSeen => Verdict::Fail(format!("not seen. {NOT_SEEN}")),
        KnobOutcome::Skipped => Verdict::NotApplicable("skipped by you (Enter pressed)".into()),
    };
    CheckResult {
        name: KNOB_CHECK.into(),
        op: "(none: listens only)".into(),
        verdict,
        millis,
        wire: String::new(),
    }
}

// [@ANCHOR: radioctld_knob_check]
/// The whole check, or `None` when the description does not claim the capability. Without a
/// terminal it is skipped (`skipped: no terminal`, not counted as a failure). At a terminal it
/// asks, waits and reports.
pub async fn knob_check(
    description: &RigDescription,
    rig: &RigHandle,
    interactive: bool,
    enter: &mut dyn FnMut() -> bool,
    out: &mut dyn Write,
    wait: Duration,
) -> Option<CheckResult> {
    if !claims_unsolicited(description) {
        return None;
    }
    if !interactive {
        return Some(CheckResult {
            name: KNOB_CHECK.into(),
            op: "(none: listens only)".into(),
            verdict: Verdict::NotApplicable("skipped: no terminal".into()),
            millis: 0,
            wire: String::new(),
        });
    }
    let _ = writeln!(
        out,
        "Please turn the VFO knob a little, in either direction, now (waiting up to {} seconds; press Enter to skip)",
        wait.as_secs()
    );
    let started = Instant::now();
    let outcome = watch_knob(rig, wait, enter).await;
    Some(knob_result(&outcome, started.elapsed().as_millis()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::rig::{spawn_current, RigOptions};
    use radioctl_core::units::Hz;
    use radioctl_protocols::conformance::engine_for;
    use radioctl_protocols::io::{spawn_virtual_rig, VirtualRigControl};
    use radioctl_protocols::virtual_rig::{PanelChange, VirtualRig};
    use tokio::runtime::Handle;

    use crate::selftest::AnyEngine;

    fn rig_for(id: &str) -> (RigDescription, RigHandle, VirtualRigControl) {
        let texts = crate::builtin::embedded_texts();
        let (_, text) = texts
            .iter()
            .find(|(l, _)| l.ends_with(id))
            .expect("embedded");
        let description = radioctl_core::rigfile::parse(text)
            .expect("parse")
            .description;
        let virtual_rig = VirtualRig::from_description(&description).expect("virtual");
        let (stream, control) = spawn_virtual_rig(&Handle::current(), virtual_rig, Duration::ZERO);
        let engine = AnyEngine(engine_for(&description).expect("engine"));
        let rig = spawn_current(engine, stream, RigOptions::default()).expect("rig");
        (description, rig, control)
    }

    // Tests [@ANCHOR: radioctld_knob_claims_unsolicited]
    #[test]
    fn only_descriptions_that_claim_transceive_get_the_check() {
        assert!(claims_unsolicited(&RigDescription::ic7300()));
        let texts = crate::builtin::embedded_texts();
        let any_without = texts.iter().any(|(_, t)| {
            !claims_unsolicited(&radioctl_core::rigfile::parse(t).expect("parse").description)
        });
        assert!(!texts.is_empty() && (any_without || texts.len() > 1));
        let mut plain = RigDescription::ic7300();
        plain.capabilities = radioctl_core::capability::CapabilitySet::default();
        assert!(!claims_unsolicited(&plain));
    }

    // Tests [@ANCHOR: radioctld_knob_watch]
    // Tests [@ANCHOR: radioctld_knob_result]
    #[tokio::test]
    async fn a_knob_turn_on_a_transceive_rig_is_seen_with_its_frequency_delta() {
        let (_, rig, control) = rig_for("icom-ic-7300");
        let before = control.state().freq_a.0;
        let turner = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            control.front_panel(PanelChange::Frequency(Hz(before + 1_000)));
        });
        let outcome = watch_knob(&rig, Duration::from_secs(5), &mut || false).await;
        turner.await.expect("turner");
        assert_eq!(outcome, KnobOutcome::Seen(1_000));
        let line = knob_result(&outcome, 5);
        assert!(
            matches!(&line.verdict, Verdict::Pass(t) if t.contains("+1000 Hz")),
            "{line:?}"
        );
        assert_eq!(line.name, KNOB_CHECK);
    }

    // Tests [@ANCHOR: radioctld_knob_watch]
    // Tests [@ANCHOR: radioctld_knob_result]
    #[tokio::test]
    async fn no_unsolicited_frame_is_not_seen_and_enter_skips() {
        // The Kenwood virtual rig starts with auto-information off: it only answers.
        let (_, rig, control) = rig_for("kenwood-ts-590sg");
        control.front_panel(PanelChange::Frequency(Hz(7_100_000)));
        let outcome = watch_knob(&rig, Duration::from_millis(600), &mut || false).await;
        assert_eq!(outcome, KnobOutcome::NotSeen);
        let line = knob_result(&outcome, 600);
        assert!(
            matches!(&line.verdict, Verdict::Fail(t) if t.contains("No unsolicited update was seen"))
        );
        assert!(matches!(&line.verdict, Verdict::Fail(t) if t.contains("problem in our software")));
        let mut presses = 0;
        let skipped = watch_knob(&rig, Duration::from_secs(5), &mut || {
            presses += 1;
            presses > 2
        })
        .await;
        assert_eq!(skipped, KnobOutcome::Skipped);
        assert!(matches!(
            knob_result(&skipped, 1).verdict,
            Verdict::NotApplicable(_)
        ));
    }

    // Tests [@ANCHOR: radioctld_knob_check]
    #[tokio::test]
    async fn the_check_prompts_only_at_a_terminal_and_skips_quietly_without_one() {
        let (description, rig, control) = rig_for("icom-ic-7300");
        let mut out = Vec::new();
        let none = knob_check(
            &description,
            &rig,
            false,
            &mut || false,
            &mut out,
            KNOB_WAIT,
        )
        .await;
        let skipped = none.expect("claims the capability");
        assert_eq!(
            skipped.verdict,
            Verdict::NotApplicable("skipped: no terminal".into())
        );
        assert!(out.is_empty());
        let before = control.state().freq_a.0;
        let turner = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            control.front_panel(PanelChange::Frequency(Hz(before - 500)));
        });
        let seen = knob_check(
            &description,
            &rig,
            true,
            &mut || false,
            &mut out,
            Duration::from_secs(5),
        )
        .await
        .expect("check");
        turner.await.expect("turner");
        assert!(matches!(seen.verdict, Verdict::Pass(_)), "{seen:?}");
        let said = String::from_utf8(out).expect("utf8");
        assert!(said.contains("Please turn the VFO knob a little, in either direction, now (waiting up to 5 seconds; press Enter to skip)"), "{said}");
        // A description that does not claim the capability gets no check at all.
        let mut plain = description.clone();
        plain.capabilities = radioctl_core::capability::CapabilitySet::default();
        assert!(knob_check(
            &plain,
            &rig,
            true,
            &mut || false,
            &mut Vec::new(),
            Duration::ZERO
        )
        .await
        .is_none());
    }
}
