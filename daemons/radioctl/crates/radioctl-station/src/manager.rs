// SPDX-License-Identifier: AGPL-3.0-or-later
//! The radio manager: the lifecycle of every radio of the station, its durable record, the power
//! button and the start-up reconcile.
//!
//! Rules it keeps: it never switches a power channel except on an instruction (the power button);
//! it reads the real channel state at start without writing; after a radio reconnects it records
//! and reports the state the radio has and sets nothing on it. Evidence comes through the
//! [`RadioLink`] trait (the rig actor's link state, hotplug or ping), virtual in tests. A task
//! is only started by [`RadioManager::spawn_monitor`], on a `Handle` the caller passes; there is no
//! runtime of its own and no blocking.

use crate::channel::BoxFuture;
use crate::channel::VirtualBank;
use crate::error::{StationError, StationErrorKind};
use crate::lifecycle::{Evidence, Lifecycle, LifecycleTimes, PowerEvidence, RadioState};
use crate::model::{ChannelKind, DeviceId, DeviceKind, PowerSource, Value};
use crate::record::{
    Cached, ConnectMethod, ConnectProcedure, KnownState, RadioRecord, RecordStore,
};
use crate::station::Station;
use radioctl_core::backoff::Backoff;
use radioctl_core::rig::RigHandle;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Instant};

/// How a radio is reached, as evidence and as an action. Virtual in tests.
pub trait RadioLink: Send + Sync {
    /// The rig actor's link state (`RigHandle::link_up`).
    fn link_up(&self, radio: &DeviceId) -> BoxFuture<'_, bool>;
    /// Hotplug (USB) or ping (network) evidence: `None` when there is none.
    fn present(&self, radio: &DeviceId) -> BoxFuture<'_, Option<bool>>;
    /// One attempt to connect (open the transport, spawn the actor). On success returns the state
    /// the radio reports. It must only read.
    fn connect(&self, radio: &DeviceId) -> BoxFuture<'_, Result<Option<KnownState>, StationError>>;
}

/// The link of real rigs: the actor's `link_up` for evidence; connecting is the caller's business
/// (it respawns the actor), so `connect` here only reports whether the link is up.
#[derive(Default)]
pub struct RigLinks {
    rigs: Mutex<BTreeMap<DeviceId, RigHandle>>,
}

impl RigLinks {
    // [@ANCHOR: radioctl_station_riglinks_new]
    /// No rigs yet.
    pub fn new() -> RigLinks {
        RigLinks::default()
    }

    // [@ANCHOR: radioctl_station_riglinks_set]
    /// Attaches (or replaces, after a reconnect) the handle of a radio.
    pub fn set(&self, radio: DeviceId, handle: RigHandle) {
        lock(&self.rigs).insert(radio, handle);
    }
}

impl RadioLink for RigLinks {
    // [@ANCHOR: radioctl_station_riglinks_link_up]
    /// The handle's `link_up`; a radio with no handle is down.
    fn link_up(&self, radio: &DeviceId) -> BoxFuture<'_, bool> {
        let up = lock(&self.rigs).get(radio).is_some_and(|h| h.link_up());
        Box::pin(async move { up })
    }

    // [@ANCHOR: radioctl_station_riglinks_present]
    /// No hotplug or ping evidence from a bare handle.
    fn present(&self, _radio: &DeviceId) -> BoxFuture<'_, Option<bool>> {
        Box::pin(async { None })
    }

    // [@ANCHOR: radioctl_station_riglinks_connect]
    /// Succeeds only if the link is up; the caller respawns the actor.
    fn connect(&self, radio: &DeviceId) -> BoxFuture<'_, Result<Option<KnownState>, StationError>> {
        let up = lock(&self.rigs).get(radio).is_some_and(|h| h.link_up());
        Box::pin(async move {
            if up {
                Ok(None)
            } else {
                Err(StationError::new(StationErrorKind::Driver, "link is down"))
            }
        })
    }
}

/// A virtual radio for tests: it boots a set time after its power line (a [`VirtualBank`] line)
/// goes on, can be made never to boot, to drop its link, or to vanish.
#[derive(Clone)]
pub struct VirtualRadioLink {
    bank: VirtualBank,
    radios: Arc<Mutex<BTreeMap<DeviceId, VRadio>>>,
}

struct VRadio {
    line: u32,
    boot: Option<Duration>,
    on_since: Option<Instant>,
    connected: bool,
    dropped: bool,
    gone: bool,
    connects: u32,
    report: Option<KnownState>,
}

