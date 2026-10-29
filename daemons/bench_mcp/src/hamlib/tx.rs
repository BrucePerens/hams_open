// SPDX-License-Identifier: AGPL-3.0-or-later
//! Keying the rig: the one place where PTT is turned on, and every way it is turned off.
//!
//! The rules, all enforced here and not in the tools that use this:
//!
//! * The transmit watchdog (`guard.rs`, a separate process) is armed BEFORE the rig is keyed, and keying does not happen if it
//!   cannot be armed.
//! * From the first `set_ptt 1` on, every path ends in the release and a read-back of the PTT state: success, an error while
//!   sampling, an abort for a meter reading, a panic (the guard's `Drop`), the end of the planned time.
//! * The release does not trust the connection that was in use: if it fails, a new one is tried, then the watchdog, which acts alone
//!   when its pipe closes or its deadline passes.
//! * Every keyed interval has a begin line and an end line in the keyed-interval log, and a state file exists while the rig may be
//!   keyed, so that the next start of the module can see a crash and release the rig.
//!
//! The software cannot know what is connected to the rig. The owner's attestation and the load plausibility check are the controls.

use super::guard::{self, Spawner, Watch, MARGIN};
use super::proto::{Client, RigError};
use super::stations::now_local;
use super::status::tolerate;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A rig that may be keyed right now, for the release when the server stops.
#[derive(Clone, Debug)]
struct Active {
    host: String,
    port: u16,
    min_power: Option<f64>,
}

static ACTIVE: Mutex<BTreeMap<String, Active>> = Mutex::new(BTreeMap::new());
static CHECKED: Mutex<Vec<String>> = Mutex::new(Vec::new());
static NOTES: Mutex<BTreeMap<String, Value>> = Mutex::new(BTreeMap::new());

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn state_file(dir: &Path, rig: &str) -> PathBuf {
    dir.join(format!("{}.keyed.json", super::safe_name(rig)))
}

pub fn log_file(dir: &Path, rig: &str) -> PathBuf {
    dir.join(format!("{}.keyed.log", super::safe_name(rig)))
}

pub fn guard_log(dir: &Path, rig: &str) -> PathBuf {
    dir.join(format!("{}.guard.log", super::safe_name(rig)))
}

/// Appends one JSON line to the keyed-interval log: the time, the event, and the fields given.
pub fn log_event(dir: &Path, rig: &str, event: &str, fields: Value) {
    // [@ANCHOR: hamlib_tx_log]
    use std::io::Write;
    let mut line = json!({"time": crate::audio::store::timestamp(), "date": now_local().0, "rig": rig, "event": event});
    if let (Some(l), Some(f)) = (line.as_object_mut(), fields.as_object()) {
        l.extend(f.clone());
    }
    let _ = std::fs::create_dir_all(dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file(dir, rig))
    {
        let _ = writeln!(f, "{line}");
        let _ = f.sync_all();
    }
}

/// What a keyed interval is for, written to the log and the state file.
#[derive(Clone, Debug)]
pub struct Purpose {
    pub tool: String,
    pub reason: String,
    pub by: String,
    pub frequency_hz: f64,
    pub mode: String,
    pub power_w: f64,
    /// `probe`, `test` or `sweep step`.
    pub kind: String,
}

/// One set of meter readings during the keyed interval.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub t_s: f64,
    pub power_w: Option<f64>,
    pub swr: Option<f64>,
    pub alc: Option<f64>,
    pub temp: Option<f64>,
}

impl Sample {
    pub fn json(&self) -> Value {
        let r2 = |x: f64| (x * 100.0).round() / 100.0;
        json!({"t_s": r2(self.t_s), "power_w": self.power_w.map(r2), "swr": self.swr.map(r2), "alc": self.alc.map(r2), "temp": self.temp.map(r2)})
    }
}

/// A standing-wave ratio is believable at 1.0 or more; the dummy rig's constant 0 and a meter that reads nothing are "not reported".
pub fn believable_swr(x: f64) -> Option<f64> {
    // [@ANCHOR: hamlib_tx_swr_valid]
    (x.is_finite() && (1.0..=100.0).contains(&x)).then_some(x)
}

