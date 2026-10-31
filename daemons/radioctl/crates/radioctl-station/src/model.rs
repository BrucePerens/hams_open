// SPDX-License-Identifier: AGPL-3.0-or-later
//! The station model: typed switch channels, devices with a power source and dependencies, and
//! station profiles. Everything here is data with validation; nothing touches hardware.

use crate::error::{StationError, StationErrorKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The name of a switch channel.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelId(pub String);

/// The name of a device (a radio, an amplifier and so on).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceId(pub String);

/// The value of a channel: a power or output channel is `Off` or `On`; a selector is `Off`
/// (every way open) or `Port(n)` with n from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Value {
    /// Off, open, released.
    Off,
    /// On, closed, asserted.
    On,
    /// Selector way n (from 1).
    Port(u32),
}

/// The typed kind of a switch channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChannelKind {
    /// On/off power. `inrush_stagger_ms` is the wait before the next power-on in a sequence;
    /// `min_off_ms` the shortest time it stays off before it may come on again.
    Power {
        /// Contact or load rating, informational.
        #[serde(default)]
        rating_watts: Option<u32>,
        /// Stagger after switching on.
        #[serde(default)]
        inrush_stagger_ms: u64,
        /// Minimum off time.
        #[serde(default)]
        min_off_ms: u64,
    },
    /// An n-way exclusive selector, break-before-make.
    Selector {
        /// Number of ways.
        ways: u32,
        /// Pause between opening the old way and closing the new one.
        #[serde(default)]
        settle_ms: u64,
    },
    /// A pulse of fixed width (a power button, a grounded line).
    Momentary {
        /// Pulse width.
        pulse_ms: u64,
    },
    /// A general digital output such as a band-decoder line.
    Output,
    /// A status sensor, debounced.
    Input {
        /// How long a new level must hold before it is believed.
        #[serde(default)]
        debounce_ms: u64,
    },
    /// A radio or amplifier key line, timed in firmware; defined so a map can name it, but the
    /// switchboard never drives it (phase 3, the FX2 firmware).
    Ptt {
        /// Longest key-down.
        max_on_ms: u64,
        /// Shortest time between key-downs.
        #[serde(default)]
        min_off_ms: u64,
    },
}

/// One switch channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelDef {
    /// Its name.
    pub id: ChannelId,
    /// Its type.
    #[serde(flatten)]
    pub kind: ChannelKind,
    /// The name of the driver bank that carries it.
    pub driver: String,
    /// The first line on that bank (a selector uses `ways` consecutive lines).
    pub line: u32,
    /// What the channel does on host loss, at boot and on a release. Required for power and ptt.
    #[serde(default)]
    pub safe: Option<Value>,
    /// Free text.
    #[serde(default)]
    pub description: String,
}

impl ChannelDef {
    // [@ANCHOR: radioctl_station_channel_lines]
    /// The driver lines this channel uses.
    pub fn lines(&self) -> Vec<u32> {
        match self.kind {
            ChannelKind::Selector { ways, .. } => {
                (self.line..self.line.saturating_add(ways)).collect()
            }
            _ => vec![self.line],
        }
    }

    // [@ANCHOR: radioctl_station_channel_validate]
    /// Checks the channel on its own: a power or ptt channel needs a defined safe state, a safe
    /// state must fit the kind, times and sizes are sane.
    pub fn validate(&self) -> Result<(), StationError> {
        let bad = |m: String| {
            Err(StationError::new(
                StationErrorKind::Config,
                format!("channel {}: {m}", self.id.0),
            ))
        };
        if self.id.0.is_empty() || self.driver.is_empty() {
            return bad("needs a name and a driver".into());
        }
        match (&self.kind, self.safe) {
            (ChannelKind::Power { .. }, None) | (ChannelKind::Ptt { .. }, None) => {
                return bad("a power or ptt channel needs a defined safe state".into())
            }
            (ChannelKind::Power { .. }, Some(Value::Port(_))) => {
                return bad("a power channel's safe state is on or off".into())
            }
            (ChannelKind::Ptt { .. }, Some(Value::Port(_) | Value::On)) => {
                return bad("a ptt channel's safe state is off".into())
            }
            (ChannelKind::Selector { ways, .. }, Some(Value::Port(n))) if n == 0 || n > *ways => {
                return bad(format!("safe port {n} is not one of {ways} ways"))
            }
            (ChannelKind::Selector { .. }, Some(Value::On)) => {
                return bad("a selector's safe state is off or a port".into())
            }
            (ChannelKind::Momentary { .. } | ChannelKind::Input { .. }, Some(Value::Port(_))) => {
                return bad("a safe port needs a selector".into())
            }
            _ => {}
        }
        match self.kind {
            ChannelKind::Selector { ways, .. } if !(1..=64).contains(&ways) => {
                bad("a selector has 1 to 64 ways".into())
            }
            ChannelKind::Momentary { pulse_ms } if pulse_ms == 0 || pulse_ms > 10_000 => {
                bad("a momentary pulse is 1 to 10000 ms".into())
            }
            ChannelKind::Ptt { max_on_ms: 0, .. } => {
                bad("a ptt channel needs a maximum on-time".into())
            }
            _ => Ok(()),
        }
    }
}

