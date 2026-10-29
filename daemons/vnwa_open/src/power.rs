// SPDX-License-Identifier: AGPL-3.0-or-later
//! Cold reset of the unit through a power-switching USB hub. The unit is bus powered; switching its hub port off and on with
//! `uhubctl` really removes its power (verified 2026-10-28: the unit goes dark and keeps no settings), after which it
//! re-enumerates about 8 s later with new device numbers. Everything the unit holds (set-up, generator state) must be sent again.

use std::time::{Duration, Instant};

/// A port of a power-switching hub, as `uhubctl -l <hub> -p <port>` names it (for example hub `3-2`, port 2).
#[derive(Clone, Debug, PartialEq)]
pub struct HubPort {
    pub hub: String,
    pub port: u8,
}

/// Runs a program and gives back its exit status and output; production is a process, tests script it.
pub trait Runner {
    fn run(&self, program: &str, args: &[String]) -> Result<(bool, String), String>;
}

pub struct ProcessRunner;

impl Runner for ProcessRunner {
    fn run(&self, program: &str, args: &[String]) -> Result<(bool, String), String> {
        // [@ANCHOR: vnwa_open_power_process_runner]
        let out = std::process::Command::new(program)
            .args(args)
            .output()
            .map_err(|e| format!("{program}: {e}"))?;
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        Ok((out.status.success(), text))
    }
}

/// How long the port stays off, so the unit's supply capacitors discharge.
pub const OFF_TIME: Duration = Duration::from_secs(3);
/// The longest wait for the unit and its codecs to be back after power-on (it takes about 8 s).
pub const BACK_TIMEOUT: Duration = Duration::from_secs(45);

