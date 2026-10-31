// SPDX-License-Identifier: AGPL-3.0-or-later
//! The station: the dependency graph, profiles and interlocks over a [`Switchboard`].
//!
//! Every command that switches anything passes the same guard first: nothing switches while any
//! attached radio is keyed or its PTT reads on, and an unknown keyed state counts as keyed (fail
//! closed). The one exception is [`Station::all_off`], which is never blocked, needs no
//! confirmation and no password, and does not look at the keyed state: cutting power is always
//! allowed.

use crate::channel::{BoxFuture, StationEvent, Switchboard};
use crate::error::{StationError, StationErrorKind};
use crate::model::{
    ChannelId, ChannelKind, DeviceDef, DeviceId, PowerSource, ProfileStep, StationDef, Value,
};
use radioctl_core::capability::Capability;
use radioctl_core::op::{Field, Observation};
use radioctl_core::rig::{Freshness, RigHandle};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout, Instant};

/// Whether a transmitter is keyed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    /// Known not to be transmitting.
    Unkeyed,
    /// Transmitting, or its PTT reads on.
    Keyed,
    /// Could not be determined.
    Unknown,
}

/// A source of keyed state for the interlock: a rig handle, or a hardware PTT input.
pub trait KeyedSource: Send + Sync {
    /// A name for messages.
    fn name(&self) -> String;
    /// The current keyed state.
    fn key_state(&self) -> BoxFuture<'_, KeyState>;
}

impl KeyedSource for RigHandle {
    // [@ANCHOR: radioctl_station_rig_name]
    /// The rig id.
    fn name(&self) -> String {
        self.id().to_string()
    }

    // [@ANCHOR: radioctl_station_rig_key_state]
    /// Keyed state from the rig handle: unknown when the link is down; the rig's own PTT read
    /// (at most one second old) when it has `read-ptt`; otherwise the cached PTT observation, and
    /// unknown when there is none. Nothing here opens the transmit gate or transmits.
    fn key_state(&self) -> BoxFuture<'_, KeyState> {
        Box::pin(async move {
            if !self.link_up() {
                return KeyState::Unknown;
            }
            if self.capabilities().has(Capability::ReadPtt) {
                return match self.ptt(Freshness::MaxAge(Duration::from_secs(1))).await {
                    Ok(c) if c.value => KeyState::Keyed,
                    Ok(_) => KeyState::Unkeyed,
                    Err(_) => KeyState::Unknown,
                };
            }
            match self.cached(Field::Ptt).map(|c| c.value) {
                Some(Observation::Ptt(true)) => KeyState::Keyed,
                Some(Observation::Ptt(false)) => KeyState::Unkeyed,
                _ => KeyState::Unknown,
            }
        })
    }
}

/// Answers whether a device is up and answering after power-on.
pub trait Readiness: Send + Sync {
    /// Whether `device` answers now.
    fn is_ready(&self, device: &DeviceId) -> BoxFuture<'_, bool>;
}

/// The result of an all-off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllOffReport {
    /// Power channels driven off, in the order done.
    pub off: Vec<ChannelId>,
    /// Power channels that could not be driven off, with the reason.
    pub failed: Vec<(ChannelId, String)>,
}

/// A station: the definition, its switchboard and its interlock sources.
pub struct Station {
    def: StationDef,
    board: Arc<Switchboard>,
    keyed: Vec<Arc<dyn KeyedSource>>,
    ready: Option<Arc<dyn Readiness>>,
}

const ALL_OFF_STEP_TIMEOUT: Duration = Duration::from_secs(3);
const READY_POLL: Duration = Duration::from_millis(100);

fn refused(m: String) -> StationError {
    StationError::new(StationErrorKind::Refused, m)
}

impl Station {
    // [@ANCHOR: radioctl_station_station_new]
    /// Builds a station over a switchboard that has every channel of `def`. Nothing is switched.
    pub fn new(
        def: StationDef,
        board: Arc<Switchboard>,
        keyed: Vec<Arc<dyn KeyedSource>>,
        ready: Option<Arc<dyn Readiness>>,
    ) -> Result<Station, StationError> {
        def.validate()?;
        for c in &def.channels {
            board.def(&c.id)?;
        }
        Ok(Station {
            def,
            board,
            keyed,
            ready,
        })
    }

