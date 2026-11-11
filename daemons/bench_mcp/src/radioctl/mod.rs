// SPDX-License-Identifier: AGPL-3.0-or-later
//! radioctl rigs for bench_mcp: receivers and (for rigs the owner has attested into a dummy load) transmitters, driven through the
//! `radioctld` daemon's TCP text protocol (the line protocol of `rigctld`, which radioctld speaks). The
//! daemon is a separate program, started by this module (bound to 127.0.0.1) or already running; the rig list comes from the rig
//! description files (`rigs/*.toml`) of radioctl.
//!
//! One configured instrument is one rig. The entry in the bench config (`BENCH_MCP_CONFIG`) is either
//! `{"driver": "radioctl", "tier": "read_config", "rig": "elecraft-kx3"}` (a radioctl rig description; with no `device` the rig is a virtual
//! one; this module starts and stops its radioctld) or
//! `{"driver": "radioctl", "tier": "read_only", "radioctld_port": 4532}` (a radioctld that is already running on this machine).
//! The keys are listed at `parse_config`; the safety model is in the manual page `instruments/radioctl/limits`.

#[cfg(test)]
pub mod testkit;

pub mod attest;
pub mod audio_in;
pub mod control;
pub mod detect;
pub mod diag;
pub mod guard;
pub mod load;
pub mod loadtools;
pub mod models;
pub mod proto;
pub mod rigd;
pub mod rxcheck;
pub mod stations;
pub mod status;
pub mod tx;
pub mod txtools;
pub mod watch;

use crate::audio::Process;
use crate::config::{Instrument, Kind, Tier};
use proto::Client;
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// A managed radioctld: this server starts it for the rig.
#[derive(Clone, Debug, PartialEq)]
pub struct Managed {
    /// The rig description id (the file `<id>.toml` in the rigs directory).
    pub rig: String,
    /// A serial device under /dev; none means a virtual rig made from the description.
    pub device: Option<String>,
    pub speed: Option<u32>,
    pub port: Option<u16>,
    /// The directory of the rig description files; none means `RADIOCTL_RIGS` from the environment.
    pub rigs_dir: Option<PathBuf>,
    /// The rig's full-scale RF output in watts (from its manual): the daemon scales the power setting by it. Required with a transmit attestation.
    pub full_power_w: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Managed(Managed),
    Existing { host: String, port: u16 },
}

/// The `radioctl` settings of an instrument entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub source: Source,
    /// How long one radioctld command may take before the call fails, seconds.
    pub reply_timeout_s: u64,
    /// The owner's receive range limits in Hz, applied in addition to the ranges the rig reports. Empty: the rig's own.
    pub rx_ranges_hz: Vec<(f64, f64)>,
    pub managed_dir: Option<PathBuf>,
    /// The rig's audio: the name of an `audio` instrument in the bench config and the alias (or node name fragment) of its input.
    pub audio: Option<AudioRef>,
    /// The owner's transmit attestation; without it no tool transmits.
    pub transmit: Option<attest::Transmit>,
}

/// Where the rig's receive audio comes from, by the names the sound tools use.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioRef {
    pub instrument: String,
    pub input: String,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            source: Source::Existing {
                host: "127.0.0.1".into(),
                port: 4532,
            },
            reply_timeout_s: 3,
            rx_ranges_hz: vec![],
            managed_dir: None,
            audio: None,
            transmit: None,
        }
    }
}

const BAUDS: &[u32] = &[
    1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600,
];

/// A string safe to put in a file name.
pub fn safe_name(s: &str) -> String {
    // [@ANCHOR: radioctl_safe_name]
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

fn token_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 120
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_./:-".contains(&b))
}

