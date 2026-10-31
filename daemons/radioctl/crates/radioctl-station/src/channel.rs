// SPDX-License-Identifier: AGPL-3.0-or-later
//! Channel drivers and the switchboard: commanded and confirmed state for every channel, events
//! on every change, break-before-make selectors, momentary pulses and debounced inputs.
//!
//! A driver is a bank of numbered binary lines (a relay board, a Shelly, an FX2 port). The
//! switchboard turns typed channels into line operations and never trusts a write: where the
//! driver can read back it does, and a read-back that differs is a `Mismatch` event and an error.
//! Where a driver cannot read back, the channel reports commanded state only.

use crate::error::{StationError, StationErrorKind};
use crate::model::{ChannelDef, ChannelId, ChannelKind, Value};
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex as StdMutex, MutexGuard};
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{sleep, Instant};

/// A boxed future, so drivers can be used as trait objects.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// How far the station can rely on a driver to take a device to its safe state, and how directly.
/// Ordered from least to most.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Assurance {
    /// The command reaches the device through another system that can be down, slow or stale
    /// (Home Assistant). Never a PTT path, and an amplifier behind it needs an independent stop.
    Mediated,
    /// The host talks to the device directly and supervises it, but nothing in the device stops
    /// it if the host dies (a relay board, a Shelly without its auto-off timer).
    HostSupervised,
    /// The device itself has a dead-man that acts when the host goes silent (the FX2 firmware).
    HardwareDeadMan,
}

/// A bank of numbered binary lines.
pub trait ChannelDriver: Send + Sync {
    // [@ANCHOR: radioctl_station_driver_assurance]
    /// The assurance level of channels on this driver; host-supervised unless the driver says
    /// otherwise.
    fn assurance(&self) -> Assurance {
        Assurance::HostSupervised
    }

    /// Drives one line. Success means the device accepted the command, not that the contact moved.
    fn write_line(&self, line: u32, on: bool) -> BoxFuture<'_, Result<(), StationError>>;
    /// Reads one line (the device's own report). Fails for a driver that cannot read back.
    fn read_line(&self, line: u32) -> BoxFuture<'_, Result<bool, StationError>>;
    /// Whether [`ChannelDriver::read_line`] reports the device's state.
    fn can_read_back(&self) -> bool;
}

/// Something that happened in the station.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StationEvent {
    /// A command was issued (before it is confirmed).
    Commanded {
        /// The channel.
        channel: ChannelId,
        /// The commanded value.
        value: Value,
    },
    /// The read-back agrees with the command.
    Confirmed {
        /// The channel.
        channel: ChannelId,
        /// The confirmed value.
        value: Value,
    },
    /// The read-back differs from the command: an alarm.
    Mismatch {
        /// The channel.
        channel: ChannelId,
        /// What was commanded.
        commanded: Value,
        /// What was read back (`None`: ambiguous or unreadable).
        confirmed: Option<Value>,
    },
    /// The driver failed.
    DriverFault {
        /// The channel.
        channel: ChannelId,
        /// What happened.
        detail: String,
    },
    /// A debounced input changed.
    InputChanged {
        /// The channel.
        channel: ChannelId,
        /// The new level.
        high: bool,
    },
    /// A momentary pulse completed.
    Pulsed {
        /// The channel.
        channel: ChannelId,
    },
    /// A command was refused by an interlock or a dependency.
    Refused {
        /// What was asked.
        what: String,
        /// Why not.
        why: String,
    },
    /// A profile or power sequence reached a step.
    Step {
        /// The profile or operation.
        name: String,
        /// The step number from 0.
        index: usize,
        /// The text.
        detail: String,
    },
    /// The emergency all-off ran.
    AllOff {
        /// Channels that did not go off.
        failed: Vec<ChannelId>,
    },
}

/// What the switchboard knows about one channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelStatus {
    /// What was last commanded (`None`: nothing since start).
    pub commanded: Option<Value>,
    /// What was last confirmed by read-back or sensor (`None`: unknown).
    pub confirmed: Option<Value>,
    /// Whether this channel's driver can read back at all.
    pub can_confirm: bool,
}

/// The result of a successful set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetOutcome {
    /// What was commanded.
    pub commanded: Value,
    /// What was read back, when the driver can.
    pub confirmed: Option<Value>,
}

/// Faults a virtual bank can inject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// Behaves.
    None,
    /// Every write fails.
    FailWrites,
    /// Writes are accepted and have no effect (a stuck relay).
    IgnoreWrites,
    /// Every read fails.
    FailReads,
    /// Reads report the opposite of the truth.
    WrongReadback,
    /// The first n writes work, then every write fails (a drop mid-sequence).
    DropAfter(usize),
}

#[derive(Debug)]
struct BankInner {
    lines: BTreeMap<u32, bool>,
    log: Vec<(u32, bool)>,
    fault: Fault,
    writes_done: usize,
    readback: bool,
}

