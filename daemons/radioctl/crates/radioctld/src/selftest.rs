// SPDX-License-Identifier: AGPL-3.0-or-later
//! The volunteer self-test: read-only checks of one rig description against one real radio, and a
//! plain-text report to attach to a bug report by hand.
//!
//! What it never does: transmit, open the transmit gate, or touch the network. Every operation it
//! plans passes [`is_read_only`], and the rig is opened with the default options (gate closed).
//! The report holds every byte exchanged with the radio, in both directions, so a maintainer can
//! turn it into a transcript. Nothing leaves the machine; the volunteer reads the file and files
//! it by hand (docs/VOLUNTEER_TESTING.md).

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use radioctl_core::capability::Capability;
use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::Op;
use radioctl_core::rig::{spawn_current, RigHandle, RigOptions};
use radioctl_core::serial::{open_serial, LineControl, PortTarget, SerialConfig};
use radioctl_core::units::{Level, Meter, Vfo};
use radioctl_core::usb::UsbMatcher;
use radioctl_protocols::conformance::engine_for;
use radioctl_protocols::io::spawn_virtual_rig;
use radioctl_protocols::virtual_rig::VirtualRig;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::runtime::Handle;

/// How one check ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The radio answered and the answer decoded.
    Pass(String),
    /// The description does not support the operation (not a radio fault).
    NotApplicable(String),
    /// The radio did not answer, refused, or sent bytes the engine could not decode.
    Fail(String),
}

/// One executed check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    /// The check's name, such as `read frequency A`.
    pub name: String,
    /// The operation, as printed by `Debug`.
    pub op: String,
    /// What happened.
    pub verdict: Verdict,
    /// Milliseconds from sending to the verdict.
    pub millis: u128,
}

/// What the report header states about the run.
#[derive(Debug, Clone)]
pub struct ReportHeader {
    /// The rig description id.
    pub rig_id: String,
    /// The description's display name.
    pub rig_name: String,
    /// The protocol family name.
    pub family: String,
    /// The target with any USB serial number removed.
    pub target: String,
    /// The baud rate used.
    pub baud: u32,
    /// The time of the run, as printed by [`utc_stamp`].
    pub started: String,
    /// Facts of the description per verified state.
    pub verified: BTreeMap<&'static str, usize>,
}

// [@ANCHOR: radioctld_selftest_is_read_only]
/// True only for operations that read; anything not listed here is refused by the self-test.
pub fn is_read_only(op: &Op) -> bool {
    matches!(
        op,
        Op::GetFrequency(_)
            | Op::GetMode
            | Op::GetPtt
            | Op::GetSignal
            | Op::GetPower
            | Op::GetSplit
            | Op::GetTxVfo
            | Op::GetRit
            | Op::GetXit
            | Op::GetRitOffset
            | Op::GetFilterWidth
            | Op::GetLevel(_)
            | Op::GetSwr
            | Op::GetMeter(_)
            | Op::GetModelId
            | Op::GetFirmware
    )
}

// [@ANCHOR: radioctld_selftest_plan_checks]
/// The read-only checks for what the description claims, in a fixed order.
pub fn plan_checks(description: &RigDescription) -> Vec<(String, Op)> {
    let has = |c: Capability| description.capabilities.has(c);
    let mut plan: Vec<(String, Op)> = Vec::new();
    if has(Capability::Identify) {
        plan.push(("identify: model id".into(), Op::GetModelId));
        plan.push(("identify: firmware".into(), Op::GetFirmware));
    }
    if has(Capability::ReadFrequency) {
        plan.push(("read frequency A".into(), Op::GetFrequency(Vfo::A)));
    }
    if has(Capability::SecondVfo) {
        plan.push(("read frequency B".into(), Op::GetFrequency(Vfo::B)));
    }
    if has(Capability::ReadMode) {
        plan.push(("read mode".into(), Op::GetMode));
    }
    if has(Capability::ReadPtt) {
        plan.push(("read transmit state".into(), Op::GetPtt));
    }
    if has(Capability::ReadSignal) {
        plan.push(("read S-meter".into(), Op::GetSignal));
    }
    if has(Capability::ReadPower) {
        plan.push(("read power meter".into(), Op::GetPower));
    }
    if has(Capability::ReadSwr) {
        plan.push(("read SWR meter".into(), Op::GetSwr));
    }
    if has(Capability::ReadMeters) {
        for meter in [Meter::Alc, Meter::Comp, Meter::Vd, Meter::Id] {
            plan.push((format!("read meter {meter:?}"), Op::GetMeter(meter)));
        }
    }
    if has(Capability::Split) {
        plan.push(("read split".into(), Op::GetSplit));
        plan.push(("read transmit VFO".into(), Op::GetTxVfo));
    }
    if has(Capability::RitXit) {
        plan.push(("read RIT".into(), Op::GetRit));
        plan.push(("read XIT".into(), Op::GetXit));
        plan.push(("read RIT/XIT offset".into(), Op::GetRitOffset));
    }
    if has(Capability::Filter) {
        plan.push(("read filter width".into(), Op::GetFilterWidth));
    }
    if has(Capability::Levels) {
        for level in [
            Level::AfGain,
            Level::RfGain,
            Level::MicGain,
            Level::MonitorLevel,
            Level::KeyerSpeed,
            Level::RfPower,
            Level::Attenuator,
            Level::Preamp,
        ] {
            plan.push((format!("read level {level:?}"), Op::GetLevel(level)));
        }
    }
    plan
}