/// Reads the radioctl settings of an instrument entry: `rig` (the id of a radioctl rig description, such as `elecraft-kx3`; the module starts a
/// radioctld for it) with `device` (a /dev path of a serial rig; without it the rig is virtual) and `speed` (serial baud rate, required with
/// `device`), `rigs_dir` (an absolute path to the description files; default the `RADIOCTL_RIGS` environment variable), `full_power_w` (the rig's full-scale RF watts; required with `transmit`) and `listen_port`; OR
/// `radioctld_port` (with `radioctld_host`, loopback unless `allow_remote_radioctld` is true) for a radioctld that already
/// runs. Also `reply_timeout_s` (1 to 30, default 3), `rx_ranges_hz` (a list of [low, high]), `managed_dir`, `audio` ({instrument, input}: the sound input that carries the rig's receive audio, by the
/// names of the audio instrument) and `transmit` (the owner's dummy-load attestation and limits, see `attest.rs`).
pub fn parse_config(c: &Value) -> Result<Config, String> {
    // [@ANCHOR: radioctl_config]
    let mut cfg = Config::default();
    for old in ["model", "set_conf"] {
        if c.get(old).is_some() {
            return Err(format!("{old} was a Hamlib setting and is gone: name the radioctl rig description in rig (bench_radioctl_models lists them)"));
        }
    }
    let rig = c.get("rig");
    let port = c.get("radioctld_port");
    cfg.source = match (rig, port) {
        (Some(_), Some(_)) => return Err("give either rig (this server starts radioctld) or radioctld_port (one already runs), not both".into()),
        (None, None) => return Err("a radioctl rig needs rig (a radioctl rig description id) or radioctld_port".into()),
        (Some(r), None) => {
            let rig = r.as_str().filter(|r| !r.is_empty() && r.len() <= 80 && r.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))).ok_or("rig must be a rig description id such as elecraft-kx3 (letters, digits, - and _; bench_radioctl_models lists them)")?.to_string();
            let device = match c.get("device") {
                None | Some(Value::Null) => None,
                Some(d) => {
                    let d = d.as_str().filter(|d| token_ok(d)).ok_or("device must be a path such as /dev/serial/by-id/usb-... (letters, digits and _ . / : -)")?;
                    if !d.starts_with("/dev/") || d.contains("..") {
                        return Err(format!("device {d:?} must be a path under /dev without .."));
                    }
                    crate::serial_guard::check_reserved_open(d)?;
                    Some(d.to_string())
                }
            };
            let speed = match c.get("speed") {
                None | Some(Value::Null) => None,
                Some(s) => Some(s.as_u64().and_then(|s| u32::try_from(s).ok()).filter(|s| BAUDS.contains(s)).ok_or_else(|| format!("speed must be one of {BAUDS:?}"))?),
            };
            if device.is_some() && speed.is_none() {
                return Err("a serial rig needs speed (the baud rate)".into());
            }
            if device.is_none() && speed.is_some() {
                return Err("speed is for a serial rig: give device too, or leave both out for a virtual rig".into());
            }
            let rigs_dir = match c.get("rigs_dir") {
                None | Some(Value::Null) => None,
                Some(d) => Some(PathBuf::from(d.as_str().filter(|d| d.starts_with('/') && !d.contains("..")).ok_or("rigs_dir must be an absolute path without ..")?)),
            };
            let listen = match c.get("listen_port") {
                None | Some(Value::Null) => None,
                Some(p) => Some(p.as_u64().and_then(|p| u16::try_from(p).ok()).filter(|p| *p >= 1024).ok_or("listen_port must be 1024 to 65535")?),
            };
            let full_power_w = match c.get("full_power_w") {
                None | Some(Value::Null) => None,
                Some(p) => Some(p.as_f64().filter(|p| p.is_finite() && (0.1..=2000.0).contains(p)).ok_or("full_power_w must be a number of watts from 0.1 to 2000: the rig's full-scale RF output from its manual")?),
            };
            Source::Managed(Managed { rig, device, speed, port: listen, rigs_dir, full_power_w })
        }
        (None, Some(p)) => {
            let port = p.as_u64().and_then(|p| u16::try_from(p).ok()).filter(|p| *p > 0).ok_or("radioctld_port must be 1 to 65535")?;
            let host = c.get("radioctld_host").map(|h| h.as_str().ok_or("radioctld_host must be a string")).transpose()?.unwrap_or("127.0.0.1");
            let remote_ok = c.get("allow_remote_radioctld").and_then(|v| v.as_bool()).unwrap_or(false);
            if !is_loopback(host) && !remote_ok {
                return Err(format!("radioctld_host {host:?} is not this machine. radioctld has no authentication and must not be shared on a network; set allow_remote_radioctld true only if the owner accepts that"));
            }
            if !token_ok(host) {
                return Err("radioctld_host has characters that are not allowed".into());
            }
            Source::Existing { host: host.to_string(), port }
        }
    };
    if let Some(t) = c.get("reply_timeout_s") {
        cfg.reply_timeout_s = t
            .as_u64()
            .filter(|t| (1..=30).contains(t))
            .ok_or("reply_timeout_s must be a whole number from 1 to 30")?;
    }
    if let Some(r) = c.get("rx_ranges_hz") {
        for pair in r
            .as_array()
            .ok_or("rx_ranges_hz must be a list of [low, high] pairs in Hz")?
        {
            let p = pair
                .as_array()
                .filter(|p| p.len() == 2)
                .ok_or("each rx_ranges_hz entry is [low, high]")?;
            let (lo, hi) = (
                p[0].as_f64().unwrap_or(f64::NAN),
                p[1].as_f64().unwrap_or(f64::NAN),
            );
            if !(lo.is_finite() && hi.is_finite() && lo >= 0.0 && hi > lo) {
                return Err("rx_ranges_hz entries need finite 0 <= low < high".into());
            }
            cfg.rx_ranges_hz.push((lo, hi));
        }
    }
    if let Some(d) = c.get("managed_dir") {
        cfg.managed_dir = Some(PathBuf::from(
            d.as_str()
                .filter(|d| d.starts_with('/') && !d.contains(".."))
                .ok_or("managed_dir must be an absolute path without ..")?,
        ));
    }
    if let Some(a) = c.get("audio") {
        let part = |k: &str| {
            a.get(k)
                .and_then(|v| v.as_str())
                .filter(|v| !v.trim().is_empty())
                .map(String::from)
        };
        cfg.audio = Some(AudioRef {
            instrument: part("instrument").ok_or("audio needs instrument: the name of an audio instrument in the bench config")?,
            input: part("input").ok_or("audio needs input: the alias or name of the sound input that carries the rig's receive audio")?,
        });
    }
    if let Some(t) = c.get("transmit") {
        cfg.transmit = Some(attest::parse_transmit(t).map_err(|e| format!("transmit: {e}"))?);
    }
    if let (Source::Managed(m), Some(_)) = (&cfg.source, &cfg.transmit) {
        if m.full_power_w.is_none() {
            return Err("a managed rig with a transmit attestation needs full_power_w: the rig's full-scale RF output in watts from its manual. radioctld has no power conversion, and the tools turn the owner's power cap into a power setting with it".into());
        }
    }
    if cfg.transmit.as_ref().is_some_and(|t| t.operator_alert) && cfg.audio.is_none() {
        return Err("transmit.operator_alert needs audio: the alert is played through the audio instrument named there".into());
    }
    Ok(cfg)
}