/// A virtual bank of lines with fault injection, for tests and for running a station without
/// hardware. Clones share the same lines.
#[derive(Debug, Clone)]
pub struct VirtualBank {
    inner: Arc<StdMutex<BankInner>>,
}

fn lock<T>(m: &StdMutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl VirtualBank {
    // [@ANCHOR: radioctl_station_vbank_new]
    /// A bank whose lines all start off, with read-back.
    pub fn new() -> VirtualBank {
        VirtualBank::build(true)
    }

    // [@ANCHOR: radioctl_station_vbank_no_readback]
    /// A bank that cannot be read back.
    pub fn without_readback() -> VirtualBank {
        VirtualBank::build(false)
    }

    fn build(readback: bool) -> VirtualBank {
        VirtualBank {
            inner: Arc::new(StdMutex::new(BankInner {
                lines: BTreeMap::new(),
                log: Vec::new(),
                fault: Fault::None,
                writes_done: 0,
                readback,
            })),
        }
    }

    // [@ANCHOR: radioctl_station_vbank_set_fault]
    /// Injects a fault.
    pub fn set_fault(&self, fault: Fault) {
        let mut g = lock(&self.inner);
        g.fault = fault;
        g.writes_done = 0;
    }

    // [@ANCHOR: radioctl_station_vbank_line]
    /// The real (virtual) state of a line.
    pub fn line(&self, line: u32) -> bool {
        lock(&self.inner).lines.get(&line).copied().unwrap_or(false)
    }

    // [@ANCHOR: radioctl_station_vbank_set_physical]
    /// Sets a line behind the switchboard's back (a sensor changing, a hand-flipped switch).
    pub fn set_physical(&self, line: u32, on: bool) {
        lock(&self.inner).lines.insert(line, on);
    }

    // [@ANCHOR: radioctl_station_vbank_log]
    /// Every write the bank accepted or ignored, in order.
    pub fn log(&self) -> Vec<(u32, bool)> {
        lock(&self.inner).log.clone()
    }
}

impl Default for VirtualBank {
    // [@ANCHOR: radioctl_station_vbank_default]
    /// Same as [`VirtualBank::new`].
    fn default() -> VirtualBank {
        VirtualBank::new()
    }
}

impl ChannelDriver for VirtualBank {
    // [@ANCHOR: radioctl_station_vbank_write_line]
    /// Applies a write unless a fault says otherwise.
    fn write_line(&self, line: u32, on: bool) -> BoxFuture<'_, Result<(), StationError>> {
        Box::pin(async move {
            let mut g = lock(&self.inner);
            let fail = match g.fault {
                Fault::FailWrites => true,
                Fault::DropAfter(n) => g.writes_done >= n,
                _ => false,
            };
            if fail {
                return Err(StationError::new(
                    StationErrorKind::Driver,
                    "virtual bank: write failed",
                ));
            }
            g.writes_done += 1;
            g.log.push((line, on));
            if g.fault != Fault::IgnoreWrites {
                g.lines.insert(line, on);
            }
            Ok(())
        })
    }

    // [@ANCHOR: radioctl_station_vbank_read_line]
    /// Reads a line unless a fault or the lack of read-back says otherwise.
    fn read_line(&self, line: u32) -> BoxFuture<'_, Result<bool, StationError>> {
        Box::pin(async move {
            let g = lock(&self.inner);
            if !g.readback || g.fault == Fault::FailReads {
                return Err(StationError::new(
                    StationErrorKind::Driver,
                    "virtual bank: no read-back",
                ));
            }
            let v = g.lines.get(&line).copied().unwrap_or(false);
            Ok(if g.fault == Fault::WrongReadback {
                !v
            } else {
                v
            })
        })
    }

    // [@ANCHOR: radioctl_station_vbank_can_read_back]
    /// Whether this bank reads back.
    fn can_read_back(&self) -> bool {
        lock(&self.inner).readback
    }
}

/// A sans-IO input debouncer: a new level is believed only after it has held for the debounce
/// time. Time is passed in, so tests need no clock.
#[derive(Debug, Clone)]
pub struct Debouncer {
    debounce: Duration,
    candidate: Option<bool>,
    since: Duration,
    stable: Option<bool>,
}

impl Debouncer {
    // [@ANCHOR: radioctl_station_debouncer_new]
    /// A debouncer with nothing believed yet.
    pub fn new(debounce: Duration) -> Debouncer {
        Debouncer {
            debounce,
            candidate: None,
            since: Duration::ZERO,
            stable: None,
        }
    }

