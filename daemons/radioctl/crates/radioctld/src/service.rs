// SPDX-License-Identifier: AGPL-3.0-or-later
//! The service in front of one rig: turns parsed commands into operations on the rig's single
//! actor queue, answering reads from the actor's cache when fresh enough.
//!
//! - No locks of its own around the rig: every client goes through [`RigHandle`], the one queue.
//!   The only shared state here is the PTT owner and the "current VFO", each held for a few
//!   instructions and never across an await.
//! - One transmitter, one owner, with the splitter's arbiter ([`PttOwner`]): the first client to
//!   key owns the transmitter; another client's key is refused until the owner unkeys or goes
//!   away (which unkeys the rig). Unkeying is open to everyone.
//! - Fail closed: keying and Morse are refused up front while the rig's transmit gate is closed
//!   or its link is down, before the arbiter is touched and before anything is queued, so a
//!   refused command never reaches the rig. The gate is opened only by the configuration.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use radioctl_core::capability::Capability;
use radioctl_core::description::RigDescription;
use radioctl_core::event::Source;
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::rig::RigHandle;
use radioctl_core::units::{Hz, Mode, Vfo};
use radioctl_splitter::arbiter::{ClientId, PttOwner};

use crate::caps::{dump_caps, dump_state, CapsView};
use crate::command::Command;
use crate::names::{
    func_lookup, level_lookup, level_range, mode_to_hamlib, nominal_width, served_levels, vfo_name,
    FuncKind, FuncLookup, LevelInfo, LevelLookup, LevelSource, VfoSel,
};
use crate::reply::{error_code, Body, EINVAL, EIO, ENAVAIL, ERJCTED};

/// Per-rig service settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceConfig {
    /// `\chk_vfo` answers 1 and responses echo the VFO.
    pub vfo_mode: bool,
    /// Reads at most this old are answered from the rig's cache.
    pub cache_max_age: Duration,
    /// Deadline for a request that has to go to the rig.
    pub request_timeout: Duration,
    /// The model number reported in `\dump_state`.
    pub hamlib_model: u32,
    /// Lowest frequency the rig is said to cover, in hertz.
    pub rx_low_hz: u64,
    /// Highest frequency the rig is said to cover, in hertz.
    pub rx_high_hz: u64,
    /// Maximum transmit power reported, in milliwatts.
    pub tx_power_mw: u32,
}

/// Counters of a service.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ServiceStats {
    /// Commands executed.
    pub commands: u64,
    /// Reads answered from the cache.
    pub cache_hits: u64,
    /// Transmit commands refused (gate closed, link down, other client owns the transmitter).
    pub transmit_refused: u64,
    /// Keying attempts refused because another client owns the transmitter.
    pub ptt_conflicts: u64,
}