// [@ANCHOR: radioctld_selftest_redact_target]
/// The target with a USB serial number replaced, so a report can be shared without it.
/// `usb:VID:PID:SERIAL` keeps only the ids; a path is kept as it is.
pub fn redact_target(target: &str) -> String {
    match target.strip_prefix("usb:") {
        Some(ids) => {
            let mut parts = ids.splitn(3, ':');
            let vid = parts.next().unwrap_or("");
            let pid = parts.next().unwrap_or("");
            match parts.next() {
                Some(_) => format!("usb:{vid}:{pid}:<serial removed>"),
                None => format!("usb:{vid}:{pid}"),
            }
        }
        None => target.to_string(),
    }
}

// [@ANCHOR: radioctld_selftest_utc_stamp]
/// `YYYY-MM-DD HH:MM:SS UTC` for a Unix time (days since 1970 turned into a civil date).
pub fn utc_stamp(unix_seconds: u64) -> String {
    let days = (unix_seconds / 86_400) as i64;
    let rest = unix_seconds % 86_400;
    // Civil-from-days, valid for the whole proleptic Gregorian calendar.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

// [@ANCHOR: radioctld_selftest_render_report]
/// The plain-text report: header, one line per check, a summary, then the raw wire lines.
pub fn render_report(header: &ReportHeader, results: &[CheckResult], wire: &str) -> String {
    let mut text = String::new();
    let mut line = |s: String| {
        text.push_str(&s);
        text.push('\n');
    };
    line("radioctl rig self-test report (read-only; nothing was transmitted)".into());
    line(format!(
        "tool version: radioctld {}",
        env!("CARGO_PKG_VERSION")
    ));
    line(format!(
        "system: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    line(format!("started: {}", header.started));
    line(format!(
        "description: {} ({}) family {}",
        header.rig_id, header.rig_name, header.family
    ));
    let states: Vec<String> = header
        .verified
        .iter()
        .map(|(state, count)| format!("{count} {state}"))
        .collect();
    line(format!("description facts: {}", states.join(", ")));
    line(format!("target: {} at {} baud", header.target, header.baud));
    line(String::new());
    for r in results {
        let (word, detail) = match &r.verdict {
            Verdict::Pass(d) => ("PASS", d.as_str()),
            Verdict::NotApplicable(d) => ("N/A ", d.as_str()),
            Verdict::Fail(d) => ("FAIL", d.as_str()),
        };
        line(format!(
            "{word} {} [{}] {} ms: {detail}",
            r.name, r.op, r.millis
        ));
    }
    let count = |f: fn(&Verdict) -> bool| results.iter().filter(|r| f(&r.verdict)).count();
    line(String::new());
    line(format!(
        "summary: {} pass, {} fail, {} not applicable",
        count(|v| matches!(v, Verdict::Pass(_))),
        count(|v| matches!(v, Verdict::Fail(_))),
        count(|v| matches!(v, Verdict::NotApplicable(_)))
    ));
    line(String::new());
    line(
        "wire log (seconds since start, tx = sent to the radio, rx = received, hex bytes):".into(),
    );
    for wire_line in wire.lines() {
        line(wire_line.to_string());
    }
    text
}

// [@ANCHOR: radioctld_selftest_wire_tap]
/// Puts a tap between the actor and the transport: bytes both ways go to `wire` as hexadecimal
/// lines `SECONDS tx|rx HEX`, the same format as `bench_console --wire`.
pub fn wire_tap<T>(transport: T, wire: Arc<Mutex<File>>) -> DuplexStream
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
{
    let (actor_side, mut tap_side) = tokio::io::duplex(8192);
    let started = Instant::now();
    tokio::spawn(async move {
        let mut inner = transport;
        let (mut from_radio, mut from_actor) = ([0u8; 1024], [0u8; 1024]);
        let record = |direction: &str, bytes: &[u8]| {
            let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
            if let Ok(mut file) = wire.lock() {
                let _ = writeln!(
                    file,
                    "{:9.3} {direction} {}",
                    started.elapsed().as_secs_f64(),
                    hex.join(" ")
                );
            }
        };
        loop {
            tokio::select! {
                read = inner.read(&mut from_radio) => match read {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let bytes = from_radio.get(..n).unwrap_or(&[]);
                        record("rx", bytes);
                        if tap_side.write_all(bytes).await.is_err() {
                            break;
                        }
                    }
                },
                read = tap_side.read(&mut from_actor) => match read {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let bytes = from_actor.get(..n).unwrap_or(&[]);
                        record("tx", bytes);
                        if inner.write_all(bytes).await.is_err() {
                            break;
                        }
                    }
                },
            }
        }
    });
    actor_side
}

/// `spawn_current` wants a sized engine type; this lets the engine of any family go through it.
struct AnyEngine(Box<dyn ProtocolEngine>);

impl ProtocolEngine for AnyEngine {
    fn description(&self) -> &RigDescription {
        self.0.description()
    }

    // [@ANCHOR: radioctld_selftest_any_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        self.0.encode(op)
    }

    // [@ANCHOR: radioctld_selftest_any_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        self.0.feed(bytes, now)
    }
}