    // [@ANCHOR: radioctl_station_debouncer_sample]
    /// Feeds one raw sample taken at `now` (time since any fixed start). Returns the new stable
    /// level when this sample made one.
    pub fn sample(&mut self, now: Duration, raw: bool) -> Option<bool> {
        if self.candidate != Some(raw) {
            self.candidate = Some(raw);
            self.since = now;
        }
        if self.stable != Some(raw) && now.saturating_sub(self.since) >= self.debounce {
            self.stable = Some(raw);
            return Some(raw);
        }
        None
    }

    // [@ANCHOR: radioctl_station_debouncer_stable]
    /// The believed level, if any.
    pub fn stable(&self) -> Option<bool> {
        self.stable
    }
}

struct Chan {
    def: ChannelDef,
    driver: Arc<dyn ChannelDriver>,
    op: Mutex<()>,
    state: StdMutex<State>,
}

#[derive(Debug, Clone, Copy, Default)]
struct State {
    commanded: Option<Value>,
    confirmed: Option<Value>,
    last_off: Option<Instant>,
}

/// All channels of a station, their drivers and their state.
pub struct Switchboard {
    channels: HashMap<ChannelId, Chan>,
    events: broadcast::Sender<StationEvent>,
}

fn config(m: impl Into<String>) -> StationError {
    StationError::new(StationErrorKind::Config, m)
}

impl Default for Switchboard {
    // [@ANCHOR: radioctl_station_board_default]
    /// An empty switchboard.
    fn default() -> Switchboard {
        Switchboard::new()
    }
}

impl Switchboard {
    // [@ANCHOR: radioctl_station_board_new]
    /// An empty switchboard.
    pub fn new() -> Switchboard {
        let (events, _) = broadcast::channel(256);
        Switchboard {
            channels: HashMap::new(),
            events,
        }
    }

    // [@ANCHOR: radioctl_station_board_add_channel]
    /// Adds a validated channel on `driver`. Nothing is written.
    pub fn add_channel(
        &mut self,
        def: ChannelDef,
        driver: Arc<dyn ChannelDriver>,
    ) -> Result<(), StationError> {
        def.validate()?;
        if self.channels.contains_key(&def.id) {
            return Err(config(format!("channel {} added twice", def.id.0)));
        }
        if matches!(def.kind, ChannelKind::Ptt { .. }) && driver.assurance() == Assurance::Mediated
        {
            return Err(config(format!(
                "channel {} is a ptt channel on a driver that goes through another system; \
                 nothing time-critical may",
                def.id.0
            )));
        }
        self.channels.insert(
            def.id.clone(),
            Chan {
                def,
                driver,
                op: Mutex::new(()),
                state: StdMutex::new(State::default()),
            },
        );
        Ok(())
    }

    // [@ANCHOR: radioctl_station_board_assurance]
    /// The assurance level of a channel's driver.
    pub fn assurance(&self, id: &ChannelId) -> Result<Assurance, StationError> {
        Ok(self.chan(id)?.driver.assurance())
    }

    // [@ANCHOR: radioctl_station_board_subscribe]
    /// A receiver of every event from now on.
    pub fn subscribe(&self) -> broadcast::Receiver<StationEvent> {
        self.events.subscribe()
    }

    // [@ANCHOR: radioctl_station_board_emit]
    /// Publishes an event (nobody listening is fine).
    pub(crate) fn emit(&self, event: StationEvent) {
        let _ = self.events.send(event);
    }

    fn chan(&self, id: &ChannelId) -> Result<&Chan, StationError> {
        self.channels.get(id).ok_or_else(|| {
            StationError::new(StationErrorKind::Unknown, format!("no channel {}", id.0))
        })
    }

    // [@ANCHOR: radioctl_station_board_def]
    /// The definition of a channel.
    pub fn def(&self, id: &ChannelId) -> Result<&ChannelDef, StationError> {
        Ok(&self.chan(id)?.def)
    }

    // [@ANCHOR: radioctl_station_board_ids]
    /// Every channel name, sorted.
    pub fn ids(&self) -> Vec<ChannelId> {
        let mut v: Vec<ChannelId> = self.channels.keys().cloned().collect();
        v.sort();
        v
    }

    // [@ANCHOR: radioctl_station_board_status]
    /// Commanded and confirmed state of a channel.
    pub fn status(&self, id: &ChannelId) -> Result<ChannelStatus, StationError> {
        let c = self.chan(id)?;
        let s = *lock(&c.state);
        Ok(ChannelStatus {
            commanded: s.commanded,
            confirmed: s.confirmed,
            can_confirm: c.driver.can_read_back(),
        })
    }