impl VirtualRadioLink {
    // [@ANCHOR: radioctl_station_vlink_new]
    /// A link over the bank whose lines power the radios.
    pub fn new(bank: VirtualBank) -> VirtualRadioLink {
        VirtualRadioLink {
            bank,
            radios: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    // [@ANCHOR: radioctl_station_vlink_add]
    /// A radio on bank line `line` that boots after `boot` (`None`: never).
    pub fn add(&self, radio: DeviceId, line: u32, boot: Option<Duration>) {
        lock(&self.radios).insert(
            radio,
            VRadio {
                line,
                boot,
                on_since: None,
                connected: false,
                dropped: false,
                gone: false,
                connects: 0,
                report: None,
            },
        );
    }

    // [@ANCHOR: radioctl_station_vlink_set_boot]
    /// Changes how long the radio takes to boot (`None`: never).
    pub fn set_boot(&self, radio: &DeviceId, boot: Option<Duration>) {
        if let Some(r) = lock(&self.radios).get_mut(radio) {
            r.boot = boot;
        }
    }

    // [@ANCHOR: radioctl_station_vlink_set_report]
    /// The state the radio reports when it is connected.
    pub fn set_report(&self, radio: &DeviceId, state: KnownState) {
        if let Some(r) = lock(&self.radios).get_mut(radio) {
            r.report = Some(state);
        }
    }

    // [@ANCHOR: radioctl_station_vlink_drop_link]
    /// Fault: the link drops and cannot be re-established until the radio is power-cycled or
    /// [`VirtualRadioLink::heal`] is called. The device stays enumerated.
    pub fn drop_link(&self, radio: &DeviceId) {
        if let Some(r) = lock(&self.radios).get_mut(radio) {
            r.connected = false;
            r.dropped = true;
        }
    }

    // [@ANCHOR: radioctl_station_vlink_heal]
    /// Clears a dropped link.
    pub fn heal(&self, radio: &DeviceId) {
        if let Some(r) = lock(&self.radios).get_mut(radio) {
            r.dropped = false;
        }
    }

    // [@ANCHOR: radioctl_station_vlink_connects]
    /// How many connect attempts were made.
    pub fn connects(&self, radio: &DeviceId) -> u32 {
        lock(&self.radios).get(radio).map_or(0, |r| r.connects)
    }

    // [@ANCHOR: radioctl_station_vlink_refresh]
    /// Follows the power line: off resets everything, the first moment on starts the boot clock.
    fn refresh(&self, r: &mut VRadio) -> bool {
        if !self.bank.line(r.line) {
            r.on_since = None;
            r.connected = false;
            r.dropped = false;
            return false;
        }
        let now = Instant::now();
        let since = *r.on_since.get_or_insert(now);
        r.boot.is_some_and(|b| now.duration_since(since) >= b) && !r.gone
    }
}

impl RadioLink for VirtualRadioLink {
    // [@ANCHOR: radioctl_station_vlink_link_up]
    /// Up when booted, connected and not dropped.
    fn link_up(&self, radio: &DeviceId) -> BoxFuture<'_, bool> {
        let up = lock(&self.radios)
            .get_mut(radio)
            .is_some_and(|r| self.refresh(r) && r.connected && !r.dropped);
        Box::pin(async move { up })
    }

    // [@ANCHOR: radioctl_station_vlink_present]
    /// Enumerated once booted (a dropped link still is).
    fn present(&self, radio: &DeviceId) -> BoxFuture<'_, Option<bool>> {
        let p = lock(&self.radios).get_mut(radio).map(|r| self.refresh(r));
        Box::pin(async move { p })
    }

    // [@ANCHOR: radioctl_station_vlink_connect]
    /// Connects if booted and not dropped.
    fn connect(&self, radio: &DeviceId) -> BoxFuture<'_, Result<Option<KnownState>, StationError>> {
        let out = match lock(&self.radios).get_mut(radio) {
            None => Err(unknown(radio)),
            Some(r) => {
                r.connects += 1;
                if self.refresh(r) && !r.dropped {
                    r.connected = true;
                    Ok(r.report.clone())
                } else {
                    Err(StationError::new(StationErrorKind::Driver, "no answer"))
                }
            }
        };
        Box::pin(async move { out })
    }
}

// [@ANCHOR: radioctl_station_manager_lock]
/// Locks a mutex, ignoring poisoning (the data are plain values).
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A radio changed state: the station-level event.
#[derive(Debug, Clone, PartialEq)]
pub struct RadioEvent {
    /// The radio.
    pub radio: DeviceId,
    /// The state before.
    pub from: RadioState,
    /// The state after.
    pub to: RadioState,
    /// Why.
    pub reason: String,
    /// Unix seconds.
    pub at_unix: u64,
    /// The state the radio reported on reconnecting (reported, never restored).
    pub reported: Option<KnownState>,
}

/// The power button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// Switch on and reconnect.
    On,
    /// Orderly off (dependents first).
    Off,
    /// Off, wait the channel's minimum off time, on, reconnect.
    Reboot,
}

/// What a button press did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonReport {
    /// The radio's state when the operation ended.
    pub state: RadioState,
    /// Devices switched off, in order (dependents first).
    pub powered_off: Vec<DeviceId>,
}

/// One radio's start-up reconcile result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileItem {
    /// The radio.
    pub radio: DeviceId,
    /// The real power read back (`None`: could not be read or no power source).
    pub real_power: Option<Value>,
    /// What the record said.
    pub recorded: RadioState,
    /// The state it was resolved to.
    pub resolved: RadioState,
    /// Whether the record and the real state disagreed.
    pub disagrees: bool,
}

struct Entry {
    record: RadioRecord,
    life: Lifecycle,
    connect_failures: u32,
    next_connect: Duration,
}

const EVENT_BUFFER: usize = 256;
const BUTTON_POLL: Duration = Duration::from_millis(250);

/// The lifecycle, record and power button of the station's radios.
pub struct RadioManager {
    station: Arc<Station>,
    link: Arc<dyn RadioLink>,
    store: RecordStore,
    clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    start: Instant,
    times: LifecycleTimes,
    backoff: Backoff,
    inner: Mutex<BTreeMap<DeviceId, Entry>>,
    events: broadcast::Sender<RadioEvent>,
    problems: Mutex<Vec<String>>,
}

impl RadioManager {
    // [@ANCHOR: radioctl_station_manager_new]
    /// Loads the record file (a bad one is set aside, see [`RecordStore::load`]) and builds an
    /// entry for every radio device of the station definition. A radio with no record gets a bare
    /// one. Every radio starts `Unreachable` until [`RadioManager::reconcile`] reads the switches.
    /// Nothing is switched. `clock` gives unix seconds.
    pub fn new(
        station: Arc<Station>,
        link: Arc<dyn RadioLink>,
        store: RecordStore,
        clock: Arc<dyn Fn() -> u64 + Send + Sync>,
        times: LifecycleTimes,
    ) -> RadioManager {
        let loaded = store.load();
        let start = Instant::now();
        let mut map = BTreeMap::new();
        for d in station
            .def()
            .devices
            .iter()
            .filter(|d| d.kind == DeviceKind::Radio)
        {
            let mut record = loaded.records.get(&d.id).cloned().unwrap_or_else(|| {
                RadioRecord::new(
                    d.id.clone(),
                    "unknown",
                    "unknown",
                    ConnectProcedure {
                        method: ConnectMethod::BootTime,
                        notes: String::new(),
                    },
                )
            });
            record.power = match &d.power {
                PowerSource::AlwaysOn => None,
                other => Some(other.clone()),
            };
            let mut t = times;
            if record.boot_time_ms > 0 {
                t.boot_time = Duration::from_millis(record.boot_time_ms);
            }
            let life = Lifecycle::new(RadioState::Unreachable, Duration::ZERO, t);
            map.insert(
                d.id.clone(),
                Entry {
                    record,
                    life,
                    connect_failures: 0,
                    next_connect: Duration::ZERO,
                },
            );
        }
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        let mgr = RadioManager {
            station,
            link,
            store,
            clock,
            start,
            times,
            backoff: Backoff::default(),
            inner: Mutex::new(map),
            events,
            problems: Mutex::new(loaded.problems),
        };
        for id in mgr.ids() {
            mgr.station.set_radio_state(&id, RadioState::Unreachable);
        }
        mgr
    }