// [@ANCHOR: radioctld_selftest_parse_target]
/// `virtual`, a serial path, or `usb:VID:PID[:SERIAL]` (hexadecimal ids) as a port target.
pub fn parse_target(target: &str) -> Result<PortTarget, String> {
    match target.strip_prefix("usb:") {
        Some(ids) => {
            let mut parts = ids.splitn(3, ':');
            let id = |p: Option<&str>| {
                u16::from_str_radix(p.unwrap_or(""), 16)
                    .map_err(|_| format!("{target:?}: VID and PID must be hexadecimal"))
            };
            let vid = id(parts.next())?;
            let pid = id(parts.next())?;
            Ok(PortTarget::Usb(UsbMatcher {
                vid,
                pid,
                serial: parts.next().map(str::to_string),
            }))
        }
        None => Ok(PortTarget::Path(PathBuf::from(target))),
    }
}

// [@ANCHOR: radioctld_selftest_open]
/// Opens the rig with the default options: the transmit gate stays closed and nothing here can
/// open it. `target` is `virtual` for an in-memory radio, which is what the tests use.
pub async fn open(
    description: &RigDescription,
    target: &str,
    baud: u32,
    unmanaged: bool,
    wire: Arc<Mutex<File>>,
) -> Result<RigHandle, String> {
    let engine = AnyEngine(engine_for(description).map_err(|e| e.detail)?);
    let options = RigOptions::default();
    if target == "virtual" {
        let radio = VirtualRig::from_description(description).map_err(|e| e.detail)?;
        let (stream, _control) = spawn_virtual_rig(&Handle::current(), radio, Duration::ZERO);
        return spawn_current(engine, wire_tap(stream, wire), options).map_err(|e| e.detail);
    }
    let mut config = SerialConfig::new(parse_target(target)?, baud);
    config.first_open_attempts = 3;
    if unmanaged {
        config.lines = LineControl::Unmanaged;
    }
    let link = open_serial(&Handle::current(), config)
        .await
        .map_err(|e| e.detail)?;
    let mut options = options;
    options.link_watch = Some(link.watch());
    let (transport, monitor) = link.into_parts();
    // The monitor (control lines, status, counters) lives as long as the program.
    std::mem::forget(monitor);
    spawn_current(engine, wire_tap(transport, wire), options).map_err(|e| e.detail)
}