    // [@ANCHOR: radioctl_station_board_store]
    /// Records commanded and/or confirmed state. The off-time stamp for the minimum off time is
    /// taken only on an observed or commanded edge from a known non-off value to off: a channel
    /// first seen off (the start-up read-back, or the first read after a driver fault) has no
    /// known turn-off time, and the minimum off time is a protection for a device that was just
    /// running, so it must not delay a power-on after a restart of the software.
    fn store(&self, c: &Chan, commanded: Option<Option<Value>>, confirmed: Option<Option<Value>>) {
        let mut s = lock(&c.state);
        let edge = |old: Option<Value>, new: Option<Value>| {
            new == Some(Value::Off) && old.is_some_and(|v| v != Value::Off)
        };
        if let Some(v) = commanded {
            if edge(s.commanded, v) {
                s.last_off = Some(Instant::now());
            }
            s.commanded = v;
        }
        if let Some(v) = confirmed {
            if edge(s.confirmed, v) {
                s.last_off = Some(Instant::now());
            }
            s.confirmed = v;
        }
    }

    fn fault(&self, c: &Chan, e: &StationError) {
        self.store(c, None, Some(None));
        self.emit(StationEvent::DriverFault {
            channel: c.def.id.clone(),
            detail: e.detail.clone(),
        });
    }

    // [@ANCHOR: radioctl_station_board_read_value]
    /// Reads a binary channel, or a selector's lines, as one value. `Ok(None)`: ambiguous
    /// (more than one way closed).
    async fn read_value(&self, c: &Chan) -> Result<Option<Value>, StationError> {
        if let ChannelKind::Selector { .. } = c.def.kind {
            let mut on = Vec::new();
            for (i, line) in c.def.lines().into_iter().enumerate() {
                if c.driver.read_line(line).await? {
                    on.push(i as u32 + 1);
                }
            }
            return Ok(match on.as_slice() {
                [] => Some(Value::Off),
                [n] => Some(Value::Port(*n)),
                _ => None,
            });
        }
        let level = c.driver.read_line(c.def.line).await?;
        Ok(Some(if level { Value::On } else { Value::Off }))
    }

    // [@ANCHOR: radioctl_station_board_read_back]
    /// Reads the real state of a channel from the driver and records it as confirmed, leaving the
    /// commanded state alone. Used at start-up: it writes nothing.
    pub async fn read_back(&self, id: &ChannelId) -> Result<Option<Value>, StationError> {
        let c = self.chan(id)?;
        let _op = c.op.lock().await;
        match self.read_value(c).await {
            Ok(v) => {
                self.store(c, None, Some(v));
                Ok(v)
            }
            Err(e) => {
                self.fault(c, &e);
                Err(e)
            }
        }
    }

    // [@ANCHOR: radioctl_station_board_verify]
    /// Compares the read-back with `want`, records and reports it.
    async fn verify(&self, c: &Chan, want: Value) -> Result<SetOutcome, StationError> {
        if !c.driver.can_read_back() {
            self.store(c, None, Some(None));
            return Ok(SetOutcome {
                commanded: want,
                confirmed: None,
            });
        }
        let got = match self.read_value(c).await {
            Ok(v) => v,
            Err(e) => {
                self.fault(c, &e);
                return Err(e);
            }
        };
        self.store(c, None, Some(got));
        if got == Some(want) {
            self.emit(StationEvent::Confirmed {
                channel: c.def.id.clone(),
                value: want,
            });
            Ok(SetOutcome {
                commanded: want,
                confirmed: got,
            })
        } else {
            self.emit(StationEvent::Mismatch {
                channel: c.def.id.clone(),
                commanded: want,
                confirmed: got,
            });
            Err(StationError::new(
                StationErrorKind::NotConfirmed,
                format!(
                    "channel {}: commanded {want:?}, read back {got:?}",
                    c.def.id.0
                ),
            ))
        }
    }

    async fn write(&self, c: &Chan, line: u32, on: bool) -> Result<(), StationError> {
        c.driver
            .write_line(line, on)
            .await
            .inspect_err(|e| self.fault(c, e))
    }