/// What kind of equipment a device is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    /// A transceiver or receiver.
    Radio,
    /// A power amplifier.
    Amplifier,
    /// An antenna tuner.
    Tuner,
    /// A rotator.
    Rotator,
    /// A switch or relay box.
    Switch,
    /// The computer.
    Computer,
}

/// How a device is powered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum PowerSource {
    /// No switch: the device is always on.
    AlwaysOn,
    /// A power channel.
    Channel {
        /// The channel.
        channel: ChannelId,
    },
    /// A momentary line pulsed to power on (a power button); an optional second one to power off.
    Momentary {
        /// The pulse that powers on.
        on: ChannelId,
        /// The pulse that powers off, if the radio has one.
        #[serde(default)]
        off: Option<ChannelId>,
    },
}

/// One device in the station.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceDef {
    /// Its name.
    pub id: DeviceId,
    /// What it is.
    pub kind: DeviceKind,
    /// How it gets power.
    pub power: PowerSource,
    /// Devices that must be on before this one may be powered on, and are powered off after it.
    #[serde(default)]
    pub depends_on: Vec<DeviceId>,
    /// Selector channels that must have a way selected before this one may be powered on.
    #[serde(default)]
    pub needs_antenna: Vec<ChannelId>,
    /// Wait after power-on before the next step.
    #[serde(default)]
    pub settle_ms: u64,
    /// How long to wait for the device to answer (0: do not wait).
    #[serde(default)]
    pub ready_timeout_ms: u64,
    /// A wait before removing power in an orderly power-off (a tube amplifier cooling).
    #[serde(default)]
    pub off_delay_ms: u64,
}

/// One step of a profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ProfileStep {
    /// Power a device on (its dependencies must already be on).
    Power {
        /// The device.
        device: DeviceId,
    },
    /// Select a way on a selector channel.
    Select {
        /// The channel.
        channel: ChannelId,
        /// The way, from 1.
        port: u32,
    },
}

/// A named group, for example "HF station on".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileDef {
    /// Its name.
    pub name: String,
    /// The steps in power-on order; power-off runs the power steps in reverse.
    pub steps: Vec<ProfileStep>,
    /// The longest the whole sequence may take.
    #[serde(default = "default_profile_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_profile_timeout_ms() -> u64 {
    120_000
}

/// The whole station.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StationDef {
    /// Switch channels.
    #[serde(default)]
    pub channels: Vec<ChannelDef>,
    /// Devices.
    #[serde(default)]
    pub devices: Vec<DeviceDef>,
    /// Profiles.
    #[serde(default)]
    pub profiles: Vec<ProfileDef>,
}

impl StationDef {
    // [@ANCHOR: radioctl_station_def_from_toml]
    /// Parses and validates a station definition.
    pub fn from_toml(text: &str) -> Result<StationDef, StationError> {
        let def: StationDef = toml::from_str(text)
            .map_err(|e| StationError::new(StationErrorKind::Config, format!("station: {e}")))?;
        def.validate()?;
        Ok(def)
    }

    // [@ANCHOR: radioctl_station_def_channel]
    /// A channel by name.
    pub fn channel(&self, id: &ChannelId) -> Option<&ChannelDef> {
        self.channels.iter().find(|c| &c.id == id)
    }

    // [@ANCHOR: radioctl_station_def_device]
    /// A device by name.
    pub fn device(&self, id: &DeviceId) -> Option<&DeviceDef> {
        self.devices.iter().find(|d| &d.id == id)
    }