// [@ANCHOR: radioctld_selftest_run_checks]
/// Runs the planned checks one after another and returns what happened. An operation that is not
/// read-only is refused here even if a plan ever contained one.
pub async fn run_checks(rig: &RigHandle, plan: Vec<(String, Op)>) -> Vec<CheckResult> {
    let mut results = Vec::new();
    for (name, op) in plan {
        let text = format!("{op:?}");
        let started = Instant::now();
        let verdict = if !is_read_only(&op) {
            Verdict::Fail("refused: not a read-only operation".into())
        } else {
            match rig.execute(op).await {
                Ok(Some(observation)) => Verdict::Pass(format!("{observation:?}")),
                Ok(None) => Verdict::Pass("ok".into()),
                Err(e) if e.kind == ErrorKind::Unsupported => Verdict::NotApplicable(e.detail),
                Err(e) => Verdict::Fail(format!("{:?}: {}", e.kind, e.detail)),
            }
        };
        results.push(CheckResult {
            name,
            op: text,
            verdict,
            millis: started.elapsed().as_millis(),
        });
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::description::RigDescription;

    fn temp_wire(name: &str) -> (Arc<Mutex<File>>, PathBuf) {
        let path =
            std::env::temp_dir().join(format!("selftest-{name}-{}.wire", std::process::id()));
        let file = File::create(&path).expect("temp file");
        (Arc::new(Mutex::new(file)), path)
    }

    // Tests [@ANCHOR: radioctld_selftest_plan_checks]
    // Tests [@ANCHOR: radioctld_selftest_is_read_only]
    #[test]
    fn every_planned_check_is_read_only() {
        for description in [RigDescription::ic7300(), RigDescription::ts2000()] {
            let plan = plan_checks(&description);
            assert!(plan.len() > 3, "{}: {plan:?}", description.id);
            for (name, op) in plan {
                assert!(is_read_only(&op), "{name}: {op:?}");
            }
        }
    }

    // Tests [@ANCHOR: radioctld_selftest_is_read_only]
    #[test]
    fn transmit_and_set_operations_are_not_read_only() {
        for op in [
            Op::SetPtt(true),
            Op::SetPtt(false),
            Op::SendCw("CQ".into()),
            Op::StopCw,
            Op::SetFrequency(Vfo::A, radioctl_core::units::Hz(7_000_000)),
            Op::SetSplit(true),
            Op::SetTransceive(true),
        ] {
            assert!(!is_read_only(&op), "{op:?}");
        }
    }

    // Tests [@ANCHOR: radioctld_selftest_redact_target]
    #[test]
    fn usb_serial_numbers_are_removed_from_the_target() {
        assert_eq!(
            redact_target("usb:10c4:ea60:ABC123"),
            "usb:10c4:ea60:<serial removed>"
        );
        assert_eq!(redact_target("usb:10c4:ea60"), "usb:10c4:ea60");
        assert_eq!(redact_target("/dev/ttyUSB0"), "/dev/ttyUSB0");
    }

    // Tests [@ANCHOR: radioctld_selftest_parse_target]
    #[test]
    fn targets_parse_or_say_why_not() {
        assert!(matches!(
            parse_target("/dev/ttyUSB0"),
            Ok(PortTarget::Path(_))
        ));
        match parse_target("usb:10c4:ea60:S1") {
            Ok(PortTarget::Usb(m)) => {
                assert_eq!(
                    (m.vid, m.pid, m.serial.as_deref()),
                    (0x10c4, 0xea60, Some("S1"))
                );
            }
            other => panic!("{other:?}"),
        }
        assert!(parse_target("usb:zz:1").is_err());
    }

    // Tests [@ANCHOR: radioctld_selftest_utc_stamp]
    #[test]
    fn unix_times_become_calendar_dates() {
        assert_eq!(utc_stamp(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc_stamp(951_782_400), "2000-02-29 00:00:00 UTC");
        assert_eq!(utc_stamp(1_793_404_800 + 3_723), "2026-10-31 01:02:03 UTC");
    }

    // Tests [@ANCHOR: radioctld_selftest_open]
    // Tests [@ANCHOR: radioctld_selftest_run_checks]
    // Tests [@ANCHOR: radioctld_selftest_wire_tap]
    // Tests [@ANCHOR: radioctld_selftest_render_report]
    // Tests [@ANCHOR: radioctld_selftest_any_engine_encode]
    // Tests [@ANCHOR: radioctld_selftest_any_engine_feed]
    #[tokio::test]
    async fn virtual_ic7300_passes_and_the_wire_log_has_both_directions() {
        let description = RigDescription::ic7300();
        let (wire, path) = temp_wire("virtual");
        let rig = open(&description, "virtual", 19_200, true, wire)
            .await
            .expect("virtual rig opens");
        let results = run_checks(&rig, plan_checks(&description)).await;
        assert!(
            results
                .iter()
                .all(|r| !matches!(r.verdict, Verdict::Fail(_))),
            "{results:?}"
        );
        assert!(results
            .iter()
            .any(|r| r.name == "read frequency A" && matches!(r.verdict, Verdict::Pass(_))));
        let wire_text = std::fs::read_to_string(&path).expect("wire file");
        std::fs::remove_file(&path).ok();
        assert!(wire_text.contains(" tx "), "{wire_text}");
        assert!(wire_text.contains(" rx "), "{wire_text}");
        // A CI-V frame to the radio starts fe fe; none of them may be a PTT set (1c 00 01).
        assert!(!wire_text.contains("1c 00 01"), "{wire_text}");
        let header = ReportHeader {
            rig_id: description.id.clone(),
            rig_name: description.name.clone(),
            family: format!("{:?}", description.family),
            target: redact_target("virtual"),
            baud: 19_200,
            started: utc_stamp(0),
            verified: BTreeMap::from([("unverified", 3)]),
        };
        let report = render_report(&header, &results, &wire_text);
        assert!(report.contains("nothing was transmitted"));
        assert!(report.contains("PASS read frequency A"));
        assert!(report.contains("summary: "));
        assert!(report.contains("3 unverified"));
        assert!(report.contains(" tx "));
    }
}
