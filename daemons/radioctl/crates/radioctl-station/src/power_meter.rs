// SPDX-License-Identifier: AGPL-3.0-or-later
//! External RF power meters: sans-IO codecs for the Elecraft KPA500 and KPA1500 amplifiers'
//! output-power read commands, an async poller over a byte stream, the freshness-checked
//! [`PowerFeed`] that implements [`RfPowerReader`], and a virtual device for tests
//! (`docs/DESIGN.md` decision 26).
//!
//! Facts (read from Elecraft's own documents, kept outside the repository; none verified on a
//! radio or an amplifier):
//! - KPA500 Programmer's Reference, Rev A2, p.1 and p.4: `^WS;` answers `^WSppp sss;`, output
//!   watts (three digits) and SWR in tenths (three digits, `000` when not transmitting); no
//!   reflected-power field; 38400 baud by default.
//! - KPA1500 Programming Reference (firmware 03.10), p.43: `^PWF;` answers `^PWFnnnn;` forward
//!   watts and `^PWR;` answers `^PWRnnnn;` reflected watts; one request at a time.
//!
//! Both meters sit inside the amplifier, so they report its output and need no external coupler;
//! the offset in the configuration is for a meter placed behind a coupler or attenuator.
//! Nothing here transmits: the commands are reads.

use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant};

use radioctl_core::power::{Power, PowerOrigin, PowerReading, PowerUnit, RfPowerReader};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio::time::timeout;

use crate::error::{StationError, StationErrorKind};

/// Which wattmeter protocol a meter speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeterDriver {
    /// The Elecraft KPA500 amplifier's `^WS` read.
    Kpa500,
    /// The Elecraft KPA1500 amplifier's `^PWF` and `^PWR` reads.
    Kpa1500,
}

impl MeterDriver {
    // [@ANCHOR: radioctl_station_meter_driver_name]
    /// The configuration name.
    pub fn name(self) -> &'static str {
        match self {
            MeterDriver::Kpa500 => "kpa500",
            MeterDriver::Kpa1500 => "kpa1500",
        }
    }

    // [@ANCHOR: radioctl_station_meter_driver_from_name]
    /// The driver with a configuration name.
    pub fn from_name(name: &str) -> Option<MeterDriver> {
        [MeterDriver::Kpa500, MeterDriver::Kpa1500]
            .into_iter()
            .find(|d| d.name() == name)
    }

    // [@ANCHOR: radioctl_station_meter_driver_unit]
    /// The unit the driver reports in (both read watts).
    pub fn unit(self) -> PowerUnit {
        PowerUnit::Watts
    }

    // [@ANCHOR: radioctl_station_meter_driver_default_baud]
    /// The serial speed the meter's document gives as its default, 38400 for the KPA500; the
    /// KPA1500 document names none, and 38400 is the usual choice there too (unverified).
    pub fn default_baud(self) -> u32 {
        38_400
    }

    // [@ANCHOR: radioctl_station_meter_driver_requests]
    /// The requests of one poll, in order. Each is sent after the previous one's reply.
    pub fn requests(self) -> &'static [&'static [u8]] {
        match self {
            MeterDriver::Kpa500 => &[b"^WS;"],
            MeterDriver::Kpa1500 => &[b"^PWF;", b"^PWR;"],
        }
    }

    // [@ANCHOR: radioctl_station_meter_driver_has_reflected]
    /// Whether the meter reports reflected power.
    pub fn has_reflected(self) -> bool {
        matches!(self, MeterDriver::Kpa1500)
    }
}

/// What one poll learned, in the driver's unit.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Sample {
    /// Forward power.
    pub forward: f64,
    /// Reflected power, when the meter reports it.
    pub reflected: Option<f64>,
}

fn proto(detail: impl Into<String>) -> StationError {
    StationError::new(StationErrorKind::Driver, detail)
}