    // [@ANCHOR: radioctl_station_board_set]
    /// Sets a power, output or selector channel and confirms it by read-back. A power-on inside
    /// the channel's minimum off time is refused. A selector is break-before-make: every other
    /// way is opened and (where it can be read) seen open, the settle time passes, then the new
    /// way closes; if any other way stays closed nothing is closed. Interlocks are the station's
    /// job; this is the raw operation.
    pub(crate) async fn set(
        &self,
        id: &ChannelId,
        value: Value,
    ) -> Result<SetOutcome, StationError> {
        let c = self.chan(id)?;
        let _op = c.op.lock().await;
        match (&c.def.kind, value) {
            (ChannelKind::Power { min_off_ms, .. }, Value::On) => {
                let off_since = lock(&c.state).last_off;
                if let Some(t) = off_since {
                    let min = Duration::from_millis(*min_off_ms);
                    if t.elapsed() < min {
                        return Err(StationError::new(
                            StationErrorKind::Refused,
                            format!(
                                "channel {} was switched off less than {min_off_ms} ms ago",
                                id.0
                            ),
                        ));
                    }
                }
            }
            (ChannelKind::Power { .. } | ChannelKind::Output, Value::On | Value::Off) => {}
            (ChannelKind::Selector { ways, .. }, Value::Port(n)) if (1..=*ways).contains(&n) => {}
            (ChannelKind::Selector { .. }, Value::Off) => {}
            _ => {
                return Err(config(format!(
                    "channel {} cannot be set to {value:?}",
                    id.0
                )))
            }
        }
        if self.status(id)?.confirmed == Some(value) && matches!(value, Value::Port(_)) {
            return Ok(SetOutcome {
                commanded: value,
                confirmed: Some(value),
            });
        }
        self.store(c, Some(Some(value)), None);
        self.emit(StationEvent::Commanded {
            channel: id.clone(),
            value,
        });
        if let ChannelKind::Selector { settle_ms, .. } = c.def.kind {
            let lines = c.def.lines();
            let target = if let Value::Port(n) = value {
                Some(n as usize - 1)
            } else {
                None
            };
            for (i, line) in lines.iter().enumerate() {
                if Some(i) != target {
                    self.write(c, *line, false).await?;
                }
            }
            if let Some(t) = target {
                if c.driver.can_read_back() {
                    for (i, line) in lines.iter().enumerate() {
                        if i != t
                            && c.driver
                                .read_line(*line)
                                .await
                                .inspect_err(|e| self.fault(c, e))?
                        {
                            let got = self.read_value(c).await.ok().flatten();
                            self.store(c, None, Some(got));
                            self.emit(StationEvent::Mismatch {
                                channel: id.clone(),
                                commanded: value,
                                confirmed: got,
                            });
                            return Err(StationError::new(
                                StationErrorKind::NotConfirmed,
                                format!(
                                    "channel {}: way {} did not open, not closing way {}",
                                    id.0,
                                    i + 1,
                                    t + 1
                                ),
                            ));
                        }
                    }
                }
                sleep(Duration::from_millis(settle_ms)).await;
                self.write(c, lines[t], true).await?;
            }
        } else {
            self.write(c, c.def.line, value == Value::On).await?;
        }
        self.verify(c, value).await
    }

    // [@ANCHOR: radioctl_station_board_pulse]
    /// Pulses a momentary channel: the line goes on for the channel's pulse width, then off.
    /// The line is turned off again even if waiting is cancelled by an error.
    pub(crate) async fn pulse(&self, id: &ChannelId) -> Result<(), StationError> {
        let c = self.chan(id)?;
        let ChannelKind::Momentary { pulse_ms } = c.def.kind else {
            return Err(config(format!("channel {} is not momentary", id.0)));
        };
        let _op = c.op.lock().await;
        self.emit(StationEvent::Commanded {
            channel: id.clone(),
            value: Value::On,
        });
        self.write(c, c.def.line, true).await?;
        sleep(Duration::from_millis(pulse_ms)).await;
        let off = self.write(c, c.def.line, false).await;
        if off.is_err() {
            let retry = self.write(c, c.def.line, false).await;
            retry?;
        }
        self.store(c, Some(Some(Value::Off)), None);
        self.emit(StationEvent::Pulsed {
            channel: id.clone(),
        });
        Ok(())
    }

    // [@ANCHOR: radioctl_station_board_force_off]
    /// Drives a power, output or selector channel to off without waiting for any operation in
    /// progress, without a minimum-time check and without a settle time. For the emergency
    /// all-off only; it does not read back (the caller reports what it could not do).
    pub(crate) async fn force_off(&self, id: &ChannelId) -> Result<(), StationError> {
        let c = self.chan(id)?;
        match c.def.kind {
            ChannelKind::Power { .. } | ChannelKind::Output | ChannelKind::Selector { .. } => {}
            _ => return Ok(()),
        }
        let mut first = Ok(());
        for line in c.def.lines() {
            if let Err(e) = c.driver.write_line(line, false).await {
                first = Err(e);
            }
        }
        match first {
            Ok(()) => {
                self.store(c, Some(Some(Value::Off)), Some(None));
                if c.driver.can_read_back() {
                    if let Ok(v) = self.read_value(c).await {
                        self.store(c, None, Some(v));
                    }
                }
                Ok(())
            }
            Err(e) => {
                self.fault(c, &e);
                Err(e)
            }
        }
    }