#[derive(Debug)]
pub struct KeyedRun {
    pub samples: Vec<Sample>,
    pub seconds_keyed: f64,
    /// Why the interval ended before its planned time, if it did.
    pub aborted: Option<String>,
    /// The PTT was read back as off. `false` when the rig cannot report PTT.
    pub ptt_read_back: bool,
}

/// What `key` needs to know. `limit_swr` and `limit_power_w` stop the interval at once when a meter passes them.
pub struct Keying<'a> {
    pub rig: &'a str,
    pub host: &'a str,
    pub port: u16,
    pub dir: &'a Path,
    pub spawner: &'a dyn Spawner,
    pub min_power: Option<f64>,
    pub purpose: Purpose,
    pub limit_swr: Option<f64>,
    pub limit_power_w: Option<f64>,
    pub previous_rfpower: Option<f64>,
}

struct Keyed<'a> {
    k: &'a Keying<'a>,
    watch: Option<Box<dyn Watch>>,
    released: bool,
}

impl Keyed<'_> {
    /// Turns PTT off and reads it back; falls back to a new connection. Ok(true): read back off. Ok(false): the rig cannot say.
    fn release(&mut self, c: &mut Client) -> Result<bool, String> {
        // [@ANCHOR: hamlib_tx_release]
        let first = (|| -> Result<bool, String> {
            c.command("set_ptt 0").map_err(|e| e.to_string())?;
            match c.command("get_ptt") {
                Ok(r) if r.kv("PTT") == Some("0") => Ok(true),
                Ok(r) => Err(format!("PTT still reads {:?}", r.kv("PTT"))),
                Err(e) if e.unsupported() => Ok(false),
                Err(e) => Err(e.to_string()),
            }
        })();
        let result = match first {
            Ok(v) => Ok(v),
            Err(_) => guard::unkey(self.k.host, self.k.port, None),
        };
        match result {
            Ok(v) => {
                self.finish();
                Ok(v)
            }
            Err(e) => {
                if let Some(w) = self.watch.take() {
                    w.leave_to_guard();
                }
                Err(format!("RELEASE NOT CONFIRMED: {e}. The rig may be transmitting. Switch it off at the rig now. The watchdog was left to un-key it and set the minimum power; call bench_hamlib_safe_state, and do not use the rig until the owner has looked at it."))
            }
        }
    }

    fn finish(&mut self) {
        // [@ANCHOR: hamlib_tx_keyed_released_flag]
        self.released = true;
        lock(&ACTIVE).remove(self.k.rig);
        let _ = std::fs::remove_file(state_file(self.k.dir, self.k.rig));
        if let Some(w) = self.watch.take() {
            let _ = w.stand_down();
        }
    }
}

impl Drop for Keyed<'_> {
    fn drop(&mut self) {
        // [@ANCHOR: hamlib_tx_keyed_drop]
        // Reached only when `release` did not finish: a panic, or an early return that skipped it.
        if !self.released {
            let _ = guard::unkey(self.k.host, self.k.port, self.k.min_power);
            lock(&ACTIVE).remove(self.k.rig);
            // The watchdog is dropped with `watch`: its pipe closes without `done`.
        }
    }
}