#[derive(Default)]
struct Counters {
    commands: AtomicU64,
    cache_hits: AtomicU64,
    transmit_refused: AtomicU64,
    ptt_conflicts: AtomicU64,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The service for one rig. Share it between client tasks behind an `Arc`.
pub struct RigService {
    rig: RigHandle,
    description: RigDescription,
    config: ServiceConfig,
    owner: Mutex<PttOwner>,
    current: Mutex<Vfo>,
    counters: Counters,
}

// [@ANCHOR: radioctld_service_fmt_float]
/// A level value as text: six decimals with trailing zeros dropped (`0.5`, `1`, `0.117647`).
pub fn fmt_float(value: f64) -> String {
    let text = format!("{value:.6}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl RigService {
    // [@ANCHOR: radioctld_service_new]
    /// A service for `rig`, described by `description`.
    pub fn new(rig: RigHandle, description: RigDescription, config: ServiceConfig) -> RigService {
        RigService {
            rig,
            description,
            config,
            owner: Mutex::new(PttOwner::default()),
            current: Mutex::new(Vfo::A),
            counters: Counters::default(),
        }
    }

    // [@ANCHOR: radioctld_service_rig]
    /// The rig's handle.
    pub fn rig(&self) -> &RigHandle {
        &self.rig
    }

    // [@ANCHOR: radioctld_service_config]
    /// The service settings.
    pub fn config(&self) -> &ServiceConfig {
        &self.config
    }

    // [@ANCHOR: radioctld_service_stats]
    /// A snapshot of the counters.
    pub fn stats(&self) -> ServiceStats {
        let c = &self.counters;
        ServiceStats {
            commands: c.commands.load(Ordering::Relaxed),
            cache_hits: c.cache_hits.load(Ordering::Relaxed),
            transmit_refused: c.transmit_refused.load(Ordering::Relaxed),
            ptt_conflicts: c.ptt_conflicts.load(Ordering::Relaxed),
        }
    }

    // [@ANCHOR: radioctld_service_ptt_owner]
    /// The client that owns the transmitter, if any.
    pub fn ptt_owner(&self) -> Option<ClientId> {
        locked(&self.owner).owner()
    }

    // [@ANCHOR: radioctld_service_caps_view]
    /// What `\dump_state` and `\dump_caps` are generated from.
    fn caps_view(&self) -> CapsView<'_> {
        CapsView {
            description: &self.description,
            hamlib_model: self.config.hamlib_model,
            rx_low_hz: self.config.rx_low_hz,
            rx_high_hz: self.config.rx_high_hz,
            tx_power_mw: self.config.tx_power_mw,
            transmit_enabled: self.rig.transmit_permit().is_some(),
        }
    }

    // [@ANCHOR: radioctld_service_disconnect]
    /// A client went away: if it owned the transmitter, give it up and unkey the rig.
    pub async fn disconnect(&self, client: ClientId) {
        let held = locked(&self.owner).release(client);
        if held {
            let _ = self.exec(Op::SetPtt(false)).await;
        }
    }

    // [@ANCHOR: radioctld_service_shutdown]
    /// The server is stopping: unkey the rig, whoever (if anyone) is recorded as holding it.
    pub async fn shutdown(&self) {
        // Unconditional: a rig keyed by Morse, or keyed while its owner record was lost, has no
        // owner, and unkeying a rig that is not keyed costs one command. A rig that cannot be
        // unkeyed by command answers Unsupported, which is all there is to do.
        let _ = self.unkey().await;
    }

    // [@ANCHOR: radioctld_service_exec]
    /// Runs one operation on the rig's queue.
    async fn exec(&self, op: Op) -> Result<Option<Observation>, i32> {
        self.rig
            .execute_from(op, self.config.request_timeout, Source::Api)
            .await
            .map_err(|e| error_code(&e))
    }

    // [@ANCHOR: radioctld_service_read]
    /// A read: the cached value if fresh enough, else asks the rig.
    async fn read(&self, field: Field, op: Op) -> Result<Observation, i32> {
        if let Some(cached) = self.rig.cached(field) {
            if cached.age <= self.config.cache_max_age {
                self.counters.cache_hits.fetch_add(1, Ordering::Relaxed);
                return Ok(cached.value);
            }
        }
        self.exec(op).await?.ok_or(EIO)
    }

    // [@ANCHOR: radioctld_service_set]
    /// A set: asks the rig and reports done.
    async fn set(&self, op: Op) -> Result<Body, i32> {
        self.exec(op).await.map(|_| Body::Done)
    }

    // [@ANCHOR: radioctld_service_resolve_vfo]
    /// The VFO a selection means right now.
    fn resolve_vfo(&self, selection: Option<VfoSel>) -> Result<Vfo, i32> {
        match selection.unwrap_or(VfoSel::Current) {
            VfoSel::Current => Ok(*locked(&self.current)),
            VfoSel::A | VfoSel::Rx => Ok(Vfo::A),
            VfoSel::B => Ok(Vfo::B),
            VfoSel::Tx => {
                let split = matches!(
                    self.rig.cached(Field::Split).map(|c| c.value),
                    Some(Observation::Split(true))
                );
                Ok(if split { Vfo::B } else { Vfo::A })
            }
            VfoSel::Absent => Err(ENAVAIL),
        }
    }

    // [@ANCHOR: radioctld_service_frequency]
    /// A VFO's frequency in hertz.
    async fn frequency(&self, vfo: Vfo) -> Result<u64, i32> {
        match self
            .read(Field::Frequency(vfo), Op::GetFrequency(vfo))
            .await?
        {
            Observation::Frequency(_, hz) => Ok(hz.0),
            _ => Err(EIO),
        }
    }

    // [@ANCHOR: radioctld_service_mode_and_width]
    /// The mode and the receive bandwidth in hertz (the mode's typical width when the rig has no
    /// filter control).
    async fn mode_and_width(&self) -> Result<(Mode, u32), i32> {
        let Observation::Mode(mode) = self.read(Field::Mode, Op::GetMode).await? else {
            return Err(EIO);
        };
        if !self.description.capabilities.has(Capability::Filter) {
            return Ok((mode, nominal_width(mode)));
        }
        match self.read(Field::FilterWidth, Op::GetFilterWidth).await? {
            Observation::FilterWidth(Hz(width)) => {
                Ok((mode, u32::try_from(width).map_err(|_| EIO)?))
            }
            _ => Err(EIO),
        }
    }

    // [@ANCHOR: radioctld_service_mode_fields]
    /// The `Mode` and `Passband` answer fields, with the given labels.
    async fn mode_fields(
        &self,
        mode_label: &'static str,
        width_label: &'static str,
    ) -> Result<Body, i32> {
        let (mode, width) = self.mode_and_width().await?;
        Ok(Body::Fields(vec![
            (mode_label, mode_to_hamlib(mode).to_string()),
            (width_label, width.to_string()),
        ]))
    }

    // [@ANCHOR: radioctld_service_set_mode]
    /// Sets the mode, and the receive bandwidth when one is asked for and the rig has the
    /// filter capability (0 and -1 leave the rig's own width).
    async fn set_mode(&self, mode: Mode, passband: i64) -> Result<Body, i32> {
        if !(-1..=1_000_000).contains(&passband) {
            return Err(EINVAL);
        }
        self.exec(Op::SetMode(mode)).await?;
        if passband > 0 && self.description.capabilities.has(Capability::Filter) {
            self.exec(Op::SetFilterWidth(Hz(passband.unsigned_abs())))
                .await?;
        }
        Ok(Body::Done)
    }

    // [@ANCHOR: radioctld_service_require_mode]
    /// A split transmit mode other than the current one cannot be set: radioctl rigs have one
    /// mode, and changing it would change the receive mode too.
    async fn require_current_mode(&self, wanted: Mode) -> Result<(), i32> {
        let (mode, _) = self.mode_and_width().await?;
        if mode == wanted {
            Ok(())
        } else {
            Err(ENAVAIL)
        }
    }

    // [@ANCHOR: radioctld_service_refuse]
    /// Counts a refused transmit command and answers with the refusal code.
    fn refuse(&self, code: i32) -> Result<Body, i32> {
        self.counters
            .transmit_refused
            .fetch_add(1, Ordering::Relaxed);
        Err(code)
    }

    // [@ANCHOR: radioctld_service_check_transmit]
    /// The fail-closed check in front of every command that can put RF on the air: the gate must
    /// be open and the link up. Nothing has been queued or claimed when this refuses.
    fn check_transmit(&self) -> Result<(), i32> {
        if self.rig.transmit_permit().is_none() {
            self.counters
                .transmit_refused
                .fetch_add(1, Ordering::Relaxed);
            return Err(ERJCTED);
        }
        if !self.rig.link_up() {
            self.counters
                .transmit_refused
                .fetch_add(1, Ordering::Relaxed);
            return Err(EIO);
        }
        Ok(())
    }

    // [@ANCHOR: radioctld_service_key]
    /// Keys the transmitter for `client` if the gate is open and no other client owns it. The
    /// claim is made before the rig is asked and given back if the rig refuses.
    async fn key(&self, client: ClientId) -> Result<Body, i32> {
        self.check_transmit()?;
        let claim = locked(&self.owner).try_key(client);
        if claim.is_err() {
            self.counters.ptt_conflicts.fetch_add(1, Ordering::Relaxed);
            return self.refuse(ERJCTED);
        }
        let outcome = self.set(Op::SetPtt(true)).await;
        if outcome.is_err() {
            locked(&self.owner).release(client);
        }
        outcome
    }

    // [@ANCHOR: radioctld_service_unkey]
    /// Unkeys the transmitter. Open to every client, gate open or not.
    async fn unkey(&self) -> Result<Body, i32> {
        let outcome = self.set(Op::SetPtt(false)).await;
        if outcome.is_ok() {
            locked(&self.owner).clear();
        }
        outcome
    }

    // [@ANCHOR: radioctld_service_send_morse]
    /// Sends Morse text unless the gate is closed or another client owns the transmitter. Morse
    /// puts RF on the air, so the sender claims the transmitter like a key does: the claim is
    /// given back if the rig refuses, and a sender that disconnects unkeys the rig.
    async fn send_morse(&self, client: ClientId, text: &str) -> Result<Body, i32> {
        self.check_transmit()?;
        let claim = locked(&self.owner).try_key(client);
        if claim.is_err() {
            self.counters.ptt_conflicts.fetch_add(1, Ordering::Relaxed);
            return self.refuse(ERJCTED);
        }
        let outcome = self.set(Op::SendCw(text.to_string())).await;
        if outcome.is_err() {
            locked(&self.owner).release(client);
        }
        outcome
    }

    // [@ANCHOR: radioctld_service_split_vfo]
    /// `s`: whether split is on and which VFO transmits.
    async fn split_vfo(&self) -> Result<Body, i32> {
        let on = if self.description.capabilities.has(Capability::Split) {
            matches!(
                self.read(Field::Split, Op::GetSplit).await?,
                Observation::Split(true)
            )
        } else {
            false
        };
        let tx = if on { Vfo::B } else { Vfo::A };
        Ok(Body::Fields(vec![
            ("Split", u8::from(on).to_string()),
            ("TX VFO", vfo_name(tx).to_string()),
        ]))
    }

    // [@ANCHOR: radioctld_service_set_split_vfo]
    /// `S`: split on means receive on VFO A and transmit on VFO B.
    async fn set_split_vfo(&self, on: bool, tx: VfoSel) -> Result<Body, i32> {
        let has_split = self.description.capabilities.has(Capability::Split);
        if on {
            if !matches!(tx, VfoSel::B) || !has_split {
                return Err(ENAVAIL);
            }
            return self.set(Op::SetSplit(true)).await;
        }
        if has_split {
            self.set(Op::SetSplit(false)).await
        } else {
            Ok(Body::Done)
        }
    }

    // [@ANCHOR: radioctld_service_rit_offset]
    /// `j` and `z`: the offset in hertz while the function is on, else 0.
    async fn rit_offset(&self, xit: bool) -> Result<Body, i32> {
        let (field, op) = if xit {
            (Field::Xit, Op::GetXit)
        } else {
            (Field::Rit, Op::GetRit)
        };
        let on = matches!(
            self.read(field, op).await?,
            Observation::Rit(true) | Observation::Xit(true)
        );
        let offset = if on {
            match self.read(Field::RitOffset, Op::GetRitOffset).await? {
                Observation::RitOffset(hz) => hz,
                _ => return Err(EIO),
            }
        } else {
            0
        };
        let label = if xit { "XIT" } else { "RIT" };
        Ok(Body::Fields(vec![(label, offset.to_string())]))
    }

    // [@ANCHOR: radioctld_service_set_rit_offset]
    /// `J` and `Z`: 0 switches the function off; any other offset sets it and switches on.
    async fn set_rit_offset(&self, xit: bool, hz: i64) -> Result<Body, i32> {
        let switch = |on: bool| if xit { Op::SetXit(on) } else { Op::SetRit(on) };
        if hz == 0 {
            return self.set(switch(false)).await;
        }
        let offset = i16::try_from(hz)
            .ok()
            .filter(|v| (-9999..=9999).contains(v))
            .ok_or(EINVAL)?;
        self.exec(Op::SetRitOffset(offset)).await?;
        self.set(switch(true)).await
    }

    // [@ANCHOR: radioctld_service_served_level]
    /// The level info if this rig serves the named level.
    fn served_level(&self, name: &str) -> Result<LevelInfo, i32> {
        match level_lookup(name) {
            LevelLookup::Unknown => Err(EINVAL),
            LevelLookup::Unserved => Err(ENAVAIL),
            LevelLookup::Known(info) => served_levels(&self.description)
                .into_iter()
                .find(|l| l.bit == info.bit)
                .ok_or(ENAVAIL),
        }
    }

    // [@ANCHOR: radioctld_service_get_level]
    /// `l`: a level as the client sees it (a 0 to 1 fraction, or the rig's own integer).
    async fn get_level(&self, name: &str) -> Result<Body, i32> {
        let info = self.served_level(name)?;
        let text = match info.source {
            LevelSource::Radio(level) => {
                let Observation::Level(_, raw) =
                    self.read(Field::Level(level), Op::GetLevel(level)).await?
                else {
                    return Err(EIO);
                };
                if info.fraction {
                    let (min, max) = level_range(&self.description, level).ok_or(ENAVAIL)?;
                    let span = f64::from(max.saturating_sub(min).max(1));
                    fmt_float(f64::from(raw.saturating_sub(min)) / span)
                } else {
                    raw.to_string()
                }
            }
            LevelSource::Swr => {
                let Observation::Swr(tenths) = self.read(Field::Swr, Op::GetSwr).await? else {
                    return Err(EIO);
                };
                fmt_float(f64::from(tenths) / 10.0)
            }
            LevelSource::RawStr => {
                let Observation::Signal(raw) = self.read(Field::Signal, Op::GetSignal).await?
                else {
                    return Err(EIO);
                };
                raw.to_string()
            }
        };
        Ok(Body::Fields(vec![("", text)]))
    }

    // [@ANCHOR: radioctld_service_set_level]
    /// `L`: sets a numeric level; meters are read-only.
    async fn set_level(&self, name: &str, value: f64) -> Result<Body, i32> {
        let info = self.served_level(name)?;
        let LevelSource::Radio(level) = info.source else {
            return Err(ENAVAIL);
        };
        let (min, max) = level_range(&self.description, level).ok_or(ENAVAIL)?;
        let raw = if info.fraction {
            if !(0.0..=1.0).contains(&value) {
                return Err(EINVAL);
            }
            f64::from(min) + (value * f64::from(max.saturating_sub(min))).round()
        } else {
            value.round()
        };
        if raw < f64::from(min) || raw > f64::from(max) {
            return Err(EINVAL);
        }
        self.set(Op::SetLevel(level, raw as u16)).await
    }

    // [@ANCHOR: radioctld_service_get_func]
    /// `u`: RIT or XIT on or off.
    async fn get_func(&self, name: &str) -> Result<Body, i32> {
        let value = match func_lookup(name) {
            FuncLookup::Unknown => return Err(EINVAL),
            FuncLookup::Unserved => return Err(ENAVAIL),
            FuncLookup::Known(FuncKind::Rit) => self.read(Field::Rit, Op::GetRit).await?,
            FuncLookup::Known(FuncKind::Xit) => self.read(Field::Xit, Op::GetXit).await?,
        };
        let on = matches!(value, Observation::Rit(true) | Observation::Xit(true));
        Ok(Body::Fields(vec![("", u8::from(on).to_string())]))
    }

    // [@ANCHOR: radioctld_service_set_func]
    /// `U`: RIT or XIT on or off.
    async fn set_func(&self, name: &str, on: bool) -> Result<Body, i32> {
        match func_lookup(name) {
            FuncLookup::Unknown => Err(EINVAL),
            FuncLookup::Unserved => Err(ENAVAIL),
            FuncLookup::Known(FuncKind::Rit) => self.set(Op::SetRit(on)).await,
            FuncLookup::Known(FuncKind::Xit) => self.set(Op::SetXit(on)).await,
        }
    }

    // [@ANCHOR: radioctld_service_vfo_info]
    /// `\get_vfo_info`: frequency, mode, width and split of the selected VFO.
    async fn vfo_info(&self, vfo: Vfo) -> Result<Body, i32> {
        let freq = self.frequency(vfo).await?;
        let (mode, width) = self.mode_and_width().await?;
        let Body::Fields(split) = self.split_vfo().await? else {
            return Err(EIO);
        };
        Ok(Body::Fields(vec![
            ("Freq", freq.to_string()),
            ("Mode", mode_to_hamlib(mode).to_string()),
            ("Width", width.to_string()),
            (
                "Split",
                split.first().map_or("0".to_string(), |(_, v)| v.clone()),
            ),
            ("SatMode", "0".to_string()),
        ]))
    }

    // [@ANCHOR: radioctld_service_execute]
    /// Executes one command for `client`. The answer is the body of a success, or the `RPRT`
    /// code of a failure.
    pub async fn execute(
        &self,
        client: ClientId,
        command: &Command,
        vfo: Option<VfoSel>,
    ) -> Result<Body, i32> {
        self.counters.commands.fetch_add(1, Ordering::Relaxed);
        let selected = self.resolve_vfo(vfo)?;
        match command {
            Command::GetFreq => {
                let hz = self.frequency(selected).await?;
                Ok(Body::Fields(vec![("Frequency", hz.to_string())]))
            }
            Command::SetFreq(hz) => self.set(Op::SetFrequency(selected, Hz(*hz))).await,
            Command::GetMode => self.mode_fields("Mode", "Passband").await,
            Command::SetMode { mode, passband } => self.set_mode(*mode, *passband).await,
            Command::GetVfo => Ok(Body::Fields(vec![(
                "VFO",
                vfo_name(*locked(&self.current)).to_string(),
            )])),
            Command::SetVfo(target) => {
                let vfo = self.resolve_vfo(Some(*target))?;
                if vfo == Vfo::B && !self.description.capabilities.has(Capability::SecondVfo) {
                    return Err(ENAVAIL);
                }
                *locked(&self.current) = vfo;
                Ok(Body::Done)
            }
            Command::GetPtt => match self.read(Field::Ptt, Op::GetPtt).await? {
                Observation::Ptt(on) => Ok(Body::Fields(vec![("PTT", u8::from(on).to_string())])),
                _ => Err(EIO),
            },
            Command::SetPtt(0) => self.unkey().await,
            Command::SetPtt(_) => self.key(client).await,
            Command::GetSplitVfo => self.split_vfo().await,
            Command::SetSplitVfo { on, tx } => self.set_split_vfo(*on, *tx).await,
            Command::GetSplitFreq => {
                let hz = self.frequency(Vfo::B).await?;
                Ok(Body::Fields(vec![("TX Frequency", hz.to_string())]))
            }
            Command::SetSplitFreq(hz) => self.set(Op::SetFrequency(Vfo::B, Hz(*hz))).await,
            Command::GetSplitMode => self.mode_fields("TX Mode", "TX Passband").await,
            Command::SetSplitMode { mode, .. } => {
                self.require_current_mode(*mode).await?;
                Ok(Body::Done)
            }
            Command::GetSplitFreqMode => {
                let hz = self.frequency(Vfo::B).await?;
                let Body::Fields(mut fields) = self.mode_fields("TX Mode", "TX Passband").await?
                else {
                    return Err(EIO);
                };
                fields.insert(0, ("TX Frequency", hz.to_string()));
                Ok(Body::Fields(fields))
            }
            Command::SetSplitFreqMode { freq, mode, .. } => {
                self.require_current_mode(*mode).await?;
                self.set(Op::SetFrequency(Vfo::B, Hz(*freq))).await
            }
            Command::GetRit => self.rit_offset(false).await,
            Command::SetRit(hz) => self.set_rit_offset(false, *hz).await,
            Command::GetXit => self.rit_offset(true).await,
            Command::SetXit(hz) => self.set_rit_offset(true, *hz).await,
            Command::GetLevel(name) => self.get_level(name).await,
            Command::SetLevel(name, value) => self.set_level(name, *value).await,
            Command::GetFunc(name) => self.get_func(name).await,
            Command::SetFunc(name, on) => self.set_func(name, *on).await,
            Command::DumpCaps => Ok(Body::Text {
                text: dump_caps(&self.caps_view()),
                rprt: true,
            }),
            Command::DumpState => Ok(Body::Text {
                text: dump_state(&self.caps_view()),
                rprt: false,
            }),
            Command::ChkVfo => Ok(Body::Fields(vec![(
                "ChkVFO",
                u8::from(self.config.vfo_mode).to_string(),
            )])),
            Command::GetPowerStat => Ok(Body::Fields(vec![(
                "Power Status",
                u8::from(self.rig.link_up()).to_string(),
            )])),
            Command::GetLockMode => Ok(Body::Fields(vec![("Locked", "0".to_string())])),
            Command::SetLockMode(false) => Ok(Body::Done),
            Command::SetLockMode(true) => Err(ENAVAIL),
            Command::GetInfo => Ok(Body::Fields(vec![("Info", self.description.name.clone())])),
            Command::GetVfoInfo => self.vfo_info(selected).await,
            Command::GetVfoList => {
                let list = if self.description.capabilities.has(Capability::SecondVfo) {
                    "VFOA VFOB"
                } else {
                    "VFOA"
                };
                Ok(Body::Fields(vec![("VFOs", list.to_string())]))
            }
            Command::SendMorse(text) => self.send_morse(client, text).await,
            Command::SetPowerStat(_) | Command::StopMorse | Command::SendCmd(_) => Err(ENAVAIL),
            Command::Quit => Ok(Body::Close),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: radioctld_service_fmt_float]
    #[test]
    fn floats_print_without_trailing_zeros() {
        assert_eq!(fmt_float(0.5), "0.5");
        assert_eq!(fmt_float(1.0), "1");
        assert_eq!(fmt_float(0.0), "0");
        assert_eq!(fmt_float(1.0 / 255.0 * 30.0), "0.117647");
        assert_eq!(fmt_float(2.5), "2.5");
    }
}