    // [@ANCHOR: radioctl_station_station_board]
    /// The switchboard, for status and events. Switching goes through the station.
    pub fn board(&self) -> &Arc<Switchboard> {
        &self.board
    }

    // [@ANCHOR: radioctl_station_station_def]
    /// The definition.
    pub fn def(&self) -> &StationDef {
        &self.def
    }

    // [@ANCHOR: radioctl_station_station_guard]
    /// The interlock: errors unless every keyed source reports unkeyed. A refusal is an event.
    pub async fn guard(&self, what: &str) -> Result<(), StationError> {
        for s in &self.keyed {
            let (kind, why) = match s.key_state().await {
                KeyState::Unkeyed => continue,
                KeyState::Keyed => (StationErrorKind::Refused, format!("{} is keyed", s.name())),
                KeyState::Unknown => (
                    StationErrorKind::KeyedUnknown,
                    format!("cannot tell whether {} is keyed", s.name()),
                ),
            };
            self.board.emit(StationEvent::Refused {
                what: what.to_string(),
                why: why.clone(),
            });
            return Err(StationError::new(kind, format!("{what}: {why}")));
        }
        Ok(())
    }

    fn device(&self, id: &DeviceId) -> Result<&DeviceDef, StationError> {
        self.def.device(id).ok_or_else(|| {
            StationError::new(StationErrorKind::Unknown, format!("no device {}", id.0))
        })
    }

    // [@ANCHOR: radioctl_station_station_device_power]
    /// The confirmed power state of a device: always-on is `On`; a channel's confirmed value;
    /// `None` when unknown (a momentary-powered device has no power sensor here).
    pub fn device_power(&self, id: &DeviceId) -> Result<Option<Value>, StationError> {
        Ok(match &self.device(id)?.power {
            PowerSource::AlwaysOn => Some(Value::On),
            PowerSource::Channel { channel } => self.board.status(channel)?.confirmed,
            PowerSource::Momentary { .. } => None,
        })
    }

    // [@ANCHOR: radioctl_station_station_set_channel]
    /// Sets a power, output or selector channel by hand, behind the interlock. A power channel
    /// that powers an amplifier-like device still gets the same refusals as `power_on`.
    pub async fn set_channel(&self, id: &ChannelId, value: Value) -> Result<(), StationError> {
        self.guard(&format!("set {}", id.0)).await?;
        if value == Value::On {
            if let Some(d) = self
                .def
                .devices
                .iter()
                .find(|d| matches!(&d.power, PowerSource::Channel { channel } if channel == id))
            {
                self.check_power_on(d)?;
            }
        }
        self.board.set(id, value).await.map(|_| ())
    }

    // [@ANCHOR: radioctl_station_station_select]
    /// Selects a way on a selector, behind the interlock.
    pub async fn select(&self, id: &ChannelId, port: u32) -> Result<(), StationError> {
        self.guard(&format!("select {} {port}", id.0)).await?;
        self.board.set(id, Value::Port(port)).await.map(|_| ())
    }

    // [@ANCHOR: radioctl_station_station_pulse]
    /// Pulses a momentary channel, behind the interlock.
    pub async fn pulse(&self, id: &ChannelId) -> Result<(), StationError> {
        self.guard(&format!("pulse {}", id.0)).await?;
        self.board.pulse(id).await
    }

    // [@ANCHOR: radioctl_station_station_check_power_on]
    /// The dependency refusals: dependencies confirmed on, required antennas selected.
    fn check_power_on(&self, d: &DeviceDef) -> Result<(), StationError> {
        for dep in &d.depends_on {
            if self.device_power(dep)? != Some(Value::On) {
                return Err(refused(format!("{} needs {} on first", d.id.0, dep.0)));
            }
        }
        for a in &d.needs_antenna {
            if !matches!(self.board.status(a)?.confirmed, Some(Value::Port(_))) {
                return Err(refused(format!(
                    "{} needs an antenna selected on {}",
                    d.id.0, a.0
                )));
            }
        }
        Ok(())
    }