    // [@ANCHOR: radioctl_station_manager_problems]
    /// What the record loader and later saves had to report.
    pub fn problems(&self) -> Vec<String> {
        lock(&self.problems).clone()
    }

    // [@ANCHOR: radioctl_station_manager_ids]
    /// The radios, sorted.
    pub fn ids(&self) -> Vec<DeviceId> {
        lock(&self.inner).keys().cloned().collect()
    }

    // [@ANCHOR: radioctl_station_manager_subscribe]
    /// A receiver of every [`RadioEvent`] from now on (the station-level event stream).
    pub fn subscribe(&self) -> broadcast::Receiver<RadioEvent> {
        self.events.subscribe()
    }

    // [@ANCHOR: radioctl_station_manager_spawn_callback]
    /// Calls `callback` for every event, from a task on `runtime`, in the style of radioctl's
    /// status-change callback. The task ends when the manager is dropped.
    pub fn spawn_callback<F: Fn(RadioEvent) + Send + 'static>(
        &self,
        runtime: &Handle,
        callback: F,
    ) -> JoinHandle<()> {
        let mut rx = self.subscribe();
        runtime.spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(e) => callback(e),
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }

    // [@ANCHOR: radioctl_station_manager_state]
    /// The lifecycle state of a radio.
    pub fn state(&self, radio: &DeviceId) -> Option<RadioState> {
        lock(&self.inner).get(radio).map(|e| e.life.state())
    }

    // [@ANCHOR: radioctl_station_manager_record]
    /// A copy of the radio's record.
    pub fn record(&self, radio: &DeviceId) -> Option<RadioRecord> {
        lock(&self.inner).get(radio).map(|e| e.record.clone())
    }

    // [@ANCHOR: radioctl_station_manager_cached]
    /// The last-known state with its age, served whether the radio is on or off.
    pub fn cached(&self, radio: &DeviceId) -> Option<Cached> {
        lock(&self.inner)
            .get(radio)
            .and_then(|e| e.record.cached((self.clock)()))
    }

    // [@ANCHOR: radioctl_station_manager_note_state]
    /// Records a state read from the live radio (and its capabilities, when given) and saves.
    pub fn note_state(
        &self,
        radio: &DeviceId,
        mut state: KnownState,
        capabilities: Option<Vec<String>>,
    ) -> Result<(), StationError> {
        state.seen_unix = (self.clock)();
        {
            let mut inner = lock(&self.inner);
            let e = inner.get_mut(radio).ok_or_else(|| unknown(radio))?;
            e.record.last_state = Some(state);
            if let Some(c) = capabilities {
                e.record.capabilities = c;
            }
        }
        self.persist()
    }

    // [@ANCHOR: radioctl_station_manager_persist]
    /// Saves every record. A failure is also kept in [`RadioManager::problems`].
    pub fn persist(&self) -> Result<(), StationError> {
        let all: BTreeMap<DeviceId, RadioRecord> = lock(&self.inner)
            .iter()
            .map(|(k, e)| (k.clone(), e.record.clone()))
            .collect();
        self.store.save(&all).inspect_err(|e| {
            lock(&self.problems).push(e.detail.clone());
        })
    }

    // [@ANCHOR: radioctl_station_manager_now]
    /// Time since the manager was built.
    fn now(&self) -> Duration {
        self.start.elapsed()
    }

    // [@ANCHOR: radioctl_station_manager_power_evidence]
    /// The switch channel's confirmed state as evidence.
    fn power_evidence(&self, radio: &DeviceId) -> PowerEvidence {
        match self.station.def().device(radio).map(|d| &d.power) {
            Some(PowerSource::Channel { .. }) => match self.station.device_power(radio) {
                Ok(Some(Value::On)) => PowerEvidence::On,
                Ok(Some(Value::Off)) => PowerEvidence::Off,
                _ => PowerEvidence::Unknown,
            },
            _ => PowerEvidence::Unknown,
        }
    }

    // [@ANCHOR: radioctl_station_manager_gather]
    /// One snapshot of evidence.
    async fn gather(&self, radio: &DeviceId) -> Evidence {
        Evidence {
            power: self.power_evidence(radio),
            link_up: self.link.link_up(radio).await,
            present: self.link.present(radio).await,
        }
    }

    // [@ANCHOR: radioctl_station_manager_apply]
    /// Applies a transition: updates the hint, the record's health and lifecycle, saves, and
    /// publishes the event.
    fn apply(
        &self,
        radio: &DeviceId,
        t: crate::lifecycle::Transition,
        reported: Option<KnownState>,
    ) -> RadioEvent {
        let at = (self.clock)();
        {
            let mut inner = lock(&self.inner);
            if let Some(e) = inner.get_mut(radio) {
                e.record.lifecycle = t.to;
                e.record.lifecycle_unix = at;
                match t.to {
                    RadioState::Ready => {
                        e.record.health.last_seen_unix = Some(at);
                        e.record.health.consecutive_failures = 0;
                        e.connect_failures = 0;
                    }
                    RadioState::Degraded | RadioState::Unreachable
                        if t.from != RadioState::Off && t.from != RadioState::Unreachable =>
                    {
                        e.record.health.failures = e.record.health.failures.saturating_add(1);
                        e.record.health.consecutive_failures =
                            e.record.health.consecutive_failures.saturating_add(1);
                    }
                    _ => {}
                }
            }
        }
        self.station.set_radio_state(radio, t.to);
        let _ = self.persist();
        let ev = RadioEvent {
            radio: radio.clone(),
            from: t.from,
            to: t.to,
            reason: t.reason.to_string(),
            at_unix: at,
            reported,
        };
        let _ = self.events.send(ev.clone());
        ev
    }

    // [@ANCHOR: radioctl_station_manager_poll]
    /// Gathers evidence and steps the radio's state machine once.
    pub async fn poll(&self, radio: &DeviceId) -> Result<Option<RadioEvent>, StationError> {
        if !lock(&self.inner).contains_key(radio) {
            return Err(unknown(radio));
        }
        let ev = self.gather(radio).await;
        let now = self.now();
        let t = lock(&self.inner)
            .get_mut(radio)
            .and_then(|e| e.life.step(now, &ev));
        Ok(t.map(|t| self.apply(radio, t, None)))
    }

    // [@ANCHOR: radioctl_station_manager_connect_step]
    /// One reconnect attempt, if one is due: the radio is powered (or its power is unknown), not
    /// linked, and either booting, degraded, or Unreachable with the device present. A failure
    /// backs off (250 ms doubling to 8 s). On success the state the radio reports is recorded and
    /// returned; nothing is sent to the radio.
    pub async fn connect_step(&self, radio: &DeviceId) -> Option<KnownState> {
        let due = {
            let inner = lock(&self.inner);
            let e = inner.get(radio)?;
            matches!(
                e.life.state(),
                RadioState::Booting | RadioState::Degraded | RadioState::Unreachable
            ) && self.now() >= e.next_connect
        };
        if !due || self.power_evidence(radio) == PowerEvidence::Off {
            return None;
        }
        if self.link.link_up(radio).await {
            return None;
        }
        if lock(&self.inner)
            .get(radio)
            .is_some_and(|e| e.life.state() == RadioState::Unreachable)
            && self.link.present(radio).await != Some(true)
        {
            return None;
        }
        let result = self.link.connect(radio).await;
        let now = self.now();
        let mut inner = lock(&self.inner);
        let e = inner.get_mut(radio)?;
        match result {
            Ok(known) => {
                e.connect_failures = 0;
                e.next_connect = now;
                known.map(|mut k| {
                    k.seen_unix = (self.clock)();
                    e.record.last_state = Some(k.clone());
                    k
                })
            }
            Err(_) => {
                e.connect_failures = e.connect_failures.saturating_add(1);
                e.next_connect = now + self.backoff.delay(e.connect_failures);
                None
            }
        }
    }

    // [@ANCHOR: radioctl_station_manager_tick]
    /// A reconnect attempt if due, then one step of the state machine. A state the radio reports
    /// on connecting rides on the event that follows (Ready).
    pub async fn tick(&self, radio: &DeviceId) -> Result<Option<RadioEvent>, StationError> {
        let reported = self.connect_step(radio).await;
        let ev = self.gather(radio).await;
        let now = self.now();
        let t = {
            let mut inner = lock(&self.inner);
            inner
                .get_mut(radio)
                .ok_or_else(|| unknown(radio))?
                .life
                .step(now, &ev)
        };
        Ok(t.map(|t| {
            let rep = if t.to == RadioState::Ready {
                reported
            } else {
                None
            };
            self.apply(radio, t, rep)
        }))
    }

    // [@ANCHOR: radioctl_station_manager_spawn_monitor]
    /// Ticks every radio every `interval` on `runtime`. Abort the handle to stop it.
    pub fn spawn_monitor(self: &Arc<Self>, runtime: &Handle, interval: Duration) -> JoinHandle<()> {
        let me = Arc::clone(self);
        runtime.spawn(async move {
            loop {
                for id in me.ids() {
                    let _ = me.tick(&id).await;
                }
                sleep(interval).await;
            }
        })
    }

    // [@ANCHOR: radioctl_station_manager_power_source]
    /// The device's power source, refusing a radio that has no power button.
    fn power_source(&self, radio: &DeviceId) -> Result<PowerSource, StationError> {
        let d = self
            .station
            .def()
            .device(radio)
            .ok_or_else(|| unknown(radio))?;
        if d.kind != DeviceKind::Radio || !lock(&self.inner).contains_key(radio) {
            return Err(unknown(radio));
        }
        match &d.power {
            PowerSource::AlwaysOn => Err(StationError::new(
                StationErrorKind::Refused,
                format!("{} has no power source, so no power button", radio.0),
            )),
            other => Ok(other.clone()),
        }
    }

    // [@ANCHOR: radioctl_station_manager_press_on]
    /// The on half of the button: power on through the station (interlocks and dependencies), then
    /// reconnect with backoff until Ready, Unreachable or Off, bounded by the boot time.
    async fn press_on(&self, radio: &DeviceId, source: &PowerSource) -> Result<(), StationError> {
        self.step_state(radio, |l, now| l.power_on_commanded(now));
        let started = self.station.power_on(radio).await;
        if let Err(e) = started {
            let ev = self.gather(radio).await;
            let now = self.now();
            let t = lock(&self.inner)
                .get_mut(radio)
                .and_then(|en| en.life.step(now, &ev));
            if let Some(t) = t {
                self.apply(radio, t, None);
            }
            return Err(e);
        }
        if matches!(source, PowerSource::Momentary { .. }) {
            self.step_state(radio, |l, now| l.assume_powered(now));
        }
        let limit = {
            let t = lock(&self.inner).get(radio).map(|e| e.record.boot_time_ms);
            Duration::from_millis(t.unwrap_or(0)).max(self.times.boot_time)
                + self.times.boot_grace
                + self.times.power_on_timeout
        };
        let deadline = Instant::now() + limit;
        loop {
            let _ = self.tick(radio).await?;
            match self.state(radio) {
                Some(RadioState::Ready | RadioState::Unreachable | RadioState::Off) | None => {
                    return Ok(())
                }
                _ => {}
            }
            if Instant::now() >= deadline {
                return Ok(());
            }
            sleep(BUTTON_POLL).await;
        }
    }

    // [@ANCHOR: radioctl_station_manager_step_state]
    /// Runs a command-driven lifecycle transition and publishes it.
    fn step_state(
        &self,
        radio: &DeviceId,
        f: impl FnOnce(&mut Lifecycle, Duration) -> Option<crate::lifecycle::Transition>,
    ) {
        let now = self.now();
        let t = lock(&self.inner)
            .get_mut(radio)
            .and_then(|e| f(&mut e.life, now));
        if let Some(t) = t {
            self.apply(radio, t, None);
        }
    }

    // [@ANCHOR: radioctl_station_manager_press]
    /// The power button of a radio with a power source: on, off or reboot, behind the interlocks
    /// and the dependency graph (the station checks them). On a radio that is already up does
    /// nothing; on a powered but Unreachable radio is refused (use reboot). A radio with no power
    /// source has no button. The outcome is the state the radio ended in; settings are never
    /// restored.
    pub async fn press(
        &self,
        radio: &DeviceId,
        button: Button,
    ) -> Result<ButtonReport, StationError> {
        let source = self.power_source(radio)?;
        let mut powered_off = Vec::new();
        if matches!(button, Button::Off | Button::Reboot) {
            powered_off = self.station.power_off(radio).await?;
            for d in &powered_off {
                if d == radio {
                    self.step_state(radio, |l, now| l.power_off_commanded(now));
                }
            }
            let _ = self.poll(radio).await?;
        }
        if button == Button::Reboot {
            let min_off = match &source {
                PowerSource::Channel { channel } => match self.station.def().channel(channel) {
                    Some(c) => match c.kind {
                        ChannelKind::Power { min_off_ms, .. } => min_off_ms,
                        _ => 0,
                    },
                    None => 0,
                },
                _ => 0,
            };
            sleep(Duration::from_millis(min_off)).await;
        }
        if matches!(button, Button::On | Button::Reboot) {
            match (button, self.state(radio)) {
                (
                    Button::On,
                    Some(
                        RadioState::Booting
                        | RadioState::Ready
                        | RadioState::Degraded
                        | RadioState::PoweringOn,
                    ),
                ) => {}
                (Button::On, Some(RadioState::Unreachable))
                    if self.power_evidence(radio) == PowerEvidence::On =>
                {
                    return Err(StationError::new(
                        StationErrorKind::Refused,
                        format!("{} is powered but unreachable: use reboot", radio.0),
                    ));
                }
                _ => self.press_on(radio, &source).await?,
            }
        }
        Ok(ButtonReport {
            state: self.state(radio).ok_or_else(|| unknown(radio))?,
            powered_off,
        })
    }

    // [@ANCHOR: radioctl_station_manager_reconcile]
    /// The start-up reconcile. Reads back the real state of each radio's power channel (a read,
    /// never a write), compares it with the record, resolves the lifecycle state from the real
    /// state and the link, updates the record, and reports every disagreement. A power channel is
    /// never changed. A switch that cannot be read leaves the radio Unreachable with the record's
    /// cached state kept.
    pub async fn reconcile(&self) -> Vec<ReconcileItem> {
        let mut items = Vec::new();
        for radio in self.ids() {
            let recorded = lock(&self.inner)
                .get(&radio)
                .map_or(RadioState::Unreachable, |e| e.record.lifecycle);
            let real_power = match self.station.def().device(&radio).map(|d| d.power.clone()) {
                Some(PowerSource::Channel { channel }) => self
                    .station
                    .board()
                    .read_back(&channel)
                    .await
                    .ok()
                    .flatten(),
                _ => None,
            };
            let ev = Evidence {
                power: match real_power {
                    Some(Value::On) => PowerEvidence::On,
                    Some(Value::Off) => PowerEvidence::Off,
                    _ => PowerEvidence::Unknown,
                },
                link_up: self.link.link_up(&radio).await,
                present: self.link.present(&radio).await,
            };
            let resolved = match (ev.power, ev.link_up) {
                (PowerEvidence::Off, _) => RadioState::Off,
                (PowerEvidence::On, true) | (PowerEvidence::Unknown, true) => RadioState::Ready,
                (PowerEvidence::On, false) => RadioState::Booting,
                (PowerEvidence::Unknown, false) => RadioState::Unreachable,
            };
            // disagreement: the record believed the radio was off but the switch is on, or the
            // other way round, or the record believed it was up and nothing is
            let up = |s: RadioState| {
                matches!(
                    s,
                    RadioState::Ready
                        | RadioState::Degraded
                        | RadioState::Booting
                        | RadioState::PoweringOn
                )
            };
            let disagrees = match ev.power {
                PowerEvidence::Off => up(recorded),
                PowerEvidence::On => recorded == RadioState::Off,
                PowerEvidence::Unknown => false,
            } || (ev.power != PowerEvidence::Unknown
                && recorded == RadioState::Ready
                && !ev.link_up);
            let now = self.now();
            let t = {
                let mut inner = lock(&self.inner);
                inner.get_mut(&radio).map(|e| {
                    let from = e.life.state();
                    e.life = Lifecycle::new(resolved, now, self.life_times(e));
                    crate::lifecycle::Transition {
                        from,
                        to: resolved,
                        reason: "start-up reconcile",
                    }
                })
            };
            if let Some(t) = t {
                if t.from != t.to || disagrees || recorded != resolved {
                    let t2 = crate::lifecycle::Transition {
                        from: if t.from == t.to { recorded } else { t.from },
                        ..t
                    };
                    self.apply(&radio, t2, None);
                } else {
                    self.station.set_radio_state(&radio, resolved);
                }
            }
            items.push(ReconcileItem {
                radio,
                real_power,
                recorded,
                resolved,
                disagrees,
            });
        }
        items
    }

    // [@ANCHOR: radioctl_station_manager_life_times]
    /// The timing for one radio: the station's, with the record's boot time if it has one.
    fn life_times(&self, e: &Entry) -> LifecycleTimes {
        let mut t = self.times;
        if e.record.boot_time_ms > 0 {
            t.boot_time = Duration::from_millis(e.record.boot_time_ms);
        }
        t
    }
}