fn read_meters(
    c: &mut Client,
    have: &mut [(&'static str, bool)],
    t: f64,
    full_scale_w: Option<f64>,
) -> Result<Sample, String> {
    // [@ANCHOR: hamlib_tx_read_meters]
    let mut sample = Sample {
        t_s: t,
        power_w: None,
        swr: None,
        alc: None,
        temp: None,
    };
    for (name, available) in have.iter_mut() {
        if !*available {
            continue;
        }
        match c.read(&format!("get_level {name}")) {
            Ok(r) => {
                let v = r.number(None).map_err(|e| e.to_string())?;
                match *name {
                    "SWR" => sample.swr = believable_swr(v),
                    "ALC" => sample.alc = Some(v),
                    "TEMP_METER" => sample.temp = Some(v),
                    "RFPOWER_METER_WATTS" => sample.power_w = Some(v),
                    // A fraction of the rig's full scale: converted with the rig's own table where there is no meter in watts.
                    _ => {
                        if sample.power_w.is_none() {
                            sample.power_w = full_scale_w.map(|w| v * w);
                        }
                    }
                }
            }
            Err(RigError::Rig {
                code: -11 | -4 | -18,
                ..
            }) => *available = false,
            Err(e) => return Err(format!("reading the {name} meter while keyed failed: {e}")),
        }
    }
    Ok(sample)
}

/// Keys the rig for `seconds`, reading the meters every `every`, and releases it on every path. The watchdog is armed first.
/// `full_scale_w` is the rig's full-scale power in watts, for a power meter that reads a fraction of it.
pub fn key(
    c: &mut Client,
    k: &Keying,
    seconds: f64,
    every: Duration,
    full_scale_w: Option<f64>,
) -> Result<KeyedRun, String> {
    // [@ANCHOR: hamlib_tx_key]
    if !(0.1..=guard_max()).contains(&seconds) {
        return Err(format!(
            "{seconds} s is outside the 0.1 to {} s a keyed interval may last",
            guard_max()
        ));
    }
    let deadline = Duration::from_secs_f64(seconds) + MARGIN;
    let watch = k.spawner.spawn(
        k.host,
        k.port,
        deadline,
        k.min_power,
        &guard_log(k.dir, k.rig),
    )?;
    let mut keyed = Keyed {
        k,
        watch: Some(watch),
        released: false,
    };
    let p = &k.purpose;
    let begin = json!({
        "tool": p.tool, "kind": p.kind, "reason": p.reason, "by": p.by, "frequency_hz": p.frequency_hz, "mode": p.mode, "power_w": p.power_w,
        "planned_seconds": seconds, "pid": std::process::id(),
    });
    let _ = std::fs::write(state_file(k.dir, k.rig), json!({"pid": std::process::id(), "host": k.host, "port": k.port, "min_power": k.min_power, "previous_rfpower": k.previous_rfpower, "began": crate::audio::store::timestamp(), "purpose": begin}).to_string());
    log_event(k.dir, k.rig, "begin", begin);
    lock(&ACTIVE).insert(
        k.rig.to_string(),
        Active {
            host: k.host.to_string(),
            port: k.port,
            min_power: k.min_power,
        },
    );
    let began = Instant::now();
    // The first command that radiates. Whatever it answers, the release below runs.
    let keyed_ok = c.command("set_ptt 1");
    let mut samples = vec![];
    let mut aborted: Option<String> = keyed_ok.err().map(|e| format!("set_ptt 1 failed: {e}"));
    let mut have = [
        ("RFPOWER_METER_WATTS", true),
        ("RFPOWER_METER", true),
        ("SWR", true),
        ("ALC", true),
        ("TEMP_METER", true),
    ];
    while aborted.is_none() {
        let t = began.elapsed().as_secs_f64();
        if t >= seconds {
            break;
        }
        match read_meters(c, &mut have, t, full_scale_w) {
            Ok(s) => {
                if let (Some(limit), Some(swr)) = (k.limit_swr, s.swr) {
                    if swr > limit {
                        aborted = Some(format!("SWR {swr:.2} is over the limit {limit:.2}"));
                    }
                }
                if let (Some(limit), Some(w)) = (k.limit_power_w, s.power_w) {
                    if w > limit * 1.5 {
                        aborted = Some(format!(
                            "the power meter reads {w:.1} W, far over the cap {limit:.1} W"
                        ));
                    }
                }
                samples.push(s);
            }
            Err(e) => aborted = Some(e),
        }
        // The rig must still be keyed: someone (safe_state, the operator) may have released it.
        if aborted.is_none() {
            if let Ok(r) = c.read("get_ptt") {
                if r.kv("PTT") == Some("0") {
                    aborted =
                        Some("the rig was un-keyed by something else during the interval".into());
                }
            }
        }
        if aborted.is_none() {
            let left = seconds - began.elapsed().as_secs_f64();
            std::thread::sleep(every.min(Duration::from_secs_f64(left.max(0.0))));
        }
    }
    let seconds_keyed = began.elapsed().as_secs_f64();
    let released = keyed.release(c);
    let outcome = match (&released, &aborted) {
        (Err(_), _) => "release_not_confirmed",
        (Ok(_), Some(_)) => "aborted",
        (Ok(_), None) => "completed",
    };
    log_event(
        k.dir,
        k.rig,
        "end",
        json!({"tool": p.tool, "kind": p.kind, "outcome": outcome, "seconds_keyed": (seconds_keyed * 100.0).round() / 100.0, "abort_reason": aborted, "frequency_hz": p.frequency_hz, "power_w": p.power_w, "reason": p.reason}),
    );
    let ptt_read_back = released?;
    Ok(KeyedRun {
        samples,
        seconds_keyed,
        aborted,
        ptt_read_back,
    })
}

/// The longest keyed interval whatever the config says.
fn guard_max() -> f64 {
    super::attest::HARD_MAX_SECONDS
}

/// Stops anything keyed by this process whose rig name starts with `prefix` (all of them for an empty prefix): called when the server
/// stops or is told to, before the daemons are stopped.
pub fn release_prefix(prefix: &str) -> Vec<String> {
    // [@ANCHOR: hamlib_tx_release_all]
    let active: Vec<(String, Active)> = lock(&ACTIVE)
        .iter()
        .filter(|(k, _)| k.starts_with(prefix))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut failed = vec![];
    for (rig, a) in active {
        match guard::unkey(&a.host, a.port, a.min_power) {
            Ok(_) => {
                lock(&ACTIVE).remove(&rig);
            }
            Err(e) => failed.push(format!("{rig}: {e}")),
        }
    }
    failed
}

/// Before the daemon of a rig is stopped on purpose: a rig this process has keyed, or a record of one, is released first, and the stop is
/// refused when that cannot be confirmed (the daemon is the only way to reach the rig). Ok(true): something was released.
pub fn release_rig(rig: &str, dir: &Path) -> Result<bool, String> {
    // [@ANCHOR: hamlib_tx_release_rig]
    let active = lock(&ACTIVE).get(rig).cloned();
    let recorded = std::fs::read_to_string(state_file(dir, rig))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| {
            Some(Active {
                host: v["host"].as_str()?.to_string(),
                port: v["port"].as_u64()? as u16,
                min_power: v["min_power"].as_f64(),
            })
        });
    let Some(a) = active.or(recorded) else {
        return Ok(false);
    };
    guard::unkey(&a.host, a.port, a.min_power).map_err(|e| format!("the rig may be keyed and could not be released first ({e}). Stopping its daemon would leave it keyed. Next: bench_hamlib_safe_state"))?;
    lock(&ACTIVE).remove(rig);
    let _ = std::fs::remove_file(state_file(dir, rig));
    Ok(true)
}

/// The leftover daemon of a dead server is about to be stopped: release a rig it may have left keyed, first.
pub fn release_after_crash(_rig: &str, port: u16) -> bool {
    // [@ANCHOR: hamlib_release_after_crash]
    guard::unkey("127.0.0.1", port, None).is_ok()
}

/// The first contact with a rig that has a transmit attestation, in this run of the server: ask it to stop transmitting, and say so
/// if a crash had left it keyed. `T 0` to a receiving rig does nothing. The note is attached to the result of the tool that
/// triggered the check.
pub fn startup_check(rig: &str, dir: &Path, host: &str, port: u16) {
    // [@ANCHOR: hamlib_tx_startup_check]
    {
        let mut done = lock(&CHECKED);
        if done.iter().any(|r| r == rig) {
            return;
        }
        done.push(rig.to_string());
    }
    let left_by = std::fs::read_to_string(state_file(dir, rig))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let mut probe = Client::connect(host, port, Duration::from_secs(2)).ok();
    let was_on = probe
        .as_mut()
        .and_then(|c| tolerate(c.read("get_ptt")).ok()?.ok())
        .map(|r| r.kv("PTT").is_some_and(|p| p != "0"));
    let stuck = left_by.is_some() || was_on == Some(true);
    // A rig that was left transmitting also gets the lowest power setting, as the watchdog would have set it.
    let min_power = if stuck {
        probe
            .as_mut()
            .and_then(|c| tolerate(c.dump_state()).ok()?.ok())
            .and_then(|d| super::txtools::rfpower_min(&d.lines))
    } else {
        None
    };
    let released = guard::unkey(host, port, min_power);
    let note = json!({
        "stuck_ptt_found": stuck, "ptt_before": match was_on { Some(true) => "on", Some(false) => "off", None => "unknown" },
        "state_file_found": left_by.is_some(), "released": released.as_ref().is_ok(), "error": released.err(),
        "what": if stuck { "the rig was left keyed (or a crash left a record): PTT was released and the power set to the minimum" } else { "PTT was released as a precaution on first contact; the rig was not transmitting" },
    });
    log_event(dir, rig, "startup_release", note.clone());
    let _ = std::fs::remove_file(state_file(dir, rig));
    if stuck {
        lock(&NOTES).insert(rig.to_string(), note);
    }
}

/// The note left by the startup check, once.
pub fn take_note(rig: &str) -> Option<Value> {
    // [@ANCHOR: hamlib_tx_take_note]
    lock(&NOTES).remove(rig)
}

/// Forgets which rigs were checked (for a test that starts "a new run of the server").
#[cfg(test)]
pub fn forget_checked(rig: &str) {
    lock(&CHECKED).retain(|r| r != rig);
}

/// Whether this process has a rig recorded as possibly keyed.
pub fn is_active(rig: &str) -> bool {
    lock(&ACTIVE).contains_key(rig)
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use super::*;
    use std::sync::Arc;

    // Tests [@ANCHOR: hamlib_tx_keyed_drop]
    #[test]
    fn a_panic_while_keyed_still_releases_the_rig_and_sets_the_minimum_power() {
        let mut sim = Sim::new();
        sim.ptt = true;
        sim.rfpower = 0.6;
        let (fake, shared) = serve_sim(sim);
        let dir = std::env::temp_dir().join(format!("bench_mcp-keyed-drop-{}", std::process::id()));
        let spawner = ThreadSpawner {
            events: Arc::new(Mutex::new(vec![])),
        };
        let k = Keying {
            rig: "tx_drop",
            host: "127.0.0.1",
            port: fake.port,
            dir: &dir,
            spawner: &spawner,
            min_power: Some(0.05),
            purpose: Purpose {
                tool: "t".into(),
                reason: "r".into(),
                by: "b".into(),
                frequency_hz: 7e6,
                mode: "USB".into(),
                power_w: 1.0,
                kind: "test".into(),
            },
            limit_swr: None,
            limit_power_w: None,
            previous_rfpower: None,
        };
        lock(&ACTIVE).insert(
            "tx_drop".into(),
            Active {
                host: "127.0.0.1".into(),
                port: fake.port,
                min_power: Some(0.05),
            },
        );
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _keyed = Keyed {
                k: &k,
                watch: None,
                released: false,
            };
            panic!("something went wrong while the rig was keyed");
        }));
        assert!(outcome.is_err());
        let s = shared.lock().unwrap();
        assert!(
            !s.ptt,
            "the guard's Drop released the rig during the unwind"
        );
        assert_eq!(s.rfpower, 0.05);
        drop(s);
        assert!(!is_active("tx_drop"));
    }

    // Tests [@ANCHOR: hamlib_tx_keyed_released_flag]
    #[test]
    fn a_normal_release_is_not_repeated_by_the_drop() {
        let mut sim = Sim::new();
        sim.ptt = true;
        let (fake, shared) = serve_sim(sim);
        let dir = std::env::temp_dir().join(format!("bench_mcp-keyed-ok-{}", std::process::id()));
        let spawner = ThreadSpawner {
            events: Arc::new(Mutex::new(vec![])),
        };
        let k = Keying {
            rig: "tx_ok_drop",
            host: "127.0.0.1",
            port: fake.port,
            dir: &dir,
            spawner: &spawner,
            min_power: Some(0.05),
            purpose: Purpose {
                tool: "t".into(),
                reason: "r".into(),
                by: "b".into(),
                frequency_hz: 7e6,
                mode: "USB".into(),
                power_w: 1.0,
                kind: "test".into(),
            },
            limit_swr: None,
            limit_power_w: None,
            previous_rfpower: None,
        };
        let mut c = Client::connect("127.0.0.1", fake.port, Duration::from_secs(2)).unwrap();
        {
            let mut keyed = Keyed {
                k: &k,
                watch: None,
                released: false,
            };
            assert_eq!(keyed.release(&mut c), Ok(true));
        }
        let s = shared.lock().unwrap();
        assert_eq!(s.ptt_commands, [0], "one release, not two");
        assert_eq!(
            s.rfpower, 0.5,
            "the power setting is left to the caller on a normal release"
        );
    }
}