    // [@ANCHOR: radioctl_station_station_power_on]
    /// Powers a device on: refused while anything is keyed, while a device it depends on is not
    /// confirmed on, or while a required antenna selector has no confirmed way. Then the settle
    /// wait, and, when the device has a ready timeout and a [`Readiness`] source, waits for it to
    /// answer.
    pub async fn power_on(&self, id: &DeviceId) -> Result<(), StationError> {
        let d = self.device(id)?.clone();
        self.guard(&format!("power on {}", id.0)).await?;
        self.check_power_on(&d)?;
        match &d.power {
            PowerSource::AlwaysOn => return Ok(()),
            PowerSource::Channel { channel } => {
                self.board.set(channel, Value::On).await?;
            }
            PowerSource::Momentary { on, .. } => self.board.pulse(on).await?,
        }
        if let ChannelKind::Power {
            inrush_stagger_ms, ..
        } = self.power_kind(&d)
        {
            sleep(Duration::from_millis(inrush_stagger_ms)).await;
        }
        sleep(Duration::from_millis(d.settle_ms)).await;
        self.wait_ready(&d).await
    }

    // [@ANCHOR: radioctl_station_station_power_kind]
    /// The channel kind that powers a device (an output kind for none).
    fn power_kind(&self, d: &DeviceDef) -> ChannelKind {
        match &d.power {
            PowerSource::Channel { channel } => self.def.channel(channel).map(|c| c.kind.clone()),
            _ => None,
        }
        .unwrap_or(ChannelKind::Output)
    }

    // [@ANCHOR: radioctl_station_station_wait_ready]
    /// Waits for a device to answer, up to its ready timeout.
    async fn wait_ready(&self, d: &DeviceDef) -> Result<(), StationError> {
        let Some(ready) = self.ready.as_ref().filter(|_| d.ready_timeout_ms > 0) else {
            return Ok(());
        };
        let deadline = Instant::now() + Duration::from_millis(d.ready_timeout_ms);
        while !ready.is_ready(&d.id).await {
            if Instant::now() >= deadline {
                return Err(StationError::new(
                    StationErrorKind::Timeout,
                    format!("{} did not answer within {} ms", d.id.0, d.ready_timeout_ms),
                ));
            }
            sleep(READY_POLL).await;
        }
        Ok(())
    }

    // [@ANCHOR: radioctl_station_station_power_off]
    /// Orderly power-off: refused while anything is keyed. Devices that depend on this one go
    /// first, the most dependent first, each after its own off delay. Returns the devices
    /// powered off, in order.
    pub async fn power_off(&self, id: &DeviceId) -> Result<Vec<DeviceId>, StationError> {
        let target = self.device(id)?.clone();
        self.guard(&format!("power off {}", id.0)).await?;
        let mut order = self.def.dependents(id);
        order.push(id.clone());
        let mut done = Vec::new();
        for dev in order {
            let d = self.device(&dev)?.clone();
            if matches!(self.device_power(&dev)?, Some(Value::Off)) {
                continue;
            }
            self.guard(&format!("power off {}", dev.0)).await?;
            sleep(Duration::from_millis(d.off_delay_ms)).await;
            match &d.power {
                PowerSource::AlwaysOn => {
                    if d.id == target.id {
                        return Err(refused(format!("{} has no power switch", d.id.0)));
                    }
                }
                PowerSource::Channel { channel } => {
                    self.board.set(channel, Value::Off).await?;
                    done.push(dev);
                }
                PowerSource::Momentary { off: Some(off), .. } => {
                    self.board.pulse(off).await?;
                    done.push(dev);
                }
                PowerSource::Momentary { off: None, .. } => {
                    return Err(refused(format!("{} has no power-off line", d.id.0)));
                }
            }
        }
        Ok(done)
    }