// [@ANCHOR: radioctl_station_manager_unknown]
/// The error for a radio the station does not have.
fn unknown(radio: &DeviceId) -> StationError {
    StationError::new(StationErrorKind::Unknown, format!("no radio {}", radio.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{Fault, Switchboard};
    use crate::model::{ChannelId, StationDef};
    use crate::station::{KeyState, KeyedSource};
    use std::sync::atomic::{AtomicU8, Ordering};

    const DEF: &str = r#"
[[channels]]
id = "radio_pwr"
type = "power"
driver = "bank"
line = 0
safe = "off"
min_off_ms = 500
[[channels]]
id = "amp_pwr"
type = "power"
driver = "bank"
line = 1
safe = "off"
[[devices]]
id = "ic7300"
kind = "radio"
power = { source = "channel", channel = "radio_pwr" }
[[devices]]
id = "amp"
kind = "amplifier"
power = { source = "channel", channel = "amp_pwr" }
depends_on = ["ic7300"]
[[devices]]
id = "k3"
kind = "radio"
power = { source = "always_on" }
"#;

    struct Key(AtomicU8);
    impl KeyedSource for Key {
        fn name(&self) -> String {
            "rig".into()
        }
        fn key_state(&self) -> BoxFuture<'_, crate::station::KeyState> {
            Box::pin(async move {
                match self.0.load(Ordering::SeqCst) {
                    0 => KeyState::Unkeyed,
                    1 => KeyState::Keyed,
                    _ => KeyState::Unknown,
                }
            })
        }
    }

    struct Rig {
        mgr: Arc<RadioManager>,
        bank: VirtualBank,
        link: VirtualRadioLink,
        key: Arc<Key>,
        path: std::path::PathBuf,
    }

    fn dev(s: &str) -> DeviceId {
        DeviceId(s.into())
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn rig(name: &str, boot: Option<Duration>) -> Rig {
        let path = std::env::temp_dir().join(format!("rs-mgr-{name}-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        build(path, boot, VirtualBank::new())
    }

    fn build(path: std::path::PathBuf, boot: Option<Duration>, bank: VirtualBank) -> Rig {
        let def = StationDef::from_toml(DEF).unwrap();
        let mut b = Switchboard::new();
        for c in def.channels.clone() {
            b.add_channel(c, Arc::new(bank.clone())).unwrap();
        }
        let key = Arc::new(Key(AtomicU8::new(0)));
        let station = Arc::new(Station::new(def, Arc::new(b), vec![key.clone()], None).unwrap());
        let link = VirtualRadioLink::new(bank.clone());
        link.add(dev("ic7300"), 0, boot);
        let t0 = Instant::now();
        let clock: Arc<dyn Fn() -> u64 + Send + Sync> =
            Arc::new(move || 1_000 + t0.elapsed().as_secs());
        let mgr = Arc::new(RadioManager::new(
            station,
            Arc::new(link.clone()),
            RecordStore::new(&path),
            clock,
            LifecycleTimes::default(),
        ));
        Rig {
            mgr,
            bank,
            link,
            key,
            path,
        }
    }

    fn drain(rx: &mut broadcast::Receiver<RadioEvent>) -> Vec<(RadioState, RadioState)> {
        let mut v = Vec::new();
        while let Ok(e) = rx.try_recv() {
            v.push((e.from, e.to));
        }
        v
    }

    // Tests [@ANCHOR: radioctl_station_manager_new]
    // Tests [@ANCHOR: radioctl_station_manager_ids]
    // Tests [@ANCHOR: radioctl_station_manager_subscribe]
    // Tests [@ANCHOR: radioctl_station_manager_state]
    // Tests [@ANCHOR: radioctl_station_manager_record]
    // Tests [@ANCHOR: radioctl_station_manager_cached]
    // Tests [@ANCHOR: radioctl_station_manager_note_state]
    // Tests [@ANCHOR: radioctl_station_manager_persist]
    // Tests [@ANCHOR: radioctl_station_manager_press]
    // Tests [@ANCHOR: radioctl_station_manager_press_on]
    // Tests [@ANCHOR: radioctl_station_manager_step_state]
    // Tests [@ANCHOR: radioctl_station_manager_power_source]
    // Tests [@ANCHOR: radioctl_station_manager_tick]
    // Tests [@ANCHOR: radioctl_station_manager_connect_step]
    // Tests [@ANCHOR: radioctl_station_manager_apply]
    // Tests [@ANCHOR: radioctl_station_manager_gather]
    // Tests [@ANCHOR: radioctl_station_manager_power_evidence]
    // Tests [@ANCHOR: radioctl_station_manager_now]
    // Tests [@ANCHOR: radioctl_station_manager_life_times]
    // Tests [@ANCHOR: radioctl_station_manager_lock]
    // Tests [@ANCHOR: radioctl_station_manager_unknown]
    // Tests [@ANCHOR: radioctl_station_vlink_new]
    // Tests [@ANCHOR: radioctl_station_vlink_add]
    // Tests [@ANCHOR: radioctl_station_vlink_link_up]
    // Tests [@ANCHOR: radioctl_station_vlink_present]
    // Tests [@ANCHOR: radioctl_station_vlink_connect]
    // Tests [@ANCHOR: radioctl_station_vlink_refresh]
    // Tests [@ANCHOR: radioctl_station_vlink_set_report]
    #[tokio::test(start_paused = true)]
    async fn the_power_button_walks_a_radio_up_and_reports_without_restoring() {
        let r = rig("happy", Some(secs(5)));
        assert_eq!(r.mgr.ids(), vec![dev("ic7300"), dev("k3")]);
        r.link.set_report(
            &dev("ic7300"),
            KnownState {
                frequency_hz: Some(7_074_000),
                ..KnownState::default()
            },
        );
        let items = r.mgr.reconcile().await;
        sleep(secs(1)).await; // past the channel's minimum off time
        assert_eq!(items[0].resolved, RadioState::Off);
        let mut rx = r.mgr.subscribe();
        let rep = r.mgr.press(&dev("ic7300"), Button::On).await.unwrap();
        assert_eq!(rep.state, RadioState::Ready);
        assert_eq!(
            drain(&mut rx),
            vec![
                (RadioState::Off, RadioState::PoweringOn),
                (RadioState::PoweringOn, RadioState::Booting),
                (RadioState::Booting, RadioState::Ready),
            ]
        );
        // the only write was the power-on: nothing was restored to the radio
        assert_eq!(r.bank.log(), vec![(0, true)]);
        // the reported state was recorded and is served with its age
        let c = r.mgr.cached(&dev("ic7300")).unwrap();
        assert_eq!(c.state.frequency_hz, Some(7_074_000));
        tokio::time::sleep(secs(90)).await;
        assert!(r.mgr.cached(&dev("ic7300")).unwrap().age >= secs(90));
        // pressing On again does nothing; Off takes the amplifier down first
        assert_eq!(
            r.mgr.press(&dev("ic7300"), Button::On).await.unwrap().state,
            RadioState::Ready
        );
        let off = r.mgr.press(&dev("ic7300"), Button::Off).await.unwrap();
        assert_eq!(off.state, RadioState::Off);
        assert_eq!(off.powered_off, vec![dev("amp"), dev("ic7300")]);
        // while off the cached state is still served, with a growing age
        assert!(r.mgr.cached(&dev("ic7300")).is_some());
        // note_state, record and persistence across a restart
        r.mgr
            .note_state(
                &dev("ic7300"),
                KnownState {
                    mode: Some("CW".into()),
                    ..KnownState::default()
                },
                Some(vec!["ReadPtt".into()]),
            )
            .unwrap();
        assert!(r
            .mgr
            .note_state(&dev("nope"), KnownState::default(), None)
            .is_err());
        let again = build(r.path.clone(), Some(secs(5)), r.bank.clone());
        let rec = again.mgr.record(&dev("ic7300")).unwrap();
        assert_eq!(rec.capabilities, vec!["ReadPtt".to_string()]);
        assert_eq!(rec.last_state.unwrap().mode.as_deref(), Some("CW"));
        assert_eq!(rec.lifecycle, RadioState::Off);
        assert!(again.mgr.problems().is_empty());
        // no button for a radio with no power source, or for an unknown one
        assert_eq!(
            r.mgr.press(&dev("k3"), Button::On).await.unwrap_err().kind,
            StationErrorKind::Refused
        );
        assert_eq!(
            r.mgr.press(&dev("amp"), Button::On).await.unwrap_err().kind,
            StationErrorKind::Unknown
        );
        let _ = std::fs::remove_file(&r.path);
    }

    // Tests [@ANCHOR: radioctl_station_manager_press]
    // Tests [@ANCHOR: radioctl_station_manager_connect_step]
    // Tests [@ANCHOR: radioctl_station_manager_tick]
    #[tokio::test(start_paused = true)]
    async fn a_switch_that_confirms_but_a_radio_that_never_boots_is_unreachable_and_can_be_power_cycled(
    ) {
        let r = rig("noboot", None);
        r.mgr.reconcile().await;
        sleep(secs(1)).await; // past the channel's minimum off time
        let mut rx = r.mgr.subscribe();
        let rep = r.mgr.press(&dev("ic7300"), Button::On).await.unwrap();
        assert_eq!(rep.state, RadioState::Unreachable);
        let seen = drain(&mut rx);
        assert_eq!(
            seen.last(),
            Some(&(RadioState::Booting, RadioState::Unreachable))
        );
        // reconnects backed off: far fewer attempts than one per poll
        let tries = r.link.connects(&dev("ic7300"));
        assert!((1..=12).contains(&tries), "{tries}");
        // powered but unreachable: On is refused, Reboot is the way, even with keyed state unknown
        assert_eq!(
            r.mgr
                .press(&dev("ic7300"), Button::On)
                .await
                .unwrap_err()
                .kind,
            StationErrorKind::Refused
        );
        r.key.0.store(2, Ordering::SeqCst);
        r.link.set_boot(&dev("ic7300"), Some(secs(3)));
        let rep = r.mgr.press(&dev("ic7300"), Button::Reboot).await.unwrap();
        assert_eq!(rep.state, RadioState::Ready);
        // the dependent amplifier is driven off first (its state was unknown), then the radio
        assert_eq!(
            r.bank.log(),
            vec![(0, true), (1, false), (0, false), (0, true)]
        );
        // a known keyed state refuses even the reboot
        r.key.0.store(1, Ordering::SeqCst);
        assert_eq!(
            r.mgr
                .press(&dev("ic7300"), Button::Reboot)
                .await
                .unwrap_err()
                .kind,
            StationErrorKind::Refused
        );
        assert_eq!(r.mgr.state(&dev("ic7300")), Some(RadioState::Ready));
        // health was counted
        assert!(r.mgr.record(&dev("ic7300")).unwrap().health.failures >= 1);
        let _ = std::fs::remove_file(&r.path);
    }

    // Tests [@ANCHOR: radioctl_station_manager_spawn_monitor]
    // Tests [@ANCHOR: radioctl_station_manager_spawn_callback]
    // Tests [@ANCHOR: radioctl_station_vlink_drop_link]
    // Tests [@ANCHOR: radioctl_station_vlink_heal]
    // Tests [@ANCHOR: radioctl_station_vlink_connects]
    // Tests [@ANCHOR: radioctl_station_vlink_set_boot]
    // Tests [@ANCHOR: radioctl_station_manager_poll]
    #[tokio::test(start_paused = true)]
    async fn a_radio_that_drops_while_powered_degrades_then_comes_back_without_any_write() {
        let r = rig("drop", Some(secs(2)));
        r.mgr.reconcile().await;
        sleep(secs(1)).await; // past the channel's minimum off time
        let seen: Arc<Mutex<Vec<RadioEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let _cb = r
            .mgr
            .spawn_callback(&Handle::current(), move |e| lock(&sink).push(e));
        let _mon = r
            .mgr
            .spawn_monitor(&Handle::current(), Duration::from_millis(500));
        r.mgr.press(&dev("ic7300"), Button::On).await.unwrap();
        assert_eq!(r.mgr.state(&dev("ic7300")), Some(RadioState::Ready));
        let writes = r.bank.log();
        r.link.drop_link(&dev("ic7300"));
        sleep(secs(2)).await;
        assert_eq!(r.mgr.state(&dev("ic7300")), Some(RadioState::Degraded));
        // it stays down past the degraded time-out: Unreachable
        sleep(secs(70)).await;
        assert_eq!(r.mgr.state(&dev("ic7300")), Some(RadioState::Unreachable));
        let tries = r.link.connects(&dev("ic7300"));
        assert!(tries < 40, "backoff keeps attempts rare: {tries}");
        // the radio comes back by itself: reconnects and reports; nothing is written
        r.link.heal(&dev("ic7300"));
        sleep(secs(20)).await;
        assert_eq!(r.mgr.state(&dev("ic7300")), Some(RadioState::Ready));
        assert_eq!(r.bank.log(), writes);
        let path: Vec<RadioState> = lock(&seen).iter().map(|e| e.to).collect();
        assert_eq!(
            path,
            vec![
                RadioState::PoweringOn,
                RadioState::Booting,
                RadioState::Ready,
                RadioState::Degraded,
                RadioState::Unreachable,
                RadioState::Ready
            ]
        );
        // a poll with nothing new changes nothing
        assert!(r.mgr.poll(&dev("ic7300")).await.unwrap().is_none());
        assert!(r.mgr.poll(&dev("nope")).await.is_err());
        let _ = std::fs::remove_file(&r.path);
    }

    // Tests [@ANCHOR: radioctl_station_manager_reconcile]
    // Tests [@ANCHOR: radioctl_station_manager_problems]
    #[tokio::test(start_paused = true)]
    async fn reconcile_reads_without_writing_and_reports_disagreement_with_the_record() {
        // a record that says Ready, with a cached state, while the switch is really off
        let first = rig("recon", Some(secs(2)));
        first.mgr.reconcile().await;
        sleep(secs(1)).await;
        first.mgr.press(&dev("ic7300"), Button::On).await.unwrap();
        first
            .mgr
            .note_state(
                &dev("ic7300"),
                KnownState {
                    frequency_hz: Some(14_200_000),
                    ..KnownState::default()
                },
                None,
            )
            .unwrap();
        assert_eq!(
            first.mgr.record(&dev("ic7300")).unwrap().lifecycle,
            RadioState::Ready
        );
        // restart: the line is physically off (power failed)
        let bank = VirtualBank::new();
        let second = build(first.path.clone(), Some(secs(2)), bank.clone());
        let items = second.mgr.reconcile().await;
        let item = items.iter().find(|i| i.radio == dev("ic7300")).unwrap();
        assert_eq!(
            (item.recorded, item.resolved, item.disagrees),
            (RadioState::Ready, RadioState::Off, true)
        );
        assert_eq!(item.real_power, Some(Value::Off));
        assert!(bank.log().is_empty(), "reconcile must not write");
        assert_eq!(
            second
                .mgr
                .cached(&dev("ic7300"))
                .unwrap()
                .state
                .frequency_hz,
            Some(14_200_000)
        );
        // the other way: the record says Off, but the line is really on (switched by hand)
        bank.set_physical(0, true);
        let third = build(first.path.clone(), Some(secs(2)), bank.clone());
        let items = third.mgr.reconcile().await;
        let item = items.iter().find(|i| i.radio == dev("ic7300")).unwrap();
        assert_eq!(
            (item.recorded, item.resolved, item.disagrees),
            (RadioState::Off, RadioState::Booting, true)
        );
        assert!(bank.log().is_empty());
        // a switch that cannot be read: Unreachable, nothing written, cached state kept
        let bank2 = VirtualBank::new();
        bank2.set_fault(Fault::FailReads);
        let fourth = build(first.path.clone(), Some(secs(2)), bank2.clone());
        let items = fourth.mgr.reconcile().await;
        let item = items.iter().find(|i| i.radio == dev("ic7300")).unwrap();
        assert_eq!(
            (item.real_power, item.resolved),
            (None, RadioState::Unreachable)
        );
        assert!(bank2.log().is_empty());
        assert!(fourth.mgr.cached(&dev("ic7300")).is_some());
        // a corrupt record file never crashes or gets trusted
        std::fs::write(&first.path, b"{ garbage").unwrap();
        let fifth = build(first.path.clone(), Some(secs(2)), VirtualBank::new());
        assert!(!fifth.mgr.problems().is_empty());
        assert!(fifth
            .mgr
            .record(&dev("ic7300"))
            .unwrap()
            .last_state
            .is_none());
        assert_eq!(fifth.mgr.record(&dev("ic7300")).unwrap().make, "unknown");
        let _ = ChannelId("x".into());
        let _ = std::fs::remove_file(&first.path);
    }
}