/// The digits of `text` as a whole number, nothing else allowed.
fn digits(text: &str) -> Option<u32> {
    (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

// [@ANCHOR: radioctl_station_meter_parse_kpa500]
/// Parses a KPA500 `^WSppp sss;` frame into the output watts. The SWR field is checked but not
/// used (a power meter reports power).
pub fn parse_kpa500(frame: &str) -> Result<Sample, StationError> {
    let body = frame
        .strip_prefix("^WS")
        .and_then(|b| b.strip_suffix(';'))
        .ok_or_else(|| proto(format!("kpa500: not a ^WS reply: {frame:?}")))?;
    let (watts, swr) = body
        .split_once(' ')
        .ok_or_else(|| proto(format!("kpa500: malformed ^WS reply: {frame:?}")))?;
    let watts =
        digits(watts).ok_or_else(|| proto(format!("kpa500: bad power field in {frame:?}")))?;
    digits(swr).ok_or_else(|| proto(format!("kpa500: bad SWR field in {frame:?}")))?;
    Ok(Sample {
        forward: f64::from(watts),
        reflected: None,
    })
}

// [@ANCHOR: radioctl_station_meter_parse_kpa1500]
/// Parses one KPA1500 frame, `^PWFnnnn;` (forward) or `^PWRnnnn;` (reflected): the tag and the
/// watts.
pub fn parse_kpa1500(frame: &str) -> Result<(char, f64), StationError> {
    let body = frame
        .strip_prefix("^PW")
        .and_then(|b| b.strip_suffix(';'))
        .ok_or_else(|| proto(format!("kpa1500: not a ^PW reply: {frame:?}")))?;
    let mut chars = body.chars();
    let tag = chars
        .next()
        .filter(|c| matches!(c, 'F' | 'R'))
        .ok_or_else(|| proto(format!("kpa1500: unknown ^PW reply {frame:?}")))?;
    let watts = digits(chars.as_str())
        .ok_or_else(|| proto(format!("kpa1500: bad power field in {frame:?}")))?;
    Ok((tag, f64::from(watts)))
}

/// The longest reply frame accepted.
const MAX_FRAME: usize = 32;

/// An async poller for one external wattmeter on a byte stream.
pub struct MeterPoller<T> {
    driver: MeterDriver,
    io: Mutex<T>,
    reply_timeout: Duration,
}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> MeterPoller<T> {
    // [@ANCHOR: radioctl_station_meter_poller_new]
    /// A poller for `driver` over `io`. Nothing is sent until [`MeterPoller::poll`].
    pub fn new(driver: MeterDriver, io: T, reply_timeout: Duration) -> MeterPoller<T> {
        MeterPoller {
            driver,
            io: Mutex::new(io),
            reply_timeout,
        }
    }

    // [@ANCHOR: radioctl_station_meter_poller_read_frame]
    /// Reads bytes up to the `;` that ends a frame, bounded in size and time. Bytes before the
    /// frame's `^` (the echo of a wake-up `;`, line noise) are dropped.
    async fn read_frame(&self, io: &mut T) -> Result<String, StationError> {
        let mut frame = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            match timeout(self.reply_timeout, io.read(&mut byte)).await {
                Ok(Ok(1)) => {}
                Ok(Ok(_)) => return Err(proto("power meter: the port closed")),
                Ok(Err(e)) => return Err(proto(format!("power meter: read failed: {e}"))),
                Err(_) => {
                    return Err(StationError::new(
                        StationErrorKind::Timeout,
                        "power meter: no reply",
                    ))
                }
            }
            if frame.is_empty() && byte[0] != b'^' {
                continue;
            }
            frame.push(byte[0]);
            if byte[0] == b';' {
                return String::from_utf8(frame).map_err(|_| proto("power meter: non-text reply"));
            }
            if frame.len() > MAX_FRAME {
                return Err(proto("power meter: reply too long"));
            }
        }
    }

    // [@ANCHOR: radioctl_station_meter_poller_poll]
    /// Asks the meter once and returns what it said. A timeout or a malformed reply is an error;
    /// the caller then simply has no new reading, and the old one ages out.
    pub async fn poll(&self) -> Result<Sample, StationError> {
        let mut io = self.io.lock().await;
        let mut sample = Sample::default();
        for request in self.driver.requests() {
            io.write_all(request)
                .await // lock-scope-ignore: a serial bus is half duplex; the whole request/reply exchange must be serialized
                .map_err(|e| proto(format!("power meter: write failed: {e}")))?;
            let frame = self.read_frame(&mut io).await?; // lock-scope-ignore: a serial bus is half duplex; the whole request/reply exchange must be serialized
            match self.driver {
                MeterDriver::Kpa500 => sample = parse_kpa500(&frame)?,
                MeterDriver::Kpa1500 => match parse_kpa1500(&frame)? {
                    ('F', w) => sample.forward = w,
                    (_, w) => sample.reflected = Some(w),
                },
            }
        }
        Ok(sample)
    }
}

/// A source of the current time, replaceable in tests.
pub type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

struct FeedState {
    at: Instant,
    sample: Sample,
}

/// The latest reading of one external meter, with the rule that makes a stale one "no power".
pub struct PowerFeed {
    name: String,
    unit: PowerUnit,
    offset_db: f64,
    stale_after: Duration,
    clock: Clock,
    state: StdMutex<Option<FeedState>>,
}

impl PowerFeed {
    // [@ANCHOR: radioctl_station_feed_new]
    /// A feed named `name` for a meter that reports in `unit`, behind `offset_db` of coupler and
    /// attenuator loss, whose readings are good for `stale_after`.
    pub fn new(name: &str, unit: PowerUnit, offset_db: f64, stale_after: Duration) -> PowerFeed {
        PowerFeed::with_clock(name, unit, offset_db, stale_after, Arc::new(Instant::now))
    }

    // [@ANCHOR: radioctl_station_feed_with_clock]
    /// As [`PowerFeed::new`] with the clock the freshness rule reads.
    pub fn with_clock(
        name: &str,
        unit: PowerUnit,
        offset_db: f64,
        stale_after: Duration,
        clock: Clock,
    ) -> PowerFeed {
        PowerFeed {
            name: name.to_string(),
            unit,
            offset_db,
            stale_after,
            clock,
            state: StdMutex::new(None),
        }
    }

    // [@ANCHOR: radioctl_station_feed_push]
    /// Records a sample taken now.
    pub fn push(&self, sample: Sample) {
        let at = (self.clock)();
        *self.state.lock().unwrap_or_else(|p| p.into_inner()) = Some(FeedState { at, sample });
    }

    // [@ANCHOR: radioctl_station_feed_name]
    /// The meter's name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl RfPowerReader for PowerFeed {
    // [@ANCHOR: radioctl_station_feed_read]
    fn read(&self) -> Option<PowerReading> {
        let guard = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let state = guard.as_ref()?;
        let age = (self.clock)().saturating_duration_since(state.at);
        if age > self.stale_after {
            return None;
        }
        let forward = self.unit.to_power(state.sample.forward, self.offset_db)?;
        let reflected = state
            .sample
            .reflected
            .and_then(|r| self.unit.to_power(r, self.offset_db));
        Some(PowerReading {
            forward,
            reflected,
            origin: PowerOrigin::External(self.name.clone()),
            valid_for: self.stale_after - age,
            relative: false,
            scale: None::<Power>,
        })
    }
}

// [@ANCHOR: radioctl_station_meter_run]
/// Polls `poller` every `interval` into `feed` until the task is dropped. A failed poll leaves
/// the feed alone, so its last reading ages out.
pub async fn run_poller<T: AsyncRead + AsyncWrite + Unpin + Send>(
    poller: MeterPoller<T>,
    feed: Arc<PowerFeed>,
    interval: Duration,
) {
    let mut tick = tokio::time::interval(interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        if let Ok(sample) = poller.poll().await {
            feed.push(sample);
        }
    }
}

/// What a virtual meter reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VirtualPower {
    /// Forward watts.
    pub forward_w: u32,
    /// Reflected watts (KPA1500).
    pub reflected_w: u32,
    /// SWR in tenths (KPA500).
    pub swr_tenths: u32,
    /// The device is off or silent: it answers nothing.
    pub silent: bool,
}