    // [@ANCHOR: radioctl_station_board_spawn_input]
    /// Starts a task on `runtime` that samples an input channel every `interval`, debounces it by
    /// the channel's debounce time and records and reports each change. A failed read makes the
    /// input unknown (fail closed). Abort the handle to stop it.
    pub fn spawn_input(
        self: &Arc<Self>,
        runtime: &Handle,
        id: &ChannelId,
        interval: Duration,
    ) -> Result<JoinHandle<()>, StationError> {
        let c = self.chan(id)?;
        let ChannelKind::Input { debounce_ms } = c.def.kind else {
            return Err(config(format!("channel {} is not an input", id.0)));
        };
        let board = Arc::clone(self);
        let id = id.clone();
        Ok(runtime.spawn(async move {
            let start = Instant::now();
            let mut deb = Debouncer::new(Duration::from_millis(debounce_ms));
            loop {
                if let Some(c) = board.channels.get(&id) {
                    match c.driver.read_line(c.def.line).await {
                        Ok(raw) => {
                            if let Some(level) = deb.sample(start.elapsed(), raw) {
                                board.store(
                                    c,
                                    None,
                                    Some(Some(if level { Value::On } else { Value::Off })),
                                );
                                board.emit(StationEvent::InputChanged {
                                    channel: id.clone(),
                                    high: level,
                                });
                            }
                        }
                        Err(e) => {
                            deb = Debouncer::new(Duration::from_millis(debounce_ms));
                            board.fault(c, &e);
                        }
                    }
                }
                sleep(interval).await;
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(bank: &VirtualBank) -> Switchboard {
        let def = crate::model::StationDef::from_toml(crate::model::tests::SAMPLE).unwrap();
        let mut b = Switchboard::new();
        for mut c in def.channels {
            if c.id.0 == "btn" {
                c.kind = ChannelKind::Momentary { pulse_ms: 300 };
            }
            b.add_channel(c, Arc::new(bank.clone())).unwrap();
        }
        b
    }

    fn id(s: &str) -> ChannelId {
        ChannelId(s.into())
    }

    // Tests [@ANCHOR: radioctl_station_vbank_new]
    // Tests [@ANCHOR: radioctl_station_vbank_default]
    // Tests [@ANCHOR: radioctl_station_vbank_line]
    // Tests [@ANCHOR: radioctl_station_vbank_log]
    // Tests [@ANCHOR: radioctl_station_vbank_write_line]
    // Tests [@ANCHOR: radioctl_station_vbank_read_line]
    // Tests [@ANCHOR: radioctl_station_vbank_can_read_back]
    // Tests [@ANCHOR: radioctl_station_vbank_set_physical]
    #[tokio::test]
    async fn a_virtual_bank_holds_lines() {
        let bank = VirtualBank::default();
        assert!(!bank.line(3));
        bank.write_line(3, true).await.unwrap();
        assert!(bank.read_line(3).await.unwrap());
        bank.set_physical(3, false);
        assert!(!bank.line(3));
        assert_eq!(bank.log(), vec![(3, true)]);
        assert!(bank.can_read_back());
        assert!(!VirtualBank::without_readback().can_read_back());
    }

    // Tests [@ANCHOR: radioctl_station_vbank_no_readback]
    // Tests [@ANCHOR: radioctl_station_vbank_set_fault]
    #[tokio::test]
    async fn faults_are_injected() {
        let bank = VirtualBank::without_readback();
        assert!(bank.read_line(0).await.is_err());
        let bank = VirtualBank::new();
        bank.set_fault(Fault::FailWrites);
        assert!(bank.write_line(0, true).await.is_err());
        bank.set_fault(Fault::DropAfter(1));
        assert!(bank.write_line(0, true).await.is_ok());
        assert!(bank.write_line(1, true).await.is_err());
        bank.set_fault(Fault::IgnoreWrites);
        bank.write_line(2, true).await.unwrap();
        assert!(!bank.line(2));
        bank.set_fault(Fault::WrongReadback);
        assert!(bank.read_line(2).await.unwrap());
        bank.set_fault(Fault::FailReads);
        assert!(bank.read_line(2).await.is_err());
    }

    // Tests [@ANCHOR: radioctl_station_debouncer_new]
    // Tests [@ANCHOR: radioctl_station_debouncer_sample]
    // Tests [@ANCHOR: radioctl_station_debouncer_stable]
    #[test]
    fn the_debouncer_believes_only_a_held_level() {
        let ms = Duration::from_millis;
        let mut d = Debouncer::new(ms(20));
        assert_eq!(d.sample(ms(0), true), None);
        assert_eq!(d.sample(ms(10), true), None);
        assert_eq!(d.sample(ms(15), false), None); // a glitch restarts the hold
        assert_eq!(d.sample(ms(20), true), None);
        assert_eq!(d.sample(ms(41), true), Some(true));
        assert_eq!(d.sample(ms(50), true), None);
        assert_eq!(d.stable(), Some(true));
        assert_eq!(d.sample(ms(60), false), None);
        assert_eq!(d.sample(ms(80), false), Some(false));
    }

    // Tests [@ANCHOR: radioctl_station_board_default]
    // Tests [@ANCHOR: radioctl_station_board_new]
    // Tests [@ANCHOR: radioctl_station_board_add_channel]
    // Tests [@ANCHOR: radioctl_station_board_def]
    // Tests [@ANCHOR: radioctl_station_board_ids]
    // Tests [@ANCHOR: radioctl_station_board_subscribe]
    // Tests [@ANCHOR: radioctl_station_board_emit]
    // Tests [@ANCHOR: radioctl_station_board_status]
    // Tests [@ANCHOR: radioctl_station_board_set]
    // Tests [@ANCHOR: radioctl_station_board_store]
    // Tests [@ANCHOR: radioctl_station_board_read_value]
    // Tests [@ANCHOR: radioctl_station_board_verify]
    #[tokio::test]
    async fn power_is_commanded_then_confirmed_and_mismatches_alarm() {
        let bank = VirtualBank::new();
        let b = board(&bank);
        assert_eq!(b.ids().len(), 4);
        assert!(b.def(&id("ant")).is_ok());
        assert!(b.def(&id("zzz")).is_err());
        assert!(b.status(&id("radio_pwr")).unwrap().commanded.is_none());
        let mut rx = b.subscribe();
        let out = b.set(&id("radio_pwr"), Value::On).await.unwrap();
        assert_eq!(out.confirmed, Some(Value::On));
        assert!(bank.line(0));
        assert_eq!(
            rx.recv().await.unwrap(),
            StationEvent::Commanded {
                channel: id("radio_pwr"),
                value: Value::On
            }
        );
        assert_eq!(
            rx.recv().await.unwrap(),
            StationEvent::Confirmed {
                channel: id("radio_pwr"),
                value: Value::On
            }
        );
        bank.set_fault(Fault::IgnoreWrites);
        let err = b.set(&id("radio_pwr"), Value::Off).await.unwrap_err();
        assert_eq!(err.kind, StationErrorKind::NotConfirmed);
        let st = b.status(&id("radio_pwr")).unwrap();
        assert_eq!(
            (st.commanded, st.confirmed),
            (Some(Value::Off), Some(Value::On))
        );
        assert!(matches!(
            rx.recv().await.unwrap(),
            StationEvent::Commanded { .. }
        ));
        assert!(matches!(
            rx.recv().await.unwrap(),
            StationEvent::Mismatch { .. }
        ));
        bank.set_fault(Fault::FailWrites);
        assert_eq!(
            b.set(&id("radio_pwr"), Value::Off).await.unwrap_err().kind,
            StationErrorKind::Driver
        );
        assert_eq!(b.status(&id("radio_pwr")).unwrap().confirmed, None);
        assert!(b.set(&id("btn"), Value::On).await.is_err());
        assert!(b.set(&id("ant"), Value::Port(9)).await.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn power_respects_the_minimum_off_time() {
        let bank = VirtualBank::new();
        let b = board(&bank);
        b.set(&id("amp_pwr"), Value::On).await.unwrap();
        b.set(&id("amp_pwr"), Value::Off).await.unwrap();
        let err = b.set(&id("amp_pwr"), Value::On).await.unwrap_err();
        assert_eq!(err.kind, StationErrorKind::Refused);
        tokio::time::advance(Duration::from_millis(600)).await;
        b.set(&id("amp_pwr"), Value::On).await.unwrap();
    }

    // Tests [@ANCHOR: radioctl_station_board_store]
    #[tokio::test(start_paused = true)]
    async fn a_channel_first_seen_off_may_be_switched_on_at_once() {
        // The start-up read-back finds the amplifier off. The software has only just started: the
        // channel's minimum off time protects a device that was running, so it must not apply.
        let bank = VirtualBank::new();
        let b = board(&bank);
        assert_eq!(b.read_back(&id("amp_pwr")).await.unwrap(), Some(Value::Off));
        b.set(&id("amp_pwr"), Value::On).await.unwrap();

        b.set(&id("amp_pwr"), Value::Off).await.unwrap();
        tokio::time::advance(Duration::from_millis(600)).await;
        b.set(&id("amp_pwr"), Value::On).await.unwrap();

        // A real turn-off seen by read-back (someone flipped the switch by hand) is an edge.
        bank.set_physical(1, false);
        assert_eq!(b.read_back(&id("amp_pwr")).await.unwrap(), Some(Value::Off));
        let err = b.set(&id("amp_pwr"), Value::On).await.unwrap_err();
        assert_eq!(err.kind, StationErrorKind::Refused);
    }

    #[tokio::test(start_paused = true)]
    async fn selectors_break_before_make() {
        let bank = VirtualBank::new();
        let b = board(&bank);
        b.set(&id("ant"), Value::Port(1)).await.unwrap();
        bank.set_fault(Fault::None);
        let before = bank.log().len();
        let out = b.set(&id("ant"), Value::Port(3)).await.unwrap();
        assert_eq!(out.confirmed, Some(Value::Port(3)));
        let log = bank.log()[before..].to_vec();
        // way 1 (line 2) opens before way 3 (line 4) closes, and way 3 closes last
        let open1 = log.iter().position(|w| *w == (2, false)).unwrap();
        let make3 = log.iter().position(|w| *w == (4, true)).unwrap();
        assert!(open1 < make3);
        assert_eq!(log.last(), Some(&(4, true)));
        // a way that will not open stops the make
        bank.set_physical(4, true);
        bank.set_fault(Fault::IgnoreWrites);
        let err = b.set(&id("ant"), Value::Port(2)).await.unwrap_err();
        assert_eq!(err.kind, StationErrorKind::NotConfirmed);
        assert!(!bank.log().contains(&(3, true)));
        // an unchanged selection writes nothing
        bank.set_fault(Fault::None);
        bank.set_physical(4, false);
        b.set(&id("ant"), Value::Port(2)).await.unwrap();
        let n = bank.log().len();
        b.set(&id("ant"), Value::Port(2)).await.unwrap();
        assert_eq!(bank.log().len(), n);
        b.set(&id("ant"), Value::Off).await.unwrap();
        assert_eq!(b.status(&id("ant")).unwrap().confirmed, Some(Value::Off));
    }

    // Tests [@ANCHOR: radioctl_station_board_read_back]
    #[tokio::test]
    async fn read_back_reports_without_writing() {
        let bank = VirtualBank::new();
        let b = board(&bank);
        bank.set_physical(1, true);
        bank.set_physical(3, true);
        assert_eq!(b.read_back(&id("amp_pwr")).await.unwrap(), Some(Value::On));
        assert_eq!(b.read_back(&id("ant")).await.unwrap(), Some(Value::Port(2)));
        bank.set_physical(4, true);
        assert_eq!(b.read_back(&id("ant")).await.unwrap(), None);
        assert!(bank.log().is_empty());
        let st = b.status(&id("amp_pwr")).unwrap();
        assert_eq!((st.commanded, st.confirmed), (None, Some(Value::On)));
        bank.set_fault(Fault::FailReads);
        assert!(b.read_back(&id("amp_pwr")).await.is_err());
    }

    // Tests [@ANCHOR: radioctl_station_board_pulse]
    #[tokio::test(start_paused = true)]
    async fn a_momentary_pulses_for_its_width() {
        let bank = VirtualBank::new();
        let b = board(&bank);
        let start = Instant::now();
        b.pulse(&id("btn")).await.unwrap();
        assert!(start.elapsed() >= Duration::from_millis(300));
        assert_eq!(bank.log(), vec![(5, true), (5, false)]);
        assert!(!bank.line(5));
        assert!(b.pulse(&id("amp_pwr")).await.is_err());
    }

    // Tests [@ANCHOR: radioctl_station_board_force_off]
    #[tokio::test]
    async fn force_off_ignores_minimum_times_and_busy_channels() {
        let bank = VirtualBank::new();
        let b = board(&bank);
        b.set(&id("amp_pwr"), Value::On).await.unwrap();
        b.set(&id("ant"), Value::Port(2)).await.unwrap();
        b.force_off(&id("amp_pwr")).await.unwrap();
        b.force_off(&id("ant")).await.unwrap();
        b.force_off(&id("btn")).await.unwrap();
        assert!(!bank.line(1) && !bank.line(3));
        assert_eq!(
            b.status(&id("amp_pwr")).unwrap().confirmed,
            Some(Value::Off)
        );
        bank.set_fault(Fault::FailWrites);
        assert!(b.force_off(&id("amp_pwr")).await.is_err());
    }

    // Tests [@ANCHOR: radioctl_station_board_spawn_input]
    #[tokio::test(start_paused = true)]
    async fn an_input_is_debounced_and_fails_closed() {
        let bank = VirtualBank::new();
        let mut b = Switchboard::new();
        let def = ChannelDef {
            id: id("swr"),
            kind: ChannelKind::Input { debounce_ms: 30 },
            driver: "bank".into(),
            line: 7,
            safe: None,
            description: String::new(),
        };
        b.add_channel(def, Arc::new(bank.clone())).unwrap();
        let b = Arc::new(b);
        let mut rx = b.subscribe();
        let task = b
            .spawn_input(&Handle::current(), &id("swr"), Duration::from_millis(10))
            .unwrap();
        bank.set_physical(7, true);
        let ev = rx.recv().await.unwrap();
        assert_eq!(
            ev,
            StationEvent::InputChanged {
                channel: id("swr"),
                high: true
            }
        );
        assert_eq!(b.status(&id("swr")).unwrap().confirmed, Some(Value::On));
        bank.set_fault(Fault::FailReads);
        assert!(matches!(
            rx.recv().await.unwrap(),
            StationEvent::DriverFault { .. }
        ));
        assert_eq!(b.status(&id("swr")).unwrap().confirmed, None);
        task.abort();
        assert!(b
            .spawn_input(
                &Handle::current(),
                &id("amp_pwr"),
                Duration::from_millis(10)
            )
            .is_err());
    }
}