fn uhubctl(run: &dyn Runner, port: &HubPort, action: &str) -> Result<(), String> {
    // [@ANCHOR: vnwa_open_power_uhubctl]
    let args: Vec<String> = [
        "-n",
        "uhubctl",
        "-l",
        &port.hub,
        "-p",
        &port.port.to_string(),
        "-a",
        action,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let (ok, text) = run.run("sudo", &args)?;
    if !ok {
        return Err(format!(
            "uhubctl {action} on hub {} port {} failed: {}",
            port.hub,
            port.port,
            text.trim()
        ));
    }
    Ok(())
}

/// Polls `ready` every `every` until it is true or `timeout` passes.
pub fn wait_until(timeout: Duration, every: Duration, mut ready: impl FnMut() -> bool) -> bool {
    // [@ANCHOR: vnwa_open_power_wait_until]
    let end = Instant::now() + timeout;
    loop {
        if ready() {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(every);
    }
}

/// Switches the port off, waits `off_time`, switches it on, and waits (up to `back`) for `ready` (the unit and both codecs
/// present). An error says which step failed.
pub fn power_cycle(
    run: &dyn Runner,
    port: &HubPort,
    off_time: Duration,
    back: Duration,
    ready: impl FnMut() -> bool,
) -> Result<(), String> {
    // [@ANCHOR: vnwa_open_power_cycle]
    uhubctl(run, port, "off")?;
    std::thread::sleep(off_time);
    uhubctl(run, port, "on")?;
    if !wait_until(back, Duration::from_millis(500), ready) {
        return Err(format!(
            "the unit did not come back within {} s of power-on on hub {} port {}",
            back.as_secs(),
            port.hub,
            port.port
        ));
    }
    Ok(())
}

/// True when the unit is on the bus and both codecs are present.
pub fn unit_present() -> bool {
    let usb = rusb::devices()
        .map(|d| {
            d.iter().any(|dev| {
                dev.device_descriptor()
                    .map(|dd| dd.vendor_id() == crate::VID && dd.product_id() == crate::PID)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    usb && crate::topology::codecs().is_ok()
}

/// The unit's control chip once failed to enumerate (kernel: "unable to enumerate USB device", errors -32 and -71) after a 3 s power-off
/// that followed a modulation lock-up; a second cycle with a longer power-off brought it back. So a cycle that does not bring the unit back
/// is repeated with a longer off time.
pub const LONG_OFF_TIME: Duration = Duration::from_secs(12);

/// Runs `power_cycle` for each (off time, wait for the unit) pair in turn until one succeeds; the error lists every failure.
pub fn power_cycle_escalating(
    run: &dyn Runner,
    port: &HubPort,
    schedule: &[(Duration, Duration)],
    mut ready: impl FnMut() -> bool,
) -> Result<(), String> {
    // [@ANCHOR: vnwa_open_power_escalation]
    let mut failures = Vec::new();
    for (i, (off, back)) in schedule.iter().enumerate() {
        match power_cycle(run, port, *off, *back, &mut ready) {
            Ok(()) => return Ok(()),
            Err(e) => failures.push(format!("cycle {} (off {} s): {e}", i + 1, off.as_secs())),
        }
    }
    Err(failures.join("; "))
}

/// A power-cycle implementation supplied by the program that embeds this crate: given the hub port and the ladder of (off time, wait for the unit), it cycles
/// until the unit is back. `bench_mcp` installs its USB hub module's here, so that the recovery inside a measurement, the generator-off recovery and
/// the `bench_vna_power_cycle` tool all run one implementation (verification of re-enumeration, the hub protection rules, the evidence record). This crate cannot depend
/// on `bench_mcp`, so standalone programs (the examples, the soak) use the built-in `power_cycle_escalating` over `sudo -n uhubctl`.
pub type CycleHook =
    Box<dyn Fn(&HubPort, &[(Duration, Duration)]) -> Result<(), String> + Send + Sync>;

static HOOK: std::sync::OnceLock<CycleHook> = std::sync::OnceLock::new();

/// Installs the program's power-cycle implementation; the first call wins (a later one returns false).
pub fn set_cycle_hook(hook: CycleHook) -> bool {
    // [@ANCHOR: vnwa_open_cycle_hook]
    HOOK.set(hook).is_ok()
}

/// Cycles with the installed hook if there is one, else with the built-in implementation.
fn cycle_with(
    hook: Option<&CycleHook>,
    port: &HubPort,
    schedule: &[(Duration, Duration)],
) -> Result<(), String> {
    match hook {
        Some(h) => h(port, schedule),
        None => power_cycle_escalating(&ProcessRunner, port, schedule, unit_present),
    }
}

/// The production cold reset: `sudo -n uhubctl` through the hub, then wait for the unit and its codecs; if the unit does not come back,
/// once more with a longer power-off.
pub fn cold_reset(port: &HubPort) -> Result<(), String> {
    // [@ANCHOR: vnwa_open_power_cold_reset]
    power_cycle_escalating(
        &ProcessRunner,
        port,
        &[
            (OFF_TIME, Duration::from_secs(25)),
            (LONG_OFF_TIME, BACK_TIMEOUT),
        ],
        unit_present,
    )?;
    // The unit and its codecs are listed before they answer reliably (a request sent 3 s after enumeration stalled); give them time.
    std::thread::sleep(Duration::from_secs(8));
    Ok(())
}

/// The ladder after AM or FM: the unit's control chip failed to come back after one or more cycles in 11 of 25 recoveries from a modulation
/// lock-up measured on 2026-10-29 (it needed 1 cycle 14 times, 2 cycles 6 times, 3 cycles 3 times, 4 or more twice), so this tries
/// five cycles, 3, 12, 12, 30 and 12 s off, before it gives up. The cost is paid only when the unit does not come back.
pub fn modulation_ladder() -> Vec<(Duration, Duration)> {
    [3u64, 12, 12, 30, 12]
        .iter()
        .map(|s| {
            (
                Duration::from_secs(*s),
                if *s == 3 {
                    Duration::from_secs(25)
                } else {
                    BACK_TIMEOUT
                },
            )
        })
        .collect()
}

/// The cold reset to use when the unit is in a modulation lock-up (or may be): the longer ladder of `modulation_ladder`.
pub fn cold_reset_after_modulation(port: &HubPort) -> Result<(), String> {
    // [@ANCHOR: vnwa_open_modulation_recovery]
    cycle_with(HOOK.get(), port, &modulation_ladder())?;
    std::thread::sleep(Duration::from_secs(8));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    // Tests [@ANCHOR: vnwa_open_cycle_hook]
    #[test]
    fn an_installed_cycle_implementation_gets_the_port_and_the_ladder_and_its_error_is_returned() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let hook: CycleHook = Box::new(move |p, ladder| {
            s2.lock()
                .unwrap()
                .push((p.hub.clone(), p.port, ladder.len()));
            Err("hub tools say no".into())
        });
        let port = HubPort {
            hub: "3-2".into(),
            port: 2,
        };
        let ladder = modulation_ladder();
        let e = cycle_with(Some(&hook), &port, &ladder).unwrap_err();
        assert_eq!(e, "hub tools say no");
        assert_eq!(
            *seen.lock().unwrap(),
            vec![("3-2".to_string(), 2u8, 5usize)]
        );
        // Without a hook the built-in implementation runs `sudo -n uhubctl` (not run here: a hook that must not be reached would show as a call).
        assert!(
            HOOK.get().is_none(),
            "no test installs the process-wide hook"
        );
    }

    // Tests [@ANCHOR: vnwa_open_modulation_recovery]
    #[test]
    fn the_modulation_ladder_starts_short_and_ends_with_a_long_power_off() {
        let l = modulation_ladder();
        assert_eq!(l.len(), 5);
        assert_eq!(l[0].0, OFF_TIME);
        assert!(l
            .windows(2)
            .all(|w| w[1].0 >= LONG_OFF_TIME || w[1].0 >= w[0].0));
        assert_eq!(l[3].0.as_secs(), 30);
        // A unit that comes back at the fourth cycle is recovered by it, and the failures of the earlier ones are not an error.
        let fake = Fake {
            calls: RefCell::new(Vec::new()),
            fail_on: None,
        };
        let mut n = 0;
        // The same ladder in milliseconds, so the test does not wait out the real off times.
        let fast: Vec<(Duration, Duration)> = l
            .iter()
            .map(|(o, b)| {
                (
                    Duration::from_millis(o.as_secs()),
                    Duration::from_millis(b.as_secs() * 10),
                )
            })
            .collect();
        let r = power_cycle_escalating(
            &fake,
            &HubPort {
                hub: "3-2".into(),
                port: 2,
            },
            &fast,
            || {
                n += 1;
                n >= 4
            },
        );
        assert!(r.is_ok(), "{r:?}");
    }

    struct Fake {
        calls: RefCell<Vec<String>>,
        fail_on: Option<&'static str>,
    }
    impl Runner for Fake {
        fn run(&self, program: &str, args: &[String]) -> Result<(bool, String), String> {
            self.calls
                .borrow_mut()
                .push(format!("{program} {}", args.join(" ")));
            let failing = self.fail_on.is_some_and(|f| args.iter().any(|a| a == f));
            Ok((
                !failing,
                if failing {
                    "No compatible devices detected".into()
                } else {
                    String::new()
                },
            ))
        }
    }

    fn port() -> HubPort {
        HubPort {
            hub: "3-2".into(),
            port: 2,
        }
    }

    // Tests [@ANCHOR: vnwa_open_power_cycle]
    // Tests [@ANCHOR: vnwa_open_power_uhubctl]
    // Tests [@ANCHOR: vnwa_open_power_wait_until]
    #[test]
    fn the_cycle_is_off_then_on_through_sudo_uhubctl_and_waits_for_the_unit() {
        let f = Fake {
            calls: RefCell::new(vec![]),
            fail_on: None,
        };
        let mut polls = 0;
        power_cycle(&f, &port(), Duration::ZERO, Duration::from_secs(5), || {
            polls += 1;
            polls >= 3
        })
        .unwrap();
        assert_eq!(
            *f.calls.borrow(),
            vec![
                "sudo -n uhubctl -l 3-2 -p 2 -a off".to_string(),
                "sudo -n uhubctl -l 3-2 -p 2 -a on".to_string()
            ]
        );
        assert_eq!(polls, 3);
    }

    // Tests [@ANCHOR: vnwa_open_power_escalation]
    #[test]
    fn a_unit_that_does_not_come_back_gets_a_second_cycle_with_a_longer_off_time() {
        let f = Fake {
            calls: RefCell::new(vec![]),
            fail_on: None,
        };
        let mut polls = 0;
        let sched = [
            (Duration::ZERO, Duration::from_millis(30)),
            (Duration::ZERO, Duration::from_secs(5)),
        ];
        // Not ready during the first cycle's short wait, ready in the second.
        power_cycle_escalating(&f, &port(), &sched, || {
            polls += 1;
            polls >= 3
        })
        .unwrap();
        assert_eq!(f.calls.borrow().len(), 4, "off, on, off, on");
        let f = Fake {
            calls: RefCell::new(vec![]),
            fail_on: None,
        };
        let e = power_cycle_escalating(&f, &port(), &sched[..1], || false).unwrap_err();
        assert!(
            e.contains("cycle 1") && e.contains("did not come back"),
            "{e}"
        );
    }

    // Tests [@ANCHOR: vnwa_open_power_uhubctl]
    #[test]
    fn a_unit_that_does_not_return_and_a_failing_hub_are_clear_errors() {
        let f = Fake {
            calls: RefCell::new(vec![]),
            fail_on: None,
        };
        let e = power_cycle(
            &f,
            &port(),
            Duration::ZERO,
            Duration::from_millis(50),
            || false,
        )
        .unwrap_err();
        assert!(e.contains("did not come back"), "{e}");
        let f = Fake {
            calls: RefCell::new(vec![]),
            fail_on: Some("off"),
        };
        let e = power_cycle(
            &f,
            &port(),
            Duration::ZERO,
            Duration::from_millis(50),
            || true,
        )
        .unwrap_err();
        assert!(
            e.contains("uhubctl off") && e.contains("No compatible"),
            "{e}"
        );
        assert_eq!(
            f.calls.borrow().len(),
            1,
            "no power-on after a failed power-off"
        );
    }

    // Tests [@ANCHOR: vnwa_open_power_process_runner]
    #[test]
    fn the_process_runner_gives_the_exit_status_and_both_output_streams_of_a_real_program() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (ok, text) = ProcessRunner
            .run("sh", &args(&["-c", "echo out; echo err >&2"]))
            .unwrap();
        assert!(ok);
        assert!(text.contains("out") && text.contains("err"), "{text}");
        let (ok, _) = ProcessRunner.run("sh", &args(&["-c", "exit 1"])).unwrap();
        assert!(!ok);
        let e = ProcessRunner
            .run("/nonexistent/vnwa_open_test_program", &[])
            .unwrap_err();
        assert!(e.contains("vnwa_open_test_program"), "{e}");
    }

    // Tests [@ANCHOR: vnwa_open_power_wait_until]
    #[test]
    fn waiting_ends_at_the_first_true_answer_and_gives_up_after_the_timeout() {
        let mut polls = 0;
        assert!(wait_until(
            Duration::from_secs(5),
            Duration::from_millis(1),
            || {
                polls += 1;
                polls == 4
            }
        ));
        assert_eq!(polls, 4);
        let started = Instant::now();
        assert!(!wait_until(
            Duration::from_millis(30),
            Duration::from_millis(5),
            || false
        ));
        assert!(started.elapsed() >= Duration::from_millis(30));
    }
}