    // [@ANCHOR: radioctl_station_station_run_profile]
    /// Runs a profile's steps in order within its timeout, with a `Step` event at each. A device
    /// already on is skipped. The first failure stops the sequence and is returned; nothing that
    /// was switched on is switched back off automatically.
    pub async fn run_profile(&self, name: &str) -> Result<(), StationError> {
        let p = self.profile(name)?.clone();
        let run = async {
            for (i, step) in p.steps.iter().enumerate() {
                let detail = format!("{step:?}");
                self.board.emit(StationEvent::Step {
                    name: p.name.clone(),
                    index: i,
                    detail: detail.clone(),
                });
                match step {
                    ProfileStep::Power { device } => {
                        if self.device_power(device)? != Some(Value::On) {
                            self.power_on(device).await
                        } else {
                            Ok(())
                        }
                    }
                    ProfileStep::Select { channel, port } => self.select(channel, *port).await,
                }
                .inspect_err(|e| {
                    self.board.emit(StationEvent::Step {
                        name: p.name.clone(),
                        index: i,
                        detail: format!("failed: {}", e.detail),
                    });
                })?;
            }
            Ok(())
        };
        match timeout(Duration::from_millis(p.timeout_ms), run).await {
            Ok(r) => r,
            Err(_) => Err(StationError::new(
                StationErrorKind::Timeout,
                format!("profile {name} ran past {} ms", p.timeout_ms),
            )),
        }
    }

    // [@ANCHOR: radioctl_station_station_stop_profile]
    /// Powers off the devices a profile powers on, in reverse order, with the off delays and the
    /// dependents-first rule.
    pub async fn stop_profile(&self, name: &str) -> Result<(), StationError> {
        let p = self.profile(name)?.clone();
        let devices: Vec<DeviceId> = p
            .steps
            .iter()
            .filter_map(|s| {
                if let ProfileStep::Power { device } = s {
                    Some(device.clone())
                } else {
                    None
                }
            })
            .collect();
        for (i, device) in devices.iter().rev().enumerate() {
            self.board.emit(StationEvent::Step {
                name: p.name.clone(),
                index: i,
                detail: format!("power off {}", device.0),
            });
            if self.device_power(device)? == Some(Value::Off) {
                continue;
            }
            self.power_off(device).await?;
        }
        Ok(())
    }