pub struct Spec {
    pub name: &'static str,
    pub tier: Tier,
    pub desc: &'static str,
    pub props: &'static str,
    pub required: &'static [&'static str],
}

macro_rules! props {
    ($($rest:tt)*) => { concat!("{", $($rest)*, "}") };
}

/// Every tool of this module. A new tool is an entry here, an arm in `call`, a page under `manual/tools/` and a test.
pub const SPECS: &[Spec] = &[
    Spec {
        name: "bench_radioctl_models",
        tier: Tier::ReadOnly,
        desc: "RADIOCTL: the radio rigs radioctl has descriptions for (the `rigs/*.toml` files), filtered by `search` (words of the id, name or protocol family), or with `rig` (a rig description id) what that description declares: name, protocol family and capabilities. This is what the description says, not what one unit does. Needs no rig.",
        props: props!(r#""instrument":{"type":"string"},"search":{"type":"string","description":"words to find in the rig id, name or family"},"rig":{"type":"string","description":"a rig description id such as elecraft-kx3: describe it instead of listing"},"limit":{"type":"integer","description":"most rigs to list, 1 to 500, default 50"}"#),
        required: &[],
    },
    Spec {
        name: "bench_radioctl_rigs",
        tier: Tier::ReadOnly,
        desc: "RADIOCTL: the configured rigs and their state: tier, whether radioctld is managed by this server and running, whether the rig answers now, whether transmitting is configured. Starts nothing. With `instrument`, one rig.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_radioctl_status",
        tier: Tier::ReadOnly,
        desc: "RADIOCTL receiver status in one call: frequency, mode and passband, VFO, split, RIT/XIT, PTT state, carrier detect, power state, rig info, signal strength (dB relative to S9 and S-units), AF/RF gain, squelch, AGC, attenuator, preamp, noise blanker/reduction and the other levels and functions the rig supports. A value the rig cannot give is listed under `unavailable` with the reason. Changes nothing.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_radioctl_get",
        tier: Tier::ReadOnly,
        desc: "RADIOCTL: one value, read now. `item` is frequency, mode, vfo, split, rit, xit, ptt, squelch, power, info, memory, channel (with `channel`), level or function (with `name`, such as AF, ATT, NB), or signal (the signal strength; `count` readings `interval_ms` apart give the mean and spread). Changes nothing.",
        props: props!(r#""instrument":{"type":"string"},"item":{"type":"string","description":"frequency, mode, vfo, split, rit, xit, ptt, squelch, power, info, memory, channel, level, function or signal"},"name":{"type":"string","description":"level or function name for item level or function, such as STRENGTH, AF, ATT, NB"},"channel":{"type":"integer","description":"memory channel number for item channel"},"count":{"type":"integer","description":"readings for item signal, 1 to 50, default 1"},"interval_ms":{"type":"integer","description":"milliseconds between readings, 0 to 2000, default 100"}"#),
        required: &["item"],
    },
    Spec {
        name: "bench_radioctl_daemon",
        tier: Tier::ReadConfig,
        desc: "RADIOCTL: the radioctld daemon of a rig this server manages: `status` (running, port, restarts, last exit), `start`, `stop` or `restart`. It listens on 127.0.0.1 only. A rig configured with an existing radioctld is not started or stopped by this server. Other tools start the daemon by themselves when needed.",
        props: props!(r#""instrument":{"type":"string"},"action":{"type":"string","description":"status, start, stop or restart"}"#),
        required: &["action"],
    },
    Spec {
        name: "bench_radioctl_set",
        tier: Tier::ReadConfig,
        desc: "RADIOCTL receiver control, each change read back: `frequency_hz` (inside the rig's receive range and the config's rx_ranges_hz), `mode` with `passband_hz`, `vfo`, `split` ({enabled, tx_vfo}), `rit_hz`, `xit_hz`, `levels` (AF, RF, SQL, IF, APF, NR, NB, PBT_IN, PBT_OUT, CWPITCH, NOTCHF, BAL, AGC, ATT, PREAMP) and `functions` (NB, NR, ANF, APF, AIP, MN, LOCK, MUTE, SQL...). Every argument is checked before anything is sent. Transmit-side settings (RF power, microphone gain, VOX, the tuner) are refused. Refuses while the rig reports PTT on. Never keys the transmitter.",
        props: props!(r#""instrument":{"type":"string"},"frequency_hz":{"type":"number","description":"receive frequency in Hz"},"mode":{"type":"string","description":"rig mode name such as USB, LSB, CW, AM, FM"},"passband_hz":{"type":"number","description":"passband width in Hz, 0 for the rig's default; needs mode"},"vfo":{"type":"string","description":"VFOA, VFOB, MEM, Main, Sub..."},"split":{"type":"object","description":"{enabled: bool, tx_vfo: VFOB}"},"rit_hz":{"type":"number","description":"-9999 to 9999"},"xit_hz":{"type":"number","description":"-9999 to 9999"},"levels":{"type":"object","description":"level name to value, for example {\"AF\":0.5,\"ATT\":20}; fractions are 0 to 1, ATT and PREAMP in dB"},"functions":{"type":"object","description":"function name to true or false, for example {\"NB\":true}"}"#),
        required: &[],
    },
    Spec {
        name: "bench_radioctl_memory",
        tier: Tier::ReadConfig,
        desc: "RADIOCTL: recall memory channel `channel` (selects the memory VFO and the channel) and return the frequency and mode the rig then shows. radioctld serves no memory channels today (RPRT -4), so with it this reports that and changes nothing. Warns when the channel holds a frequency outside the receive limits. Refuses while the rig reports PTT on.",
        props: props!(r#""instrument":{"type":"string"},"channel":{"type":"integer","description":"memory channel number, 0 to 99999"}"#),
        required: &["channel"],
    },
    Spec {
        name: "bench_radioctl_scan",
        tier: Tier::ReadConfig,
        desc: "RADIOCTL: step the receiver from `start_hz` to `stop_hz` in `step_hz` steps, dwelling `dwell_ms` (20 to 5000, default 200) at each, and list signal strength and squelch state per frequency with the strongest `top` ranked. At most 400 steps and 120 s in all; the range must be inside the receive limits. `stop_when_above_db` ends the scan at the first frequency stronger than that many dB relative to S9. The rig's frequency and mode are restored afterwards.",
        props: props!(r#""instrument":{"type":"string"},"start_hz":{"type":"number","description":"first frequency, Hz"},"stop_hz":{"type":"number","description":"last frequency, Hz"},"step_hz":{"type":"number","description":"step, Hz"},"dwell_ms":{"type":"number","description":"milliseconds at each frequency, 20 to 5000, default 200"},"mode":{"type":"string","description":"mode to scan in; default: the rig's current mode"},"top":{"type":"integer","description":"how many of the strongest to rank, 1 to 100, default 10"},"stop_when_above_db":{"type":"number","description":"stop at the first reading above this many dB relative to S9"}"#),
        required: &["start_hz", "stop_hz", "step_hz"],
    },
    Spec {
        name: "bench_radioctl_diagnose",
        tier: Tier::ReadOnly,
        desc: "RADIOCTL connection test for a rig that does not answer: the radioctld program, the model, the serial speed against what the backend allows, the device (exists, permissions, other processes holding it open), the DTR/RTS settings, whether the daemon runs and the rig answers, and the daemon's log. Never opens the serial port and never starts the daemon.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_radioctl_stations",
        tier: Tier::ReadOnly,
        desc: "RADIOCTL: the table of reference stations the load check listens for (WWV and WWVH on 2.5, 5, 10, 15 and 20 MHz, AM broadcast channel samples, NOAA weather radio, and the owner's own entries), with notes on where each status was checked, the date this bench last heard it, and whether that was recent. Never assume a station is on the air: silence is not a failure. With `frequency_hz`, the references that would be used now for that transmit frequency. Review the table.",
        props: props!(r#""instrument":{"type":"string"},"frequency_hz":{"type":"number","description":"show the references chosen for this transmit frequency now"},"include_retired":{"type":"boolean","description":"also list retired stations, default false"}"#),
        required: &[],
    },
    Spec {
        name: "bench_radioctl_station_set",
        tier: Tier::ReadConfig,
        desc: "RADIOCTL: edit the reference station table. `action` add (id, frequency_hz, mode, audio wwv|wwvh|carrier|voice|none, name, and a note saying where its status was checked and when), retire or restore (id, optional note), remove (an entry the owner added), or note (id, note). The shipped entries are retired, not removed. Stations go off the air; expect to retire entries.",
        props: props!(r#""instrument":{"type":"string"},"action":{"type":"string","description":"add, retire, restore, remove or note"},"id":{"type":"string","description":"station id: a-z, 0-9, - and _"},"name":{"type":"string","description":"a short display name, up to 80 characters"},"frequency_hz":{"type":"number","description":"carrier frequency in Hz, 10 kHz to 6 GHz"},"mode":{"type":"string","description":"rig mode name, default AM"},"audio":{"type":"string","description":"wwv, wwvh, carrier, voice or none"},"kind":{"type":"string","description":"local (default), time_station, am_band or weather"},"note":{"type":"string","description":"where the status was checked and when; at most 600 characters"}"#),
        required: &["action", "id"],
    },
    Spec {
        name: "bench_radioctl_rx_check",
        tier: Tier::ReadConfig,
        desc: "RADIOCTL receiver test with the rig's audio (the sound input named in the rig's `audio` setting): tunes to `frequency_hz` (default mode AM below 30 MHz, else FM), reads the S-meter five times and the squelch, records `seconds` of audio and reports the signal strength against the audio level, the audio noise floor, the strongest steady tone (with its offset from `expect_tone_hz`) and whether a time station's ticks or tones are in the audio. Without audio configured it reports the S-meter and squelch only. Restores the rig's frequency, mode and AF level. Never transmits.",
        props: props!(r#""instrument":{"type":"string"},"frequency_hz":{"type":"number","description":"receive frequency, Hz"},"mode":{"type":"string","description":"rig mode name"},"seconds":{"type":"number","description":"audio to record, 3 to 20, default 5"},"expect_tone_hz":{"type":"number","description":"the tone frequency expected in the audio, to report the offset, 100 to 5000"}"#),
        required: &["frequency_hz"],
    },
    Spec {
        name: "bench_radioctl_load_check",
        tier: Tier::ReadConfig,
        desc: "RADIOCTL dummy-load plausibility check, receive only, never keys: with the rig listening on its supposed dummy-load path, reads the S-meter, squelch and audio at the transmit frequency `frequency_hz` and at reference frequencies (time stations, AM band, weather radio from the station table) and compares them with the baseline recorded when the owner attested the load. Signals or noise above the baseline, a time station heard, an unsteady reading, an unreadable S-meter or a dead audio input REFUSE. EVIDENCE, NOT PROOF: an open or unplugged connection and an antenna on a dead band also hear nothing; the baseline is valid only for the same cabling and rig settings; the owner's attestation still gates transmitting. Only for rigs with a transmit attestation in the config.",
        props: props!(r#""instrument":{"type":"string"},"frequency_hz":{"type":"number","description":"the transmit frequency to check, inside one of the owner's transmit.frequency_ranges"}"#),
        required: &["frequency_hz"],
    },
    Spec {
        name: "bench_radioctl_load_baseline",
        tier: Tier::Full,
        desc: "RADIOCTL: record the load-check BASELINE for the owner's frequency range holding `frequency_hz`: the readings through the dummy load, stored with the attestation's name, date and cabling. `confirm_dummy_load` must be true and must come from the person at the bench: it states that the rig's antenna port is on the dummy load. Receive only; never keys. Refuses to record if a time station is heard or readings are unsteady. Record again after any change of cabling, load or rig settings. `dry_run` returns the plan and does nothing. Only for rigs with a transmit attestation in the config.",
        props: props!(r#""instrument":{"type":"string"},"frequency_hz":{"type":"number","description":"a frequency inside one of the owner's transmit.frequency_ranges"},"confirm_dummy_load":{"type":"boolean","description":"true: the person at the bench says the rig's antenna port is on the dummy load"},"dry_run":{"type":"boolean","description":"return the plan and do nothing"}"#),
        required: &["frequency_hz"],
    },
    Spec {
        name: "bench_radioctl_watch",
        tier: Tier::ReadOnly,
        desc: "RADIOCTL: watches the rig for `seconds` (1 to 120, default 10) by polling every `interval_ms` (100 to 5000, default 500) and returns the changes of frequency, mode, passband, VFO and PTT with their times, after the starting values. For someone turning the dial or software retuning the rig. A change that reverts between two polls is not seen. At most 200 events. Changes nothing.",
        props: props!(r#""instrument":{"type":"string"},"seconds":{"type":"number","description":"how long to watch, 1 to 120, default 10"},"interval_ms":{"type":"number","description":"polling interval, 100 to 5000, default 500"}"#),
        required: &[],
    },
    Spec {
        name: "bench_radioctl_safe_state",
        tier: Tier::ReadConfig,
        desc: "RADIOCTL: puts the rig in its safe state: PTT off, read back, and (unless `lower_power` is false) the power setting set to the rig's minimum; says what the power setting was. Safe at any time and does not wait for any other call, so it also ends a keyed interval that is running. Use it first when anything went wrong. If the rig will not confirm PTT off, the answer says what to do by hand.",
        props: props!(r#""instrument":{"type":"string"},"lower_power":{"type":"boolean","description":"also set the power setting to the rig's minimum, default true"}"#),
        required: &[],
    },
    Spec {
        name: "bench_radioctl_tx_key_test",
        tier: Tier::Full,
        desc: "RADIOCTL TRANSMIT TEST INTO A DUMMY LOAD, DESTROYS NOTHING BUT RADIATES IF THE LOAD IS NOT THERE: keys the rig on `frequency_hz` in `mode` at `power_w` for `seconds`, reading power, SWR, ALC and temperature meters every `sample_every_ms`, then releases PTT and reads it back. Only for a rig whose config has the owner's dummy-load attestation (transmit block), and only inside the owner's frequency ranges, modes, power cap and time cap. Needs `confirm_dummy_load: true` from the person at the bench and `reason` (who and why, logged). Before keying: the load plausibility check must say consistent with a load (a baseline must exist), the operator alert if configured, then a low-power probe key that reads the SWR (an open or shorted connection receives nothing, so only the SWR shows it). A separate watchdog process un-keys the rig on timeout, error, crash or exit. `dry_run` returns the plan and sends nothing. THE SOFTWARE CANNOT KNOW WHAT IS CONNECTED: the attestation is the control; the checks are evidence, not proof. Licence compliance is the owner's responsibility.",
        props: props!(r#""instrument":{"type":"string"},"frequency_hz":{"type":"number","description":"transmit frequency in Hz, inside the owner's transmit.frequency_ranges"},"mode":{"type":"string","description":"a mode in transmit.allowed_modes, default the first"},"power_w":{"type":"number","description":"power in watts, at most transmit.max_power_w"},"seconds":{"type":"number","description":"keyed duration in seconds, at most transmit.max_seconds and 60"},"sample_every_ms":{"type":"number","description":"meter sampling interval, 100 to 2000, default 250"},"reason":{"type":"string","description":"who is asking and why, up to 120 characters; written to the keyed-interval log"},"confirm_dummy_load":{"type":"boolean","description":"true: the person at the bench says the rig's antenna port is on the dummy load"},"dry_run":{"type":"boolean","description":"return the plan and send nothing"}"#),
        required: &["frequency_hz", "power_w", "seconds"],
    },
    Spec {
        name: "bench_radioctl_tx_power_sweep",
        tier: Tier::Full,
        desc: "RADIOCTL TRANSMIT SWEEP INTO A DUMMY LOAD: like bench_radioctl_tx_key_test, but `powers_w` lists 1 to 8 rising power levels and each is a separate keyed interval of `dwell_s` seconds with its own watchdog, its meters read, stopping at the first interval that is aborted (SWR over the limit, a power meter far over the cap). The total keyed time is at most 60 s. All the conditions of the key test apply: attestation, owner's ranges, caps, `confirm_dummy_load`, `reason`, the load plausibility check, the low-power probe. `dry_run` returns the plan and sends nothing. THE SOFTWARE CANNOT KNOW WHAT IS CONNECTED; licence compliance is the owner's.",
        props: props!(r#""instrument":{"type":"string"},"frequency_hz":{"type":"number","description":"transmit frequency in Hz, inside the owner's transmit.frequency_ranges"},"mode":{"type":"string","description":"a mode in transmit.allowed_modes, default the first"},"powers_w":{"type":"array","items":{"type":"number"},"description":"rising power levels in watts, 1 to 8, each at most transmit.max_power_w"},"dwell_s":{"type":"number","description":"seconds keyed at each level, 0.1 to transmit.max_seconds, default 1"},"sample_every_ms":{"type":"number","description":"meter sampling interval, 100 to 2000, default 250"},"reason":{"type":"string","description":"who is asking and why, up to 120 characters"},"confirm_dummy_load":{"type":"boolean","description":"true: the person at the bench says the rig's antenna port is on the dummy load"},"dry_run":{"type":"boolean","description":"return the plan and send nothing"}"#),
        required: &["frequency_hz", "powers_w"],
    },
];

/// Tools that exist only for a rig whose config carries a transmit attestation.
const NEEDS_ATTESTATION: &[&str] = &[
    "bench_radioctl_load_check",
    "bench_radioctl_load_baseline",
    "bench_radioctl_tx_key_test",
    "bench_radioctl_tx_power_sweep",
];

fn instrument<'a>(insts: &'a [Instrument], args: &Value) -> Result<&'a Instrument, String> {
    // [@ANCHOR: radioctl_instrument]
    let ours: Vec<&Instrument> = insts.iter().filter(|i| i.kind == Kind::Radioctl).collect();
    if let Some(n) = args.get("instrument").and_then(|n| n.as_str()) {
        return ours
            .iter()
            .copied()
            .find(|i| i.name == n)
            .ok_or_else(|| format!("no radioctl instrument named {n:?}"));
    }
    match ours.as_slice() {
        [one] => Ok(one),
        [] => Err("no radioctl instrument is configured".into()),
        many => Err(format!(
            "name the rig: {}",
            many.iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The tools to list: an entry of `SPECS` appears only when a radioctl rig is configured at or above its tier.
pub fn tool_defs(insts: &[Instrument]) -> Vec<Tool> {
    SPECS
        .iter()
        .filter(|s| {
            insts.iter().any(|i| {
                i.kind == Kind::Radioctl
                    && i.tier >= s.tier
                    && (i.radioctl.transmit.is_some() || !NEEDS_ATTESTATION.contains(&s.name))
            })
        })
        .map(|s| {
            let schema = json!({"type": "object", "properties": serde_json::from_str::<Value>(s.props).unwrap(), "required": s.required});
            let Value::Object(schema) = schema else { unreachable!() };
            Tool::new(s.name, format!("[{:?}] {}", s.tier, s.desc), Arc::new(schema))
        })
        .collect()
}

/// A connection to the rig's radioctld, starting the managed daemon if it is not running.
pub fn open(inst: &Instrument) -> Result<Client, String> {
    // [@ANCHOR: radioctl_open]
    let (host, port) = rigd::endpoint(&inst.name, &inst.radioctl)?;
    if inst.radioctl.transmit.is_some() {
        // The first contact with a rig that can be keyed: make sure a crash did not leave it transmitting.
        tx::startup_check(&inst.name, &rigd::dir(&inst.radioctl), &host, port);
    }
    Client::connect(
        &host,
        port,
        Duration::from_secs(inst.radioctl.reply_timeout_s),
    )
    .map_err(|e| e.to_string())
}

pub fn call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: radioctl_call]
    let spec = SPECS
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("unknown tool {name}"))?;
    if name == "bench_radioctl_models" {
        let tier_ok = insts
            .iter()
            .any(|i| i.kind == Kind::Radioctl && i.tier >= spec.tier);
        return if tier_ok {
            let dir = insts
                .iter()
                .filter(|i| i.kind == Kind::Radioctl)
                .find_map(|i| match &i.radioctl.source {
                    Source::Managed(m) => Some(m),
                    Source::Existing { .. } => None,
                })
                .map_or_else(
                    || rigd::rigs_dir(&Managed { rig: String::new(), device: None, speed: None, port: None, rigs_dir: None, full_power_w: None }),
                    rigd::rigs_dir,
                )?;
            models::tool(&dir, args)
        } else {
            Err(format!(
                "{name} needs a radioctl instrument at tier {:?}",
                spec.tier
            ))
        };
    }
    if name == "bench_radioctl_rigs" {
        return rigs(insts, args);
    }
    if name == "bench_radioctl_stations" || name == "bench_radioctl_station_set" {
        let inst = instrument(insts, args)?;
        if inst.tier < spec.tier {
            return Err(format!(
                "{name} needs tier {:?}; {:?} is configured at {:?}",
                spec.tier, inst.name, inst.tier
            ));
        }
        return if name == "bench_radioctl_stations" {
            loadtools::stations(&inst.radioctl, args)
        } else {
            loadtools::station_set(&inst.radioctl, args)
        };
    }
    let inst = instrument(insts, args)?;
    if inst.tier < spec.tier {
        return Err(format!(
            "{name} needs tier {:?}; {:?} is configured at {:?}",
            spec.tier, inst.name, inst.tier
        ));
    }
    // The safe state does not wait for any other call: it must work while a keyed interval is running, and end it.
    if name == "bench_radioctl_safe_state" {
        let r = open(inst).and_then(|mut c| txtools::safe_state(&mut c, inst, args));
        return with_startup_note(inst, r);
    }
    let lock = crate::instrument_lock(&inst.name);
    let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
    let result = match name {
        "bench_radioctl_status" => status::status(&mut open(inst)?, &inst.name),
        "bench_radioctl_watch" => watch::watch(&mut open(inst)?, args),
        "bench_radioctl_get" => status::get(&mut open(inst)?, args),
        "bench_radioctl_set" => control::set(&mut open(inst)?, &inst.radioctl, args),
        "bench_radioctl_memory" => control::memory(&mut open(inst)?, &inst.radioctl, args),
        "bench_radioctl_scan" => control::scan(&mut open(inst)?, &inst.radioctl, args),
        "bench_radioctl_rx_check" => {
            let audio = audio_in::resolve(insts, inst)?;
            rxcheck::rx_check(
                &mut open(inst)?,
                &inst.radioctl,
                audio.as_ref().map(|a| a as &dyn audio_in::AudioSource),
                args,
            )
        }
        "bench_radioctl_load_check" => {
            let audio = audio_in::resolve(insts, inst)?;
            loadtools::load_check(
                &mut open(inst)?,
                &inst.radioctl,
                audio.as_ref().map(|a| a as &dyn audio_in::AudioSource),
                &inst.name,
                args,
            )
        }
        "bench_radioctl_load_baseline" => {
            let audio = audio_in::resolve(insts, inst)?;
            loadtools::load_baseline(
                &mut open(inst)?,
                &inst.radioctl,
                audio.as_ref().map(|a| a as &dyn audio_in::AudioSource),
                &inst.name,
                args,
            )
        }
        "bench_radioctl_tx_key_test" | "bench_radioctl_tx_power_sweep" => {
            let audio = audio_in::resolve(insts, inst)?;
            let alert_for = inst.radioctl.audio.as_ref().map(|a| txtools::AudioAlert {
                insts,
                instrument: a.instrument.clone(),
            });
            let deps = txtools::Deps {
                spawner: &guard::ProcessSpawner,
                alert: alert_for.as_ref().map(|a| a as &dyn txtools::Alerter),
                audio: audio.as_ref().map(|a| a as &dyn audio_in::AudioSource),
            };
            let mut c = open(inst)?;
            if name == "bench_radioctl_tx_key_test" {
                txtools::key_test(&mut c, inst, &deps, args)
            } else {
                txtools::power_sweep(&mut c, inst, &deps, args)
            }
        }
        "bench_radioctl_diagnose" => diag::diagnose(&Process, &inst.name, &inst.radioctl),
        "bench_radioctl_daemon" => radioctld_action(inst, args),
        _ => Err(format!("{name} does not apply to {:?}", inst.name)),
    };
    with_startup_note(inst, result)
}

/// A result, with the note of the first-contact PTT check if that found a rig left keyed.
fn with_startup_note(inst: &Instrument, r: Result<Value, String>) -> Result<Value, String> {
    // [@ANCHOR: radioctl_startup_note]
    r.map(|mut v| {
        if let (Some(note), Some(o)) = (tx::take_note(&inst.name), v.as_object_mut()) {
            o.insert("startup_release".into(), note);
        }
        v
    })
}

/// `bench_radioctl_daemon`: status, start, stop, restart.
fn radioctld_action(inst: &Instrument, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: radioctl_daemon_tool]
    let action = args
        .get("action")
        .and_then(|a| a.as_str())
        .ok_or("action is required: status, start, stop or restart")?;
    let (name, cfg) = (&inst.name, &inst.radioctl);
    match action {
        "status" => Ok(rigd::status(name, cfg)),
        "start" => rigd::start(name, cfg),
        "stop" | "restart" => {
            // Stopping the daemon of a rig that may be keyed would leave it keyed, with no way left to release it.
            tx::release_rig(name, &rigd::dir(cfg))
                .map_err(|e| format!("not stopping the daemon: {e}"))?;
            let stopped = rigd::stop(name, cfg)?;
            if action == "restart" {
                rigd::start(name, cfg)
            } else {
                Ok(stopped)
            }
        }
        other => Err(format!(
            "action {other:?} is not status, start, stop or restart"
        )),
    }
}

/// `bench_radioctl_rigs`: every configured rig, whether its daemon runs and whether the rig answers. Starts nothing.
fn rigs(insts: &[Instrument], args: &Value) -> Result<Value, String> {
    // [@ANCHOR: radioctl_rigs]
    let only = args.get("instrument").and_then(|n| n.as_str());
    let mut out = Vec::new();
    for i in insts
        .iter()
        .filter(|i| i.kind == Kind::Radioctl && only.is_none_or(|n| n == i.name))
    {
        let daemon = rigd::status(&i.name, &i.radioctl);
        let endpoint = match (
            &i.radioctl.source,
            daemon["running"].as_bool(),
            daemon["port"].as_u64(),
        ) {
            (Source::Existing { host, port }, _, _) => Some((host.clone(), *port)),
            (_, Some(true), Some(p)) => Some(("127.0.0.1".to_string(), p as u16)),
            _ => None,
        };
        let answers = match endpoint {
            None => json!("not started: the daemon starts at the first call that needs the rig"),
            Some((h, p)) => {
                match Client::connect(&h, p, Duration::from_secs(i.radioctl.reply_timeout_s))
                    .and_then(|mut c| c.read_number("get_freq", Some("Frequency")))
                {
                    Ok(f) => json!({"answers": true, "frequency_hz": f}),
                    Err(e) => json!({"answers": false, "error": e.to_string()}),
                }
            }
        };
        out.push(json!({"name": i.name, "tier": format!("{:?}", i.tier), "radioctld": daemon, "rig": answers}));
    }
    if out.is_empty() {
        return Err(match only {
            Some(n) => format!("no radioctl instrument named {n:?}"),
            None => "no radioctl instrument is configured".into(),
        });
    }
    Ok(json!({"rigs": out}))
}

/// Stops what this module started: the managed daemons. Called when the server stops.
pub fn shutdown() {
    shutdown_prefix("");
}

/// The same for the rigs whose names start with `prefix`.
pub fn shutdown_prefix(prefix: &str) {
    // [@ANCHOR: radioctl_shutdown_all]
    // Whatever is keyed is released first: stopping the daemons would make that impossible.
    for failed in tx::release_prefix(prefix) {
        eprintln!("bench_mcp radioctl: could not confirm the release of a keyed rig: {failed}");
    }
    rigd::shutdown_prefix(prefix);
}

#[cfg(test)]
mod tests;