    // [@ANCHOR: radioctl_station_def_validate]
    /// Checks names, references, channel kinds, one device per power channel, and that the
    /// dependency graph has no cycle.
    pub fn validate(&self) -> Result<(), StationError> {
        let bad = |m: String| Err(StationError::new(StationErrorKind::Config, m));
        let mut seen = BTreeSet::new();
        for c in &self.channels {
            c.validate()?;
            if !seen.insert(&c.id) {
                return bad(format!("channel {} is defined twice", c.id.0));
            }
        }
        let mut lines = BTreeSet::new();
        for c in &self.channels {
            for l in c.lines() {
                if !lines.insert((c.driver.clone(), l)) {
                    return bad(format!(
                        "driver {} line {l} is used by two channels",
                        c.driver
                    ));
                }
            }
        }
        let mut devs = BTreeSet::new();
        let mut powered = BTreeSet::new();
        for d in &self.devices {
            if !devs.insert(&d.id) {
                return bad(format!("device {} is defined twice", d.id.0));
            }
            let sources: Vec<(&ChannelId, bool)> = match &d.power {
                PowerSource::AlwaysOn => vec![],
                PowerSource::Channel { channel } => vec![(channel, false)],
                PowerSource::Momentary { on, off } => {
                    let mut v = vec![(on, true)];
                    v.extend(off.iter().map(|o| (o, true)));
                    v
                }
            };
            for (id, momentary) in sources {
                let Some(c) = self.channel(id) else {
                    return bad(format!("device {} uses unknown channel {}", d.id.0, id.0));
                };
                let ok = matches!(
                    (&c.kind, momentary),
                    (ChannelKind::Momentary { .. }, true) | (ChannelKind::Power { .. }, false)
                );
                if !ok {
                    return bad(format!(
                        "device {}: channel {} has the wrong type for a power source",
                        d.id.0, id.0
                    ));
                }
                if !powered.insert(id) {
                    return bad(format!("channel {} powers two devices", id.0));
                }
            }
            for a in &d.needs_antenna {
                match self.channel(a).map(|c| &c.kind) {
                    Some(ChannelKind::Selector { .. }) => {}
                    _ => {
                        return bad(format!(
                            "device {} needs {} which is not a selector",
                            d.id.0, a.0
                        ))
                    }
                }
            }
        }
        for d in &self.devices {
            for dep in &d.depends_on {
                if self.device(dep).is_none() {
                    return bad(format!("device {} depends on unknown {}", d.id.0, dep.0));
                }
            }
        }
        self.power_on_order()?;
        let mut names = BTreeSet::new();
        for p in &self.profiles {
            if !names.insert(&p.name) {
                return bad(format!("profile {} is defined twice", p.name));
            }
            for s in &p.steps {
                match s {
                    ProfileStep::Power { device } if self.device(device).is_none() => {
                        return bad(format!("profile {} powers unknown {}", p.name, device.0))
                    }
                    ProfileStep::Select { channel, port } => {
                        match self.channel(channel).map(|c| &c.kind) {
                            Some(ChannelKind::Selector { ways, .. })
                                if (1..=*ways).contains(port) => {}
                            _ => {
                                return bad(format!(
                                    "profile {}: bad selection {} port {port}",
                                    p.name, channel.0
                                ))
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    // [@ANCHOR: radioctl_station_def_power_on_order]
    /// Every device in an order where each follows the devices it depends on (ties by name);
    /// an error on a cycle. Power-off order is the reverse.
    pub fn power_on_order(&self) -> Result<Vec<DeviceId>, StationError> {
        let mut deps: BTreeMap<&DeviceId, BTreeSet<&DeviceId>> = BTreeMap::new();
        for d in &self.devices {
            deps.insert(&d.id, d.depends_on.iter().collect());
        }
        let mut order = Vec::new();
        while !deps.is_empty() {
            let ready: Vec<&DeviceId> = deps
                .iter()
                .filter(|(_, v)| v.is_empty())
                .map(|(k, _)| *k)
                .collect();
            let Some(next) = ready.first().copied() else {
                return Err(StationError::new(
                    StationErrorKind::Config,
                    "the device dependencies form a cycle",
                ));
            };
            deps.remove(next);
            for v in deps.values_mut() {
                v.remove(next);
            }
            order.push(next.clone());
        }
        Ok(order)
    }

    // [@ANCHOR: radioctl_station_def_dependents]
    /// Every device that depends on `id`, directly or not, in power-off order (the most
    /// dependent first).
    pub fn dependents(&self, id: &DeviceId) -> Vec<DeviceId> {
        let order = self.power_on_order().unwrap_or_default();
        let mut set = BTreeSet::new();
        set.insert(id.clone());
        for d in &order {
            if self
                .device(d)
                .is_some_and(|x| x.depends_on.iter().any(|p| set.contains(p)))
            {
                set.insert(d.clone());
            }
        }
        order
            .into_iter()
            .rev()
            .filter(|d| d != id && set.contains(d))
            .collect()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const SAMPLE: &str = r#"
[[channels]]
id = "radio_pwr"
type = "power"
driver = "bank"
line = 0
safe = "off"
[[channels]]
id = "amp_pwr"
type = "power"
driver = "bank"
line = 1
safe = "off"
min_off_ms = 500
[[channels]]
id = "ant"
type = "selector"
ways = 3
settle_ms = 20
driver = "bank"
line = 2
[[channels]]
id = "btn"
type = "momentary"
pulse_ms = 300
driver = "bank"
line = 5
[[devices]]
id = "ic7300"
kind = "radio"
power = { source = "channel", channel = "radio_pwr" }
settle_ms = 10
ready_timeout_ms = 1000
[[devices]]
id = "amp"
kind = "amplifier"
power = { source = "channel", channel = "amp_pwr" }
depends_on = ["ic7300"]
needs_antenna = ["ant"]
off_delay_ms = 100
[[profiles]]
name = "hf_on"
steps = [
  { action = "power", device = "ic7300" },
  { action = "select", channel = "ant", port = 2 },
  { action = "power", device = "amp" },
]
"#;

    // Tests [@ANCHOR: radioctl_station_def_from_toml]
    // Tests [@ANCHOR: radioctl_station_def_channel]
    // Tests [@ANCHOR: radioctl_station_def_device]
    // Tests [@ANCHOR: radioctl_station_def_power_on_order]
    // Tests [@ANCHOR: radioctl_station_def_dependents]
    // Tests [@ANCHOR: radioctl_station_channel_lines]
    #[test]
    fn the_sample_station_parses_and_orders() {
        let def = StationDef::from_toml(SAMPLE).unwrap();
        assert_eq!(
            def.channel(&ChannelId("ant".into())).unwrap().lines(),
            vec![2, 3, 4]
        );
        assert!(def.device(&DeviceId("amp".into())).is_some());
        let order: Vec<String> = def
            .power_on_order()
            .unwrap()
            .into_iter()
            .map(|d| d.0)
            .collect();
        assert_eq!(order, vec!["ic7300", "amp"]);
        assert_eq!(
            def.dependents(&DeviceId("ic7300".into())),
            vec![DeviceId("amp".into())]
        );
    }

    // Tests [@ANCHOR: radioctl_station_def_validate]
    // Tests [@ANCHOR: radioctl_station_channel_validate]
    #[test]
    fn invalid_stations_are_refused() {
        let cases = [
            (SAMPLE.replacen("safe = \"off\"\n", "", 1), "safe state"),
            (SAMPLE.replace("line = 1", "line = 0"), "two channels"),
            (
                SAMPLE.replace("depends_on = [\"ic7300\"]", "depends_on = [\"amp\"]"),
                "cycle",
            ),
            (
                SAMPLE.replace("depends_on = [\"ic7300\"]", "depends_on = [\"nope\"]"),
                "unknown",
            ),
            (
                SAMPLE.replace("needs_antenna = [\"ant\"]", "needs_antenna = [\"amp_pwr\"]"),
                "selector",
            ),
            (SAMPLE.replace("port = 2", "port = 9"), "bad selection"),
            (SAMPLE.replace("pulse_ms = 300", "pulse_ms = 0"), "pulse"),
            (
                SAMPLE.replacen("safe = \"off\"", "safe = { port = 1 }", 1),
                "on or off",
            ),
            (
                SAMPLE.replace("channel = \"amp_pwr\"", "channel = \"radio_pwr\""),
                "powers two",
            ),
        ];
        for (text, want) in cases {
            let err = StationDef::from_toml(&text).unwrap_err();
            assert_eq!(err.kind, StationErrorKind::Config);
            assert!(err.detail.contains(want), "{want}: {}", err.detail);
        }
        assert!(StationDef::from_toml("not toml [").is_err());
    }
}