    fn profile(&self, name: &str) -> Result<&crate::model::ProfileDef, StationError> {
        self.def
            .profiles
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| {
                StationError::new(StationErrorKind::Unknown, format!("no profile {name}"))
            })
    }

    // [@ANCHOR: radioctl_station_station_all_off]
    /// The emergency all-off. Never blocked: no keyed-state check, no minimum times, no delays,
    /// no confirmation, no password. Power channels go off in the safe order (devices that others
    /// depend on come after those that depend on them), then every other power channel; a failing
    /// channel does not stop the rest, and each is limited to a few seconds. Selectors are left
    /// where they are. Momentary-powered radios have no off line here and are not touched.
    pub async fn all_off(&self) -> AllOffReport {
        let mut order: Vec<ChannelId> = Vec::new();
        for dev in self
            .def
            .power_on_order()
            .unwrap_or_default()
            .into_iter()
            .rev()
        {
            if let Some(PowerSource::Channel { channel }) =
                self.def.device(&dev).map(|d| d.power.clone())
            {
                order.push(channel);
            }
        }
        for c in &self.def.channels {
            if matches!(c.kind, ChannelKind::Power { .. }) && !order.contains(&c.id) {
                order.push(c.id.clone());
            }
        }
        let mut report = AllOffReport {
            off: Vec::new(),
            failed: Vec::new(),
        };
        for ch in order {
            match timeout(ALL_OFF_STEP_TIMEOUT, self.board.force_off(&ch)).await {
                Ok(Ok(())) => report.off.push(ch),
                Ok(Err(e)) => report.failed.push((ch, e.detail)),
                Err(_) => report.failed.push((ch, "timed out".to_string())),
            }
        }
        self.board.emit(StationEvent::AllOff {
            failed: report.failed.iter().map(|f| f.0.clone()).collect(),
        });
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{Fault, VirtualBank};
    use std::sync::atomic::{AtomicU8, Ordering};

    struct Key(AtomicU8);
    impl KeyedSource for Key {
        fn name(&self) -> String {
            "rig".into()
        }
        fn key_state(&self) -> BoxFuture<'_, KeyState> {
            Box::pin(async move {
                match self.0.load(Ordering::SeqCst) {
                    0 => KeyState::Unkeyed,
                    1 => KeyState::Keyed,
                    _ => KeyState::Unknown,
                }
            })
        }
    }

    struct Answers(AtomicU8);
    impl Readiness for Answers {
        fn is_ready(&self, _device: &DeviceId) -> BoxFuture<'_, bool> {
            Box::pin(async move { self.0.fetch_add(1, Ordering::SeqCst) >= 3 })
        }
    }

    fn dev(s: &str) -> DeviceId {
        DeviceId(s.into())
    }
    fn ch(s: &str) -> ChannelId {
        ChannelId(s.into())
    }

    fn rig(ready: Option<Arc<dyn Readiness>>) -> (Station, VirtualBank, Arc<Key>) {
        let def = StationDef::from_toml(crate::model::tests::SAMPLE).unwrap();
        let bank = VirtualBank::new();
        let mut b = Switchboard::new();
        for c in def.channels.clone() {
            b.add_channel(c, Arc::new(bank.clone())).unwrap();
        }
        let key = Arc::new(Key(AtomicU8::new(0)));
        let st = Station::new(def, Arc::new(b), vec![key.clone()], ready).unwrap();
        (st, bank, key)
    }

    // Tests [@ANCHOR: radioctl_station_station_new]
    // Tests [@ANCHOR: radioctl_station_station_board]
    // Tests [@ANCHOR: radioctl_station_station_def]
    // Tests [@ANCHOR: radioctl_station_station_device_power]
    // Tests [@ANCHOR: radioctl_station_station_power_on]
    // Tests [@ANCHOR: radioctl_station_station_check_power_on]
    // Tests [@ANCHOR: radioctl_station_station_power_kind]
    // Tests [@ANCHOR: radioctl_station_station_wait_ready]
    #[tokio::test(start_paused = true)]
    async fn an_amplifier_needs_the_radio_an_antenna_and_a_quiet_band() {
        let (st, bank, key) = rig(None);
        assert_eq!(st.def().devices.len(), 2);
        assert!(st.board().ids().len() >= 4);
        assert_eq!(st.device_power(&dev("amp")).unwrap(), None);
        // radio off: refused
        let e = st.power_on(&dev("amp")).await.unwrap_err();
        assert!(e.detail.contains("needs ic7300"), "{e}");
        st.power_on(&dev("ic7300")).await.unwrap();
        assert_eq!(st.device_power(&dev("ic7300")).unwrap(), Some(Value::On));
        // no antenna: refused
        let e = st.power_on(&dev("amp")).await.unwrap_err();
        assert!(e.detail.contains("antenna"), "{e}");
        st.select(&ch("ant"), 2).await.unwrap();
        // keyed: refused, nothing written
        key.0.store(1, Ordering::SeqCst);
        let n = bank.log().len();
        assert_eq!(
            st.power_on(&dev("amp")).await.unwrap_err().kind,
            StationErrorKind::Refused
        );
        key.0.store(2, Ordering::SeqCst);
        assert_eq!(
            st.power_on(&dev("amp")).await.unwrap_err().kind,
            StationErrorKind::KeyedUnknown
        );
        assert_eq!(bank.log().len(), n);
        key.0.store(0, Ordering::SeqCst);
        st.power_on(&dev("amp")).await.unwrap();
        assert!(bank.line(1));
        assert!(st.power_on(&dev("nope")).await.is_err());
    }

    // Tests [@ANCHOR: radioctl_station_station_guard]
    // Tests [@ANCHOR: radioctl_station_station_set_channel]
    // Tests [@ANCHOR: radioctl_station_station_select]
    // Tests [@ANCHOR: radioctl_station_station_pulse]
    #[tokio::test(start_paused = true)]
    async fn nothing_switches_while_keyed() {
        let (st, bank, key) = rig(None);
        let mut rx = st.board().subscribe();
        key.0.store(1, Ordering::SeqCst);
        assert!(st.set_channel(&ch("radio_pwr"), Value::On).await.is_err());
        assert!(st.select(&ch("ant"), 1).await.is_err());
        assert!(st.pulse(&ch("btn")).await.is_err());
        assert!(bank.log().is_empty());
        assert!(matches!(
            rx.recv().await.unwrap(),
            StationEvent::Refused { .. }
        ));
        key.0.store(0, Ordering::SeqCst);
        st.set_channel(&ch("radio_pwr"), Value::On).await.unwrap();
        // the channel route gets the dependency refusals too
        let e = st.set_channel(&ch("amp_pwr"), Value::On).await.unwrap_err();
        assert_eq!(e.kind, StationErrorKind::Refused);
        st.pulse(&ch("btn")).await.unwrap();
    }

    // Tests [@ANCHOR: radioctl_station_station_power_off]
    #[tokio::test(start_paused = true)]
    async fn power_off_takes_dependents_first_after_their_delay() {
        let (st, bank, _key) = rig(None);
        st.power_on(&dev("ic7300")).await.unwrap();
        st.select(&ch("ant"), 1).await.unwrap();
        st.power_on(&dev("amp")).await.unwrap();
        let start = Instant::now();
        let done = st.power_off(&dev("ic7300")).await.unwrap();
        assert_eq!(done, vec![dev("amp"), dev("ic7300")]);
        assert!(start.elapsed() >= Duration::from_millis(100));
        let offs: Vec<u32> = bank
            .log()
            .iter()
            .filter(|w| !w.1 && (w.0 == 0 || w.0 == 1))
            .map(|w| w.0)
            .collect();
        assert_eq!(offs, vec![1, 0]);
    }

    // Tests [@ANCHOR: radioctl_station_station_run_profile]
    // Tests [@ANCHOR: radioctl_station_station_stop_profile]
    #[tokio::test(start_paused = true)]
    async fn a_profile_runs_in_order_waits_for_the_radio_and_stops_in_reverse() {
        let (st, bank, _key) = rig(Some(Arc::new(Answers(AtomicU8::new(0)))));
        let mut rx = st.board().subscribe();
        let start = Instant::now();
        st.run_profile("hf_on").await.unwrap();
        assert!(
            start.elapsed() >= Duration::from_millis(300),
            "waited for the radio to answer"
        );
        assert!(bank.line(0) && bank.line(1) && bank.line(3));
        let mut steps = 0;
        while let Ok(ev) = rx.try_recv() {
            if matches!(ev, StationEvent::Step { .. }) {
                steps += 1;
            }
        }
        assert_eq!(steps, 3);
        st.run_profile("hf_on").await.unwrap(); // everything already on
        st.stop_profile("hf_on").await.unwrap();
        assert!(!bank.line(0) && !bank.line(1));
        assert!(st.run_profile("none").await.is_err());
        // a radio that never answers stops the sequence before the amplifier
        let (st, bank, _key) = rig(Some(Arc::new(Answers(AtomicU8::new(0)))));
        bank.set_fault(Fault::None);
        struct Never;
        impl Readiness for Never {
            fn is_ready(&self, _d: &DeviceId) -> BoxFuture<'_, bool> {
                Box::pin(async { false })
            }
        }
        let st2 = Station::new(
            st.def().clone(),
            st.board().clone(),
            vec![],
            Some(Arc::new(Never)),
        )
        .unwrap();
        let e = st2.run_profile("hf_on").await.unwrap_err();
        assert_eq!(e.kind, StationErrorKind::Timeout);
        assert!(!bank.line(1));
    }

    // Tests [@ANCHOR: radioctl_station_station_all_off]
    #[tokio::test(start_paused = true)]
    async fn all_off_is_never_blocked() {
        let (st, bank, key) = rig(None);
        st.power_on(&dev("ic7300")).await.unwrap();
        st.select(&ch("ant"), 2).await.unwrap();
        st.power_on(&dev("amp")).await.unwrap();
        key.0.store(1, Ordering::SeqCst);
        assert!(st.power_off(&dev("amp")).await.is_err());
        let rep = st.all_off().await;
        assert_eq!(rep.off, vec![ch("amp_pwr"), ch("radio_pwr")]);
        assert!(rep.failed.is_empty());
        assert!(!bank.line(0) && !bank.line(1));
        assert!(bank.line(3), "the antenna selection is left alone");
        // a failing driver is reported and the rest are still tried
        bank.set_fault(Fault::FailWrites);
        let rep = st.all_off().await;
        assert_eq!(rep.failed.len(), 2);
    }
}