/// Control of a virtual meter.
#[derive(Clone)]
pub struct VirtualMeterControl {
    state: Arc<StdMutex<VirtualPower>>,
}

impl VirtualMeterControl {
    // [@ANCHOR: radioctl_station_vmeter_set]
    /// Sets what the device reports.
    pub fn set(&self, value: VirtualPower) {
        *self.state.lock().unwrap_or_else(|p| p.into_inner()) = value;
    }
}

// [@ANCHOR: radioctl_station_vmeter_serve]
/// Serves a virtual meter of `driver` on `io` (the far end of a duplex stream or a pty) until the
/// stream closes, and returns its control. It answers the documented reads and ignores anything
/// else, as the real amplifiers do.
pub fn serve_virtual_meter<T>(driver: MeterDriver, mut io: T) -> VirtualMeterControl
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let state = Arc::new(StdMutex::new(VirtualPower {
        forward_w: 0,
        reflected_w: 0,
        swr_tenths: 0,
        silent: false,
    }));
    let task_state = state.clone();
    tokio::spawn(async move {
        let mut pending = Vec::new();
        let mut byte = [0u8; 1];
        while matches!(io.read(&mut byte).await, Ok(1)) {
            pending.push(byte[0]);
            if byte[0] != b';' {
                continue;
            }
            let command = String::from_utf8_lossy(&pending).to_ascii_uppercase();
            pending.clear();
            let v = *task_state.lock().unwrap_or_else(|p| p.into_inner());
            if v.silent {
                continue;
            }
            let reply = match (driver, command.as_str()) {
                (MeterDriver::Kpa500, "^WS;") => {
                    format!(
                        "^WS{:03} {:03};",
                        v.forward_w.min(999),
                        v.swr_tenths.min(999)
                    )
                }
                (MeterDriver::Kpa1500, "^PWF;") => format!("^PWF{:04};", v.forward_w.min(9999)),
                (MeterDriver::Kpa1500, "^PWR;") => format!("^PWR{:04};", v.reflected_w.min(9999)),
                (_, ";") => ";".to_string(),
                _ => continue,
            };
            if io.write_all(reply.as_bytes()).await.is_err() {
                break;
            }
        }
    });
    VirtualMeterControl { state }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    // Tests [@ANCHOR: radioctl_station_meter_driver_name]
    // Tests [@ANCHOR: radioctl_station_meter_driver_from_name]
    // Tests [@ANCHOR: radioctl_station_meter_driver_unit]
    // Tests [@ANCHOR: radioctl_station_meter_driver_default_baud]
    // Tests [@ANCHOR: radioctl_station_meter_driver_requests]
    // Tests [@ANCHOR: radioctl_station_meter_driver_has_reflected]
    #[test]
    fn drivers_have_names_requests_and_units() {
        for d in [MeterDriver::Kpa500, MeterDriver::Kpa1500] {
            assert_eq!(MeterDriver::from_name(d.name()), Some(d));
            assert_eq!(d.unit(), PowerUnit::Watts);
            assert_eq!(d.default_baud(), 38_400);
        }
        assert_eq!(MeterDriver::from_name("lp100a"), None);
        assert_eq!(MeterDriver::Kpa500.requests(), [b"^WS;".as_slice()]);
        assert_eq!(MeterDriver::Kpa1500.requests().len(), 2);
        assert!(MeterDriver::Kpa1500.has_reflected() && !MeterDriver::Kpa500.has_reflected());
    }

    // Tests [@ANCHOR: radioctl_station_meter_parse_kpa500]
    #[test]
    fn kpa500_replies_parse_and_malformed_ones_do_not() {
        let s = parse_kpa500("^WS100 015;").expect("parses");
        assert_eq!(s.forward, 100.0);
        assert_eq!(s.reflected, None);
        assert_eq!(parse_kpa500("^WS000 000;").expect("idle").forward, 0.0);
        for bad in [
            "^WS100;",
            "^WS1x0 015;",
            "^WS100 01;x",
            "WS100 015;",
            "^WS100 015",
            "^WS 015;",
        ] {
            assert!(parse_kpa500(bad).is_err(), "{bad}");
        }
    }

    // Tests [@ANCHOR: radioctl_station_meter_parse_kpa1500]
    #[test]
    fn kpa1500_frames_parse_and_malformed_ones_do_not() {
        assert_eq!(parse_kpa1500("^PWF1204;").expect("ok"), ('F', 1204.0));
        assert_eq!(parse_kpa1500("^PWR0012;").expect("ok"), ('R', 12.0));
        for bad in ["^PWX0012;", "^PWF;", "^PWF12a4;", "^PWF0012", "^SW123;"] {
            assert!(parse_kpa1500(bad).is_err(), "{bad}");
        }
    }

    // Tests [@ANCHOR: radioctl_station_meter_poller_new]
    // Tests [@ANCHOR: radioctl_station_meter_poller_poll]
    // Tests [@ANCHOR: radioctl_station_meter_poller_read_frame]
    // Tests [@ANCHOR: radioctl_station_vmeter_serve]
    // Tests [@ANCHOR: radioctl_station_vmeter_set]
    #[tokio::test]
    async fn pollers_read_both_meters_from_virtual_devices() {
        for driver in [MeterDriver::Kpa500, MeterDriver::Kpa1500] {
            let (near, far) = duplex(256);
            let control = serve_virtual_meter(driver, far);
            control.set(VirtualPower {
                forward_w: 1204,
                reflected_w: 12,
                swr_tenths: 14,
                silent: false,
            });
            let poller = MeterPoller::new(driver, near, ms(500));
            let s = poller.poll().await.expect("answers");
            if driver == MeterDriver::Kpa500 {
                // Three digits only: the KPA500 tops out at 999 W.
                assert_eq!(s.forward, 999.0);
                assert_eq!(s.reflected, None);
            } else {
                assert_eq!(s.forward, 1204.0);
                assert_eq!(s.reflected, Some(12.0));
            }
            control.set(VirtualPower {
                forward_w: 100,
                reflected_w: 0,
                swr_tenths: 15,
                silent: false,
            });
            assert_eq!(poller.poll().await.expect("answers").forward, 100.0);
        }
    }

    // Tests [@ANCHOR: radioctl_station_meter_poller_poll]
    #[tokio::test]
    async fn a_silent_meter_times_out_and_garbage_is_an_error() {
        let (near, far) = duplex(256);
        let control = serve_virtual_meter(MeterDriver::Kpa500, far);
        control.set(VirtualPower {
            forward_w: 5,
            reflected_w: 0,
            swr_tenths: 10,
            silent: true,
        });
        let poller = MeterPoller::new(MeterDriver::Kpa500, near, ms(60));
        let err = poller.poll().await.expect_err("silent");
        assert_eq!(err.kind, StationErrorKind::Timeout);
        // A device that answers with something else.
        let (near, mut far) = duplex(256);
        tokio::spawn(async move {
            let mut b = [0u8; 4];
            let _ = far.read_exact(&mut b).await;
            let _ = far.write_all(b"^ZZ1;").await;
        });
        let poller = MeterPoller::new(MeterDriver::Kpa500, near, ms(200));
        assert!(poller.poll().await.is_err());
        // A closed port.
        let (near, far) = duplex(16);
        drop(far);
        let poller = MeterPoller::new(MeterDriver::Kpa500, near, ms(60));
        assert!(poller.poll().await.is_err());
    }

    fn manual_clock() -> (Clock, Arc<StdMutex<Instant>>) {
        let now = Arc::new(StdMutex::new(Instant::now()));
        let handle = now.clone();
        (Arc::new(move || *handle.lock().expect("clock")), now)
    }

    // Tests [@ANCHOR: radioctl_station_feed_new]
    // Tests [@ANCHOR: radioctl_station_feed_with_clock]
    // Tests [@ANCHOR: radioctl_station_feed_push]
    // Tests [@ANCHOR: radioctl_station_feed_read]
    // Tests [@ANCHOR: radioctl_station_feed_name]
    #[test]
    fn a_stale_reading_is_no_power() {
        let (clock, now) = manual_clock();
        let feed = PowerFeed::with_clock("amp", PowerUnit::Watts, 0.0, ms(1000), clock);
        assert!(feed.read().is_none(), "nothing yet");
        feed.push(Sample {
            forward: 100.0,
            reflected: Some(4.0),
        });
        let r = feed.read().expect("fresh");
        assert_eq!(r.forward.watts(), 100.0);
        assert_eq!(r.reflected.map(Power::watts), Some(4.0));
        assert_eq!(r.origin, PowerOrigin::External("amp".into()));
        assert!(!r.relative);
        assert_eq!(feed.name(), "amp");
        *now.lock().expect("clock") += ms(900);
        let r = feed.read().expect("still fresh");
        assert!(r.valid_for <= ms(100));
        *now.lock().expect("clock") += ms(200);
        assert!(
            feed.read().is_none(),
            "stale is no power, not the last number"
        );
        feed.push(Sample {
            forward: 7.0,
            reflected: None,
        });
        assert_eq!(feed.read().expect("fresh again").forward.watts(), 7.0);
    }

    // Tests [@ANCHOR: radioctl_station_feed_read]
    #[test]
    fn the_offset_and_unit_are_applied_at_the_feed() {
        // A dBm sensor behind 40 dB of coupler and attenuator: 10 dBm is 100 W.
        let feed = PowerFeed::new("sensor", PowerUnit::Dbm, 40.0, ms(1000));
        feed.push(Sample {
            forward: 10.0,
            reflected: Some(-10.0),
        });
        let r = feed.read().expect("fresh");
        assert!((r.forward.watts() - 100.0).abs() < 1e-9);
        assert!((r.reflected.expect("reflected").watts() - 1.0).abs() < 1e-9);
    }

    // Tests [@ANCHOR: radioctl_station_meter_run]
    #[tokio::test]
    async fn the_poller_task_keeps_a_feed_fresh_until_the_meter_goes_quiet() {
        let (near, far) = duplex(256);
        let control = serve_virtual_meter(MeterDriver::Kpa1500, far);
        control.set(VirtualPower {
            forward_w: 50,
            reflected_w: 2,
            swr_tenths: 0,
            silent: false,
        });
        let feed = Arc::new(PowerFeed::new("amp", PowerUnit::Watts, 0.0, ms(150)));
        let task = tokio::spawn(run_poller(
            MeterPoller::new(MeterDriver::Kpa1500, near, ms(40)),
            feed.clone(),
            ms(20),
        ));
        let started = Instant::now();
        while feed.read().is_none() {
            assert!(started.elapsed() < Duration::from_secs(3), "no reading");
            tokio::time::sleep(ms(10)).await;
        }
        assert_eq!(feed.read().expect("fresh").forward.watts(), 50.0);
        control.set(VirtualPower {
            forward_w: 50,
            reflected_w: 2,
            swr_tenths: 0,
            silent: true,
        });
        let started = Instant::now();
        while feed.read().is_some() {
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "never went stale"
            );
            tokio::time::sleep(ms(10)).await;
        }
        task.abort();
    }
}
