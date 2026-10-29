// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sound-device tools for bench_mcp: list the sound cards, record and play, verify that a device under test made a sound,
//! detect sound events, speak, alert the operator, set levels and discover which outputs and inputs really work by listening.
//!
//! The tools belong to one instrument entry with `"driver": "audio"` in the bench config. Its tier decides what is listed and
//! allowed: `read_only` (devices, status, levels), `read_config` (record, verify, detect: they hear the room), `full`
//! (play, speak, alert, set volume, loopback, discovery: they make noise in the room, inside the volume cap and quiet hours).
//!
//! The sound cards inside the VNWA are reserved (`08bb:29c0` by default): the VNA driver opens them exclusively, so these
//! tools list them, mark them reserved and never open them.

pub mod alert;
pub mod analysis;
pub mod devices;
pub mod discover;
pub mod engine;
pub mod events;
pub mod heard;
pub mod levels;
pub mod safety;
pub mod speak;
pub mod store;
#[cfg(test)]
pub mod testkit;
pub mod tones;
pub mod tools;
pub mod verify;
pub mod wav;

use crate::config::{Instrument, Kind, Tier};
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

/// Runs a program and gives back its success flag and its output; production is a process, tests script it.
pub trait Run {
    fn run(&self, program: &str, args: &[String]) -> Result<(bool, String), String>;
}

pub struct Process;

impl Run for Process {
    fn run(&self, program: &str, args: &[String]) -> Result<(bool, String), String> {
        // [@ANCHOR: bench_mcp_audio_process_run]
        let out = Command::new(program).args(args).output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!("{program} is not installed (not found on PATH)")
            } else {
                format!("{program}: {e}")
            }
        })?;
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        Ok((out.status.success(), text))
    }
}

/// An alias for a device: what it matches (a node name, or a fragment of the node name or description) and an optional
/// volume cap of its own.
#[derive(Clone, Debug, PartialEq)]
pub struct Port {
    pub matcher: String,
    pub volume_cap: Option<f64>,
}

/// Local quiet hours as minutes of the day; `start > end` means the span crosses midnight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuietHours {
    pub start: u32,
    pub end: u32,
}

impl QuietHours {
    pub fn contains(&self, minute: u32) -> bool {
        // [@ANCHOR: bench_mcp_audio_quiethours_contains]
        if self.start <= self.end {
            (self.start..self.end).contains(&minute)
        } else {
            minute >= self.start || minute < self.end
        }
    }
}

/// The `audio` entry of the bench config.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub outputs: BTreeMap<String, Port>,
    pub inputs: BTreeMap<String, Port>,
    /// `auto` or an alias of `outputs`.
    pub alert_output: String,
    /// `auto` or an alias of `inputs`.
    pub verify_input: String,
    /// The highest wpctl volume any tool sets or plays at.
    pub volume_cap: f64,
    /// The volume used for audible output in quiet hours.
    pub quiet_volume: f64,
    pub quiet_hours: Option<QuietHours>,
    pub capture_dir: Option<PathBuf>,
    pub retention_files: usize,
    pub retention_days: u64,
    pub max_record_s: f64,
    /// The longest generated tone or sound file that is played.
    pub max_play_s: f64,
    /// Named commands that `bench_audio_verify` may run as its action (argument lists, no shell).
    pub verify_actions: BTreeMap<String, Vec<String>>,
    /// Speech: `auto`, `speech-dispatcher`, `espeak-ng` or `piper`; the default voice, language and rate in words per minute; the Piper model file.
    pub tts_engine: String,
    pub tts_voice: Option<String>,
    pub tts_language: String,
    pub tts_rate: u32,
    pub piper_model: Option<String>,
    /// `vid:pid` values and lower-case node-name fragments that are never opened. Always includes `08bb:29c0`.
    pub reserved: Vec<String>,
}

pub const VNWA_CODEC: &str = "08bb:29c0";

impl Default for Config {
    fn default() -> Config {
        Config {
            outputs: BTreeMap::new(),
            inputs: BTreeMap::new(),
            alert_output: "auto".into(),
            verify_input: "auto".into(),
            volume_cap: 0.5,
            quiet_volume: 0.15,
            quiet_hours: Some(QuietHours {
                start: 22 * 60,
                end: 7 * 60,
            }),
            capture_dir: None,
            retention_files: 50,
            retention_days: 14,
            max_record_s: 30.0,
            max_play_s: 10.0,
            verify_actions: BTreeMap::new(),
            tts_engine: "auto".into(),
            tts_voice: None,
            tts_language: "en-us".into(),
            tts_rate: 160,
            piper_model: None,
            reserved: vec![VNWA_CODEC.into()],
        }
    }
}

impl Config {
    /// The alias, if any, that names this device.
    pub fn alias_of(&self, d: &devices::Device) -> Option<String> {
        // [@ANCHOR: bench_mcp_audio_config_alias_of]
        let map = match d.dir {
            devices::Dir::Output => &self.outputs,
            devices::Dir::Input => &self.inputs,
        };
        map.iter()
            .find(|(_, p)| {
                let m = p.matcher.to_ascii_lowercase();
                d.node_name.eq_ignore_ascii_case(&p.matcher)
                    || d.node_name.to_ascii_lowercase().contains(&m)
                    || d.description.to_ascii_lowercase().contains(&m)
            })
            .map(|(a, _)| a.clone())
    }

    /// The volume cap that applies to a device: its own, never above the global cap.
    pub fn cap_for(&self, d: &devices::Device) -> f64 {
        // [@ANCHOR: bench_mcp_audio_config_cap_for]
        let own = d
            .alias
            .as_ref()
            .and_then(|a| match d.dir {
                devices::Dir::Output => self.outputs.get(a),
                devices::Dir::Input => self.inputs.get(a),
            })
            .and_then(|p| p.volume_cap);
        own.map_or(self.volume_cap, |c| c.min(self.volume_cap))
    }

    pub fn capture_dir(&self) -> PathBuf {
        self.capture_dir
            .clone()
            .unwrap_or_else(|| crate::out_dir().join("audio"))
    }
}

fn hhmm(s: &str) -> Result<u32, String> {
    let (h, m) = s.split_once(':').ok_or("time must look like \"22:00\"")?;
    let (h, m): (u32, u32) = (
        h.parse().map_err(|_| "bad hour")?,
        m.parse().map_err(|_| "bad minute")?,
    );
    if h > 23 || m > 59 {
        return Err(format!("{s:?} is not a time of day"));
    }
    Ok(h * 60 + m)
}

fn ports(c: &Value, key: &str) -> Result<BTreeMap<String, Port>, String> {
    let mut out = BTreeMap::new();
    let Some(v) = c.get(key) else { return Ok(out) };
    for (alias, p) in v.as_object().ok_or(format!("{key} must be an object"))? {
        if alias.trim().is_empty() || alias == "auto" {
            return Err(format!("{key}: {alias:?} is not a usable alias"));
        }
        let (matcher, volume_cap) = match p {
            Value::String(s) => (s.clone(), None),
            Value::Object(_) => (
                p.get("match")
                    .and_then(|m| m.as_str())
                    .ok_or(format!("{key}.{alias}: no \"match\""))?
                    .to_string(),
                p.get("volume_cap").and_then(|x| x.as_f64()),
            ),
            _ => return Err(format!("{key}.{alias} must be a string or an object")),
        };
        if matcher.trim().is_empty() {
            return Err(format!("{key}.{alias}: match is empty"));
        }
        if let Some(cap) = volume_cap {
            if !(cap > 0.0 && cap <= 1.0) {
                return Err(format!(
                    "{key}.{alias}: volume_cap must be above 0 and at most 1"
                ));
            }
        }
        out.insert(
            alias.clone(),
            Port {
                matcher,
                volume_cap,
            },
        );
    }
    Ok(out)
}

/// Reads the `audio` settings of an instrument entry: `outputs` and `inputs` (alias to a name fragment, or
/// `{"match": ..., "volume_cap": ...}`), `alert_output` and `verify_input` (`auto` or an alias), `volume_cap` and
/// `quiet_volume` (wpctl scale, 0 to 1), `quiet_hours` (`{"start":"22:00","end":"07:00"}` or `false`), `capture_dir`,
/// `retention_files`, `retention_days`, `max_record_s`, `max_play_s`, `tts_engine`, `tts_voice`, `tts_language`, `tts_rate`, `piper_model`, `verify_actions` (name to program and arguments, the only
/// commands `bench_audio_verify` runs), and `reserved` (more `vid:pid` values or node-name fragments;
/// `08bb:29c0` is always reserved).
pub fn parse_config(c: &Value) -> Result<Config, String> {
    // [@ANCHOR: audio_config]
    let mut cfg = Config {
        outputs: ports(c, "outputs")?,
        inputs: ports(c, "inputs")?,
        ..Config::default()
    };
    let text = |k: &str| c.get(k).and_then(|x| x.as_str()).map(String::from);
    let num = |k: &str| c.get(k).and_then(|x| x.as_f64());
    if let Some(a) = text("alert_output") {
        if a != "auto" && !cfg.outputs.contains_key(&a) {
            return Err(format!(
                "alert_output {a:?} is neither \"auto\" nor an alias in outputs"
            ));
        }
        cfg.alert_output = a;
    }
    if let Some(a) = text("verify_input") {
        if a != "auto" && !cfg.inputs.contains_key(&a) {
            return Err(format!(
                "verify_input {a:?} is neither \"auto\" nor an alias in inputs"
            ));
        }
        cfg.verify_input = a;
    }
    if let Some(v) = num("volume_cap") {
        if !(v > 0.0 && v <= 1.0) {
            return Err("volume_cap must be above 0 and at most 1".into());
        }
        cfg.volume_cap = v;
    }
    if let Some(v) = num("quiet_volume") {
        if !(v > 0.0 && v <= 1.0) {
            return Err("quiet_volume must be above 0 and at most 1".into());
        }
        cfg.quiet_volume = v;
    }
    if cfg.quiet_volume > cfg.volume_cap {
        cfg.quiet_volume = cfg.volume_cap;
    }
    match c.get("quiet_hours") {
        None => {}
        Some(Value::Bool(false)) => cfg.quiet_hours = None,
        Some(q) => {
            let get = |k: &str| {
                q.get(k)
                    .and_then(|x| x.as_str())
                    .ok_or(format!("quiet_hours needs \"{k}\" like \"22:00\""))
                    .and_then(hhmm)
            };
            let (start, end) = (get("start")?, get("end")?);
            if start == end {
                return Err("quiet_hours start and end are the same time".into());
            }
            cfg.quiet_hours = Some(QuietHours { start, end });
        }
    }
    cfg.capture_dir = text("capture_dir").map(PathBuf::from);
    if let Some(n) = c.get("retention_files").and_then(|x| x.as_u64()) {
        cfg.retention_files = n as usize;
    }
    if let Some(n) = c.get("retention_days").and_then(|x| x.as_u64()) {
        cfg.retention_days = n;
    }
    if let Some(m) = num("max_record_s") {
        if !(1.0..=600.0).contains(&m) {
            return Err("max_record_s must be 1 to 600".into());
        }
        cfg.max_record_s = m;
    }
    if let Some(m) = num("max_play_s") {
        if !(0.1..=60.0).contains(&m) {
            return Err("max_play_s must be 0.1 to 60".into());
        }
        cfg.max_play_s = m;
    }
    if let Some(e) = text("tts_engine") {
        if !["auto", "speech-dispatcher", "espeak-ng", "piper"].contains(&e.as_str()) {
            return Err(format!(
                "tts_engine {e:?} is not auto, speech-dispatcher, espeak-ng or piper"
            ));
        }
        cfg.tts_engine = e;
    }
    if let Some(v) = text("tts_voice") {
        speak::safe_code(&v).map_err(|e| format!("tts_voice: {e}"))?;
        cfg.tts_voice = Some(v);
    }
    if let Some(v) = text("tts_language") {
        speak::safe_code(&v).map_err(|e| format!("tts_language: {e}"))?;
        cfg.tts_language = v;
    }
    if let Some(r) = c.get("tts_rate").and_then(|x| x.as_u64()) {
        if !(80..=450).contains(&r) {
            return Err("tts_rate must be 80 to 450 words per minute".into());
        }
        cfg.tts_rate = r as u32;
    }
    cfg.piper_model = text("piper_model");
    if let Some(actions) = c.get("verify_actions") {
        for (name, argv) in actions
            .as_object()
            .ok_or("verify_actions must be an object")?
        {
            let list: Vec<String> = argv
                .as_array()
                .ok_or(format!(
                    "verify_actions.{name} must be a list: the program and its arguments"
                ))?
                .iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect();
            if list.is_empty() || list.len() != argv.as_array().map_or(0, |a| a.len()) {
                return Err(format!(
                    "verify_actions.{name} must be a non-empty list of strings"
                ));
            }
            cfg.verify_actions.insert(name.clone(), list);
        }
    }
    if let Some(list) = c.get("reserved") {
        for x in list
            .as_array()
            .ok_or("reserved must be a list of strings")?
        {
            let s = x
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or("reserved entries must be non-empty strings")?
                .to_ascii_lowercase();
            if !cfg.reserved.contains(&s) {
                cfg.reserved.push(s);
            }
        }
    }
    Ok(cfg)
}

/// Minutes since local midnight now.
pub fn local_minute_of_day() -> u32 {
    let mut t: libc::time_t = 0;
    // SAFETY: time and localtime_r write only to the locals given; tm is fully initialised by localtime_r on success.
    let tm = unsafe {
        libc::time(&mut t);
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    };
    (tm.tm_hour as u32) * 60 + tm.tm_min as u32
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

pub const SPECS: &[Spec] = &[
    Spec {
        name: "bench_audio_devices",
        tier: Tier::ReadOnly,
        desc: "List the sound outputs and inputs PipeWire knows: name (the persistent node name), config alias, description, direction, ALSA card and device, bus, USB path (to use with the bench_usb_* tools) and vid:pid for USB cards, port type, the driver's jack or port availability (an unplugged jack or an HDMI port with no monitor reports 'no': supporting evidence only, never proof), default, sample rates, formats, channels, volume and mute, and the reserved flag. The two USB AUDIO CODEC cards (08bb:29c0) inside the VNWA are listed as reserved and are never opened by any audio tool. Opens no audio stream.",
        props: props!(r#""instrument":{"type":"string"},"device":{"type":"string","description":"one device: alias, node name or a fragment of its name (reserved devices are refused here; they only appear in the plain list)"},"direction":{"type":"string","enum":["output","input"],"description":"filters the list; with device, says which side (default output)"},"include_reserved":{"type":"boolean","description":"default true; false hides the reserved devices"},"levels":{"type":"boolean","description":"default true; read volume and mute with wpctl"}"#),
        required: &[],
    },
    Spec {
        name: "bench_audio_status",
        tier: Tier::ReadOnly,
        desc: "The audio setup: which helper programs are installed (pw-dump, pw-play, pw-record, wpctl, espeak-ng, spd-say, piper, ...), the effective safety limits (volume cap, quiet volume, quiet hours and whether it is quiet now), the capture directory with its retention, the reserved list, and the aliases from the config. Opens no audio stream.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_audio_levels",
        tier: Tier::ReadOnly,
        desc: "Volume (wpctl scale and gain in dB), mute and the volume cap of one device or of all non-reserved devices. Opens no audio stream.",
        props: props!(r#""instrument":{"type":"string"},"device":{"type":"string"},"direction":{"type":"string","enum":["output","input"]}"#),
        required: &[],
    },
    Spec {
        name: "bench_audio_record",
        tier: Tier::ReadConfig,
        desc: "RECORDS FROM A MICROPHONE OR LINE INPUT: it hears the room, and a recording can contain speech. Records `seconds` (0.1 up to the config's max_record_s, default 3) from an input (default: the configured verify input, else the system default source) to a WAV file in the managed capture directory (private, retention-limited, never uploaded) and returns the file path, duration, sample rate, peak and RMS in dBFS, the number of clipped samples, the fraction of 20 ms windows under the silence threshold (default -60 dBFS), and an all_zero flag (a muted, disconnected or dead input records exact zeros). start_on_threshold_dbfs waits up to max_wait_s for the level to pass the threshold and keeps 0.1 s before it. monitor_of_output records what an output is playing (its monitor) instead of an input. Reserved devices are refused.",
        props: props!(r#""instrument":{"type":"string"},"input":{"type":"string","description":"alias, node name or fragment; with monitor_of_output, an output"},"seconds":{"type":"number"},"sample_rate_hz":{"type":"integer"},"start_on_threshold_dbfs":{"type":"number"},"max_wait_s":{"type":"number"},"silence_threshold_dbfs":{"type":"number"},"label":{"type":"string"},"monitor_of_output":{"type":"boolean"}"#),
        required: &[],
    },
    Spec {
        name: "bench_audio_meter",
        tier: Tier::ReadConfig,
        desc: "LISTENS TO THE ROOM through an input for `seconds` (up to 30) and returns a level reading (peak and RMS dBFS) every interval_ms. Nothing is saved. A live level meter for aiming a microphone or checking that a device is making noise.",
        props: props!(r#""instrument":{"type":"string"},"input":{"type":"string"},"seconds":{"type":"number"},"interval_ms":{"type":"integer"}"#),
        required: &[],
    },
    Spec {
        name: "bench_audio_play",
        tier: Tier::Full,
        desc: "MAKES NOISE IN THE ROOM. Plays a generated tone (sine, sweep, two_tone or noise, freq_hz, freq2_hz, seconds up to max_play_s, level_dbfs up to -1, default -12) or a .wav file from the managed capture directory on an output (default: the configured alert output, else the system default sink). The output's volume is lowered to the volume cap (and in quiet hours to the quiet volume) for the playback and put back afterwards, mute included; `volume` asks for a level but never above the limit; `unmute` unmutes it for the playback. Reserved devices are refused. Returns the volume used and the limits.",
        props: props!(r#""instrument":{"type":"string"},"output":{"type":"string"},"tone":{"type":"string","enum":["sine","sweep","two_tone","noise"]},"freq_hz":{"type":"number"},"freq2_hz":{"type":"number"},"seconds":{"type":"number"},"level_dbfs":{"type":"number"},"file":{"type":"string","description":"a .wav file name in the managed capture directory"},"volume":{"type":"number"},"unmute":{"type":"boolean"}"#),
        required: &[],
    },
    Spec {
        name: "bench_audio_set_volume",
        tier: Tier::Full,
        desc: "Set the volume and/or mute of one device through wpctl. An output's volume cannot exceed its volume cap; an input's cannot exceed 1.0. The change persists (unlike the temporary change playing makes). Reserved devices are refused.",
        props: props!(r#""instrument":{"type":"string"},"device":{"type":"string"},"direction":{"type":"string","enum":["output","input"]},"volume":{"type":"number"},"mute":{"type":"boolean"}"#),
        required: &["device"],
    },
    Spec {
        name: "bench_audio_loopback",
        tier: Tier::Full,
        desc: "MAKES A SHORT TONE AND LISTENS FOR IT. Plays a 1 kHz tone (freq_hz, seconds, level_dbfs) on one output while recording one input, to prove that the path works: whether the input heard the tone, its level, signal-to-noise ratio, how much it stands out from neighbouring frequencies, the noise floor before it, and an indicative latency. The output volume is set to the limit (the cap, or the quiet volume in quiet hours; urgent: true uses the cap) and restored afterwards. A dead input (exact zeros) is reported as such. Reserved devices are refused.",
        props: props!(r#""instrument":{"type":"string"},"output":{"type":"string"},"input":{"type":"string"},"freq_hz":{"type":"number"},"seconds":{"type":"number"},"level_dbfs":{"type":"number"},"volume":{"type":"number"},"urgent":{"type":"boolean"}"#),
        required: &[],
    },
    Spec {
        name: "bench_audio_verify",
        tier: Tier::ReadConfig,
        desc: "VERIFIES THAT A DEVICE UNDER CONTROL MADE A SOUND by listening to the room through an input (it hears the room; the recording can contain speech, stays in the managed directory and is never uploaded). First it measures the noise floor for calibrate_s (default 1.5 s); then it starts the action, optionally after delay_s, and listens for window_s (default 5). The action is a command NAME from the config's verify_actions (arbitrary commands are not accepted) and/or whatever else the caller does in parallel, such as another tool call. Judgment: sound present (over the floor by min_over_noise_db, default 10), min_peak_dbfs, dominant frequency within expect_hz +/- tolerance_hz (default 5 %) or freq_min_hz..freq_max_hz (with expect_hz, events are tracked at that frequency, so a tone under the broadband noise is still found), min/max_duration_s, expect_beeps (exact) or min_beeps, and spectral similarity (>= min_similarity, default 0.8) to a reference WAV named in `reference`; expect_silence: true verifies that nothing sounded. save_reference: NAME stores the window as a reference. Returns pass, the reason, every check with its detail, and the evidence (noise floor, events with times and levels, spectrum peaks, similarity), never a bare boolean. A dead input (exact zeros) never passes.",
        props: props!(r#""instrument":{"type":"string"},"input":{"type":"string"},"action":{"type":"string","description":"a name in the config's verify_actions"},"delay_s":{"type":"number"},"calibrate_s":{"type":"number"},"window_s":{"type":"number"},"min_over_noise_db":{"type":"number"},"min_peak_dbfs":{"type":"number"},"expect_hz":{"type":"number"},"tolerance_hz":{"type":"number"},"freq_min_hz":{"type":"number"},"freq_max_hz":{"type":"number"},"min_duration_s":{"type":"number"},"max_duration_s":{"type":"number"},"expect_beeps":{"type":"integer"},"min_beeps":{"type":"integer"},"expect_silence":{"type":"boolean"},"reference":{"type":"string","description":"a .wav file in the managed capture directory"},"min_similarity":{"type":"number"},"save_reference":{"type":"string"},"sample_rate_hz":{"type":"integer"},"label":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_audio_events",
        tier: Tier::ReadConfig,
        desc: "LISTENS TO THE ROOM for `seconds` and reports sound events: onset and offset times, duration, peak and RMS level, and dominant frequency (absent for clicks and noise), with the noise floor and the threshold used (noise floor + min_over_noise_db, or threshold_dbfs). For relays clicking, a rotor motor, an alarm, beeps. track_hz follows one frequency instead of the broadband level (a tone well under the room noise). decode_morse: true also decodes keyed tones as Morse. Nothing is saved.",
        props: props!(r#""instrument":{"type":"string"},"input":{"type":"string"},"seconds":{"type":"number"},"min_over_noise_db":{"type":"number"},"threshold_dbfs":{"type":"number"},"merge_gap_ms":{"type":"number"},"min_event_ms":{"type":"number"},"decode_morse":{"type":"boolean"},"track_hz":{"type":"number","description":"follow the level of this one frequency, which finds a tone far below the broadband noise"},"sample_rate_hz":{"type":"integer"}"#),
        required: &[],
    },
    Spec {
        name: "bench_audio_synthesizers",
        tier: Tier::ReadOnly,
        desc: "Which speech synthesizers are installed (speech-dispatcher with its server state, espeak-ng, Piper), their voices (espeak-ng's list), the configured defaults, and what is missing and how to install it. Installs nothing and makes no sound.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_audio_speak",
        tier: Tier::Full,
        desc: "MAKES NOISE IN THE ROOM: speaks `text` (up to 600 characters) on an output (default: the configured alert output, else the system default sink), or writes it to a WAV file in the capture directory (`wav_file`: a label; nothing is played). Engine auto picks speech-dispatcher only when its server is running and the output is the system default (it cannot target another device), otherwise espeak-ng, then Piper; engine can be named. voice, language (default en-us), rate_wpm (80 to 450). The output's volume is limited to the cap (quiet volume in quiet hours) and restored afterwards, like bench_audio_play. Reserved devices are refused.",
        props: props!(r#""instrument":{"type":"string"},"text":{"type":"string"},"output":{"type":"string"},"engine":{"type":"string","enum":["auto","speech-dispatcher","espeak-ng","piper"]},"voice":{"type":"string"},"language":{"type":"string"},"rate_wpm":{"type":"integer"},"wav_file":{"type":"string"},"volume":{"type":"number"},"unmute":{"type":"boolean"}"#),
        required: &["text"],
    },
    Spec {
        name: "bench_audio_alert",
        tier: Tier::Full,
        desc: "ALERTS THE OPERATOR with sound: a tone pattern by severity (info: one beep; warning: two; urgent: a siren) and/or the spoken message, on the alert output, repeated `repeat` times (1 to 5, at most 30 s in all). Volume is a share of the limit by severity; in quiet hours (default 22:00 to 07:00) only the lowest volume is used unless severity is urgent; the volume cap is never exceeded; the volume and mute are restored afterwards. The result ALWAYS carries text_to_send, because the operator may be away: send it by another channel when the alert matters. If no sound can be made, audible is false with the error, and text_to_send is still returned.",
        props: props!(r#""instrument":{"type":"string"},"message":{"type":"string"},"severity":{"type":"string","enum":["info","warning","urgent"]},"tone":{"type":"boolean"},"speak":{"type":"boolean"},"repeat":{"type":"integer"},"output":{"type":"string"},"engine":{"type":"string"},"voice":{"type":"string"},"language":{"type":"string"},"rate_wpm":{"type":"integer"}"#),
        required: &[],
    },
    Spec {
        name: "bench_audio_discovery",
        tier: Tier::ReadOnly,
        desc: "The stored result of audio path discovery (which outputs make sound and which inputs hear, found by listening), whether it is stale because the device list or a port's state has changed since (hot-plug), and which output and input `auto` selects now. Makes no sound.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_audio_discover",
        tier: Tier::Full,
        desc: "FINDS BY LISTENING WHAT ACTUALLY WORKS, and makes short quiet test tones in the room to do it. Every output in turn plays a 0.4 s tone of its own frequency (volume raised in a few steps from the quiet volume to the volume cap only while nothing is heard) while every candidate input records at once (it hears the room; nothing is kept). Result: the output-by-input matrix of heard or not heard with levels and signal-to-noise ratios; which inputs are real microphones (they heard a tone, or have the noise floor of a live capture; exact zeros means muted or disconnected); which outputs produce sound (heard by a microphone; one heard by none is 'no evidence of sound: speaker absent, muted, or no microphone in range', not a failure); the working loopback pairs; port jack state as supporting evidence only; and the selection of alert output and verification input used by `auto`. With no working microphone it says sound cannot be verified automatically and only the operator can confirm by ear. Refused in quiet hours unless urgent: true (then only the quiet volume is used). Bluetooth and network outputs are skipped unless include_bluetooth. The VNWA codecs are never touched. Volumes and mutes are restored. The result is stored in the capture directory. dry_run: true shows the plan without sound.",
        props: props!(r#""instrument":{"type":"string"},"urgent":{"type":"boolean"},"include_bluetooth":{"type":"boolean"},"dry_run":{"type":"boolean"},"output":{"type":"string","description":"limit the test to this output (a partial run is not stored)"},"input":{"type":"string","description":"limit the test to this input (a partial run is not stored)"}"#),
        required: &[],
    },
];

fn instrument<'a>(insts: &'a [Instrument], args: &Value) -> Result<&'a Instrument, String> {
    // [@ANCHOR: bench_mcp_audio_instrument]
    let audio: Vec<&Instrument> = insts.iter().filter(|i| i.kind == Kind::Audio).collect();
    if let Some(n) = args.get("instrument").and_then(|n| n.as_str()) {
        return audio
            .iter()
            .copied()
            .find(|i| i.name == n)
            .ok_or_else(|| format!("no audio instrument named {n:?}"));
    }
    match audio.as_slice() {
        [one] => Ok(one),
        [] => Err("no audio instrument is configured".into()),
        many => Err(format!(
            "name the instrument: {}",
            many.iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

pub fn tool_defs(insts: &[Instrument]) -> Vec<Tool> {
    SPECS
        .iter()
        .filter(|s| insts.iter().any(|i| i.kind == Kind::Audio && i.tier >= s.tier))
        .map(|s| {
            let schema = json!({"type": "object", "properties": serde_json::from_str::<Value>(s.props).unwrap(), "required": s.required});
            let Value::Object(schema) = schema else { unreachable!() };
            Tool::new(s.name, format!("[{:?}] {}", s.tier, s.desc), Arc::new(schema))
        })
        .collect()
}

/// The user's runtime directory (`XDG_RUNTIME_DIR`).
pub fn runtime_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)
}

/// Where to find a program on `PATH`.
pub(crate) fn on_path(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(program))
            .find(|f| f.is_file())
    })
}

pub fn status(cfg: &Config, now_minute: u32) -> Value {
    let programs: Vec<Value> = [
        "pw-dump",
        "pw-play",
        "pw-record",
        "wpctl",
        "espeak-ng",
        "spd-say",
        "piper",
        "sox",
        "ffmpeg",
    ]
    .iter()
    .map(|p| json!({"program": p, "path": on_path(p)}))
    .collect();
    json!({
        "programs": programs,
        "volume_cap": cfg.volume_cap, "quiet_volume": cfg.quiet_volume,
        "volume_scale": "wpctl volume (0 to 1; the signal amplitude factor is the cube: 0.5 is about -18 dB)",
        "quiet_hours": cfg.quiet_hours.map(|q| json!({"start_minute": q.start, "end_minute": q.end})),
        "quiet_now": cfg.quiet_hours.is_some_and(|q| q.contains(now_minute)),
        "local_minute_of_day": now_minute,
        "capture_dir": cfg.capture_dir(), "retention_files": cfg.retention_files, "retention_days": cfg.retention_days,
        "max_record_s": cfg.max_record_s, "reserved": cfg.reserved,
        "alert_output": cfg.alert_output, "verify_input": cfg.verify_input,
        "outputs": cfg.outputs.iter().map(|(a, p)| json!({"alias": a, "match": p.matcher, "volume_cap": p.volume_cap})).collect::<Vec<_>>(),
        "inputs": cfg.inputs.iter().map(|(a, p)| json!({"alias": a, "match": p.matcher, "volume_cap": p.volume_cap})).collect::<Vec<_>>(),
    })
}

fn list_devices(run: &dyn Run, asound: &Path, cfg: &Config, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_audio_list_devices]
    let mut devs = devices::list(run, asound, cfg)?;
    if let Some(name) = args.get("device").and_then(|d| d.as_str()) {
        let dir = match args.get("direction").and_then(|d| d.as_str()) {
            Some("input") => devices::Dir::Input,
            _ => devices::Dir::Output,
        };
        // resolve refuses reserved devices; to see a reserved card use the plain list.
        let found = devices::resolve(&devs, name, dir)?.node_name.clone();
        devs.retain(|d| d.node_name == found);
    }
    match args.get("direction").and_then(|d| d.as_str()) {
        None => {}
        Some("output") => devs.retain(|d| d.dir == devices::Dir::Output),
        Some("input") => devs.retain(|d| d.dir == devices::Dir::Input),
        Some(o) => return Err(format!("direction {o:?} is not output or input")),
    }
    if args.get("include_reserved").and_then(|v| v.as_bool()) == Some(false) {
        devs.retain(|d| d.reserved.is_none());
    }
    let want_levels = args.get("levels").and_then(|v| v.as_bool()) != Some(false);
    let mut out = Vec::new();
    for d in &devs {
        let mut j = d.json();
        j["volume_cap"] = json!(cfg.cap_for(d));
        // The volume of a reserved device is not read either: it is not touched in any way.
        if want_levels && d.reserved.is_none() {
            match levels::get(run, d.id) {
                Ok(l) => {
                    j["volume"] = json!(l.volume);
                    j["muted"] = json!(l.muted);
                    j["gain_db"] = json!(levels::gain_db(l.volume));
                }
                Err(e) => j["levels_error"] = json!(e),
            }
        }
        out.push(j);
    }
    Ok(
        json!({"devices": out, "volume_scale": "wpctl volume; the signal amplitude factor is the cube"}),
    )
}

pub fn call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String> {
    call_with(
        &Process,
        Path::new("/proc/asound"),
        local_minute_of_day(),
        insts,
        name,
        args,
    )
}

/// `call` with the program runner, the `/proc/asound` directory and the minute of the local day given: production passes the
/// process runner, the real directory and the clock.
pub fn call_with(
    run: &dyn Run,
    asound: &Path,
    minute: u32,
    insts: &[Instrument],
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_audio_call_with]
    let spec = SPECS
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("unknown tool {name}"))?;
    let inst = instrument(insts, args)?;
    if inst.tier < spec.tier {
        return Err(format!(
            "{name} needs tier {:?}; {:?} is configured at {:?}",
            spec.tier, inst.name, inst.tier
        ));
    }
    let cfg = &inst.audio;
    match name {
        "bench_audio_devices" => return list_devices(run, asound, cfg, args),
        "bench_audio_status" => return Ok(status(cfg, minute)),
        _ => {}
    }
    let devs = devices::list(run, asound, cfg)?;
    let stored = discover::load(&cfg.capture_dir());
    let ctx = tools::Ctx {
        run,
        cfg,
        devs: &devs,
        minute,
        discovery: stored.as_ref(),
    };
    if name == "bench_audio_alert" && wants_rediscovery(&ctx, args) {
        // The device list changed since the last discovery: listen again first, then alert on what works now.
        let again = discover::run(&ctx, &json!({}));
        let devs = devices::list(run, asound, cfg)?;
        let stored = discover::load(&cfg.capture_dir());
        let ctx = tools::Ctx {
            run,
            cfg,
            devs: &devs,
            minute,
            discovery: stored.as_ref(),
        };
        let mut out = alert::alert(&ctx, args)?;
        out["rediscovery"] = match again {
            Ok(v) => {
                json!({"ran": true, "audible_seconds": v["audible_seconds"], "alert_output": v["selection"]["alert_output"]})
            }
            Err(e) => json!({"ran": false, "error": e}),
        };
        return Ok(out);
    }
    match name {
        "bench_audio_levels" => tools::levels_get(&ctx, args),
        "bench_audio_record" => tools::record(&ctx, args),
        "bench_audio_meter" => tools::meter(&ctx, args),
        "bench_audio_play" => tools::play(&ctx, args),
        "bench_audio_set_volume" => tools::set_volume(&ctx, args),
        "bench_audio_loopback" => tools::loopback(&ctx, args),
        "bench_audio_verify" => verify::verify(&ctx, args),
        "bench_audio_events" => verify::events(&ctx, args),
        "bench_audio_synthesizers" => speak::synthesizers(&ctx, args),
        "bench_audio_speak" => speak::speak(&ctx, args),
        "bench_audio_alert" => alert::alert(&ctx, args),
        "bench_audio_discover" => discover::run(&ctx, args),
        "bench_audio_discovery" => discover::report(&ctx, args),
        _ => Err(format!("{name} is not implemented")),
    }
}

/// An alert listens again first when its output is chosen by `auto`, a discovery was run before, the devices have changed since,
/// the alert is not urgent (it must not wait) and it is not quiet hours.
fn wants_rediscovery(ctx: &tools::Ctx, args: &Value) -> bool {
    // [@ANCHOR: audio_discover_auto_rerun]
    let urgent = args.get("severity").and_then(|s| s.as_str()) == Some("urgent");
    let quiet = ctx.cfg.quiet_hours.is_some_and(|q| q.contains(ctx.minute));
    ctx.cfg.alert_output == "auto"
        && args.get("output").is_none()
        && args.get("rediscover").and_then(|v| v.as_bool()) != Some(false)
        && !urgent
        && !quiet
        && ctx
            .discovery
            .is_some_and(|d| !d.changes(ctx.devs).is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use testkit::{recorded, Scripted};

    fn insts(cfg: &str) -> Vec<Instrument> {
        crate::config::parse(&format!(
            r#"{{"instruments":{{"snd":{{"driver":"audio"{cfg}}}}}}}"#
        ))
        .unwrap()
    }

    // Tests [@ANCHOR: audio_config]
    #[test]
    fn the_config_defaults_and_the_vnwa_codec_is_always_reserved() {
        let i = insts("");
        let c = &i[0].audio;
        assert_eq!(c.reserved, vec![VNWA_CODEC.to_string()]);
        assert_eq!((c.volume_cap, c.alert_output.as_str()), (0.5, "auto"));
        assert_eq!(
            c.quiet_hours,
            Some(QuietHours {
                start: 1320,
                end: 420
            })
        );
        let i = insts(r#","reserved":["1234:abcd","Radio"]"#);
        assert_eq!(i[0].audio.reserved, vec![VNWA_CODEC, "1234:abcd", "radio"]);
    }

    #[test]
    fn aliases_caps_and_quiet_hours_parse_and_bad_ones_are_refused() {
        let i = insts(
            r#","outputs":{"desk":"Speaker","tv":{"match":"HDMI 1","volume_cap":0.2}},"inputs":{"mic":"Digital Microphone"},
            "alert_output":"desk","verify_input":"mic","volume_cap":0.4,"quiet_volume":0.1,
            "quiet_hours":{"start":"23:30","end":"06:15"},"max_record_s":20"#,
        );
        let c = &i[0].audio;
        assert_eq!(c.outputs["tv"].volume_cap, Some(0.2));
        assert_eq!(
            c.quiet_hours,
            Some(QuietHours {
                start: 23 * 60 + 30,
                end: 6 * 60 + 15
            })
        );
        let bad = |extra: &str| {
            crate::config::parse(&format!(
                r#"{{"instruments":{{"s":{{"driver":"audio"{extra}}}}}}}"#
            ))
            .is_err()
        };
        assert!(bad(r#","alert_output":"nope""#));
        assert!(bad(r#","verify_input":"nope""#));
        assert!(bad(r#","volume_cap":1.5"#));
        assert!(bad(r#","volume_cap":0"#));
        assert!(bad(r#","quiet_hours":{"start":"25:00","end":"07:00"}"#));
        assert!(bad(r#","quiet_hours":{"start":"07:00","end":"07:00"}"#));
        assert!(bad(r#","outputs":{"auto":"x"}"#));
        assert!(bad(r#","outputs":{"a":{"volume_cap":0.2}}"#));
        assert!(bad(r#","max_record_s":0"#));
        assert!(insts(r#","quiet_hours":false"#)[0]
            .audio
            .quiet_hours
            .is_none());
    }

    fn stored(
        alert: Option<&str>,
        input: Option<&str>,
        fingerprint: Vec<String>,
    ) -> discover::Stored {
        discover::Stored {
            at_epoch: 1,
            fingerprint,
            alert_output: alert.map(String::from),
            verify_input: input.map(String::from),
            raw: json!({}),
        }
    }

    // Tests [@ANCHOR: audio_select_auto]
    // Tests [@ANCHOR: bench_mcp_audio_config_alias_of]
    // Tests [@ANCHOR: bench_mcp_audio_quiethours_contains]
    #[test]
    fn auto_follows_the_discovery_and_falls_back_to_the_default_when_the_device_is_gone_or_reserved(
    ) {
        let devs = testkit::devbox_devices();
        let cfg = Config::default();
        let hdmi = devs
            .iter()
            .find(|d| d.node_name.contains("HDMI1"))
            .unwrap()
            .node_name
            .clone();
        let mic2 = devs
            .iter()
            .find(|d| d.node_name.contains("Mic2"))
            .unwrap()
            .node_name
            .clone();
        let st = stored(Some(&hdmi), Some(&mic2), vec![]);
        let ctx = tools::Ctx {
            run: &Scripted::new(&[]),
            cfg: &cfg,
            devs: &devs,
            minute: 720,
            discovery: Some(&st),
        };
        assert_eq!(
            tools::pick(&ctx, &json!({}), "output", devices::Dir::Output)
                .unwrap()
                .node_name,
            hdmi
        );
        assert_eq!(
            tools::pick(&ctx, &json!({}), "input", devices::Dir::Input)
                .unwrap()
                .node_name,
            mic2
        );
        // An explicit name beats the discovery.
        assert!(tools::pick(
            &ctx,
            &json!({"output": "Speaker"}),
            "output",
            devices::Dir::Output
        )
        .unwrap()
        .description
        .ends_with("Speaker"));
        let reserved = devs
            .iter()
            .find(|d| d.reserved.is_some() && d.dir == devices::Dir::Output)
            .unwrap()
            .node_name
            .clone();
        let gone = stored(Some("no-such-node"), Some(&reserved), vec![]);
        let ctx = tools::Ctx {
            run: &Scripted::new(&[]),
            cfg: &cfg,
            devs: &devs,
            minute: 720,
            discovery: Some(&gone),
        };
        assert!(
            tools::pick(&ctx, &json!({}), "output", devices::Dir::Output)
                .unwrap()
                .is_default
        );
        assert!(tools::pick(&ctx, &json!({}), "input", devices::Dir::Input)
            .unwrap()
            .reserved
            .is_none());
        let none = tools::Ctx {
            run: &Scripted::new(&[]),
            cfg: &cfg,
            devs: &devs,
            minute: 720,
            discovery: None,
        };
        assert!(
            tools::pick(&none, &json!({}), "output", devices::Dir::Output)
                .unwrap()
                .is_default
        );
        // A configured alias beats the discovery too.
        let mut pinned = Config {
            alert_output: "desk".into(),
            ..Config::default()
        };
        pinned.outputs.insert(
            "desk".into(),
            Port {
                matcher: "Speaker".into(),
                volume_cap: None,
            },
        );
        let mut with_alias = devs.clone();
        for d in with_alias.iter_mut() {
            d.alias = pinned.alias_of(d);
        }
        let ctx = tools::Ctx {
            run: &Scripted::new(&[]),
            cfg: &pinned,
            devs: &with_alias,
            minute: 720,
            discovery: Some(&st),
        };
        assert!(
            tools::pick(&ctx, &json!({}), "output", devices::Dir::Output)
                .unwrap()
                .description
                .ends_with("Speaker")
        );
    }

    // Tests [@ANCHOR: audio_discover_auto_rerun]
    #[test]
    fn an_alert_listens_again_only_for_a_changed_device_list_and_never_urgently_or_in_quiet_hours()
    {
        let devs = testkit::devbox_devices();
        let cfg = Config::default();
        let fresh = stored(None, None, discover::fingerprint(&devs));
        let old = stored(None, None, vec!["output|gone|-".into()]);
        let mk =
            |st: Option<&discover::Stored>, minute: u32| (Scripted::new(&[]), st.cloned(), minute);
        let check = |st: Option<&discover::Stored>, minute: u32, cfg: &Config, args: Value| {
            let (run, st, minute) = mk(st, minute);
            let ctx = tools::Ctx {
                run: &run,
                cfg,
                devs: &devs,
                minute,
                discovery: st.as_ref(),
            };
            wants_rediscovery(&ctx, &args)
        };
        assert!(check(Some(&old), 720, &cfg, json!({"severity": "warning"})));
        assert!(!check(Some(&fresh), 720, &cfg, json!({})), "fresh");
        assert!(!check(None, 720, &cfg, json!({})), "never run before");
        assert!(
            !check(Some(&old), 720, &cfg, json!({"severity": "urgent"})),
            "urgent alerts do not wait"
        );
        assert!(!check(Some(&old), 23 * 60, &cfg, json!({})), "quiet hours");
        assert!(!check(Some(&old), 720, &cfg, json!({"output": "Speaker"})));
        assert!(!check(Some(&old), 720, &cfg, json!({"rediscover": false})));
        let pinned = Config {
            alert_output: "desk".into(),
            ..Config::default()
        };
        assert!(!check(Some(&old), 720, &pinned, json!({})));
    }

    #[test]
    fn speech_settings_parse_and_bad_ones_are_refused() {
        let i = insts(
            r#","tts_engine":"espeak-ng","tts_voice":"en-us+f3","tts_language":"en","tts_rate":140,"piper_model":"/m/en.onnx""#,
        );
        let c = &i[0].audio;
        assert_eq!(
            (c.tts_engine.as_str(), c.tts_rate, c.tts_language.as_str()),
            ("espeak-ng", 140, "en")
        );
        assert_eq!(
            (c.tts_voice.as_deref(), c.piper_model.as_deref()),
            (Some("en-us+f3"), Some("/m/en.onnx"))
        );
        let bad = |extra: &str| {
            crate::config::parse(&format!(
                r#"{{"instruments":{{"s":{{"driver":"audio"{extra}}}}}}}"#
            ))
            .is_err()
        };
        assert!(bad(r#","tts_engine":"festival""#));
        assert!(bad(r#","tts_voice":"-x""#));
        assert!(bad(r#","tts_rate":10"#));
        assert_eq!(insts("")[0].audio.tts_rate, 160);
    }

    #[test]
    fn verify_actions_are_named_argument_lists_and_nothing_else() {
        let i = insts(r#","verify_actions":{"relay":["/usr/local/bin/click","--once"]}"#);
        assert_eq!(
            i[0].audio.verify_actions["relay"],
            vec!["/usr/local/bin/click", "--once"]
        );
        let bad = |extra: &str| {
            crate::config::parse(&format!(
                r#"{{"instruments":{{"s":{{"driver":"audio"{extra}}}}}}}"#
            ))
            .is_err()
        };
        assert!(bad(r#","verify_actions":{"x":"echo hi"}"#));
        assert!(bad(r#","verify_actions":{"x":[]}"#));
        assert!(bad(r#","verify_actions":{"x":["ok",3]}"#));
    }

    #[test]
    fn quiet_hours_cross_midnight() {
        let q = QuietHours {
            start: 22 * 60,
            end: 7 * 60,
        };
        assert!(q.contains(23 * 60) && q.contains(0) && q.contains(6 * 60 + 59));
        assert!(!q.contains(7 * 60) && !q.contains(12 * 60) && !q.contains(22 * 60 - 1));
        let day = QuietHours {
            start: 13 * 60,
            end: 14 * 60,
        };
        assert!(day.contains(13 * 60 + 30) && !day.contains(14 * 60));
    }

    #[test]
    fn the_quiet_volume_never_exceeds_the_cap() {
        assert_eq!(
            insts(r#","volume_cap":0.1,"quiet_volume":0.3"#)[0]
                .audio
                .quiet_volume,
            0.1
        );
    }

    // Tests [@ANCHOR: bench_mcp_audio_devices_list]
    // Tests [@ANCHOR: bench_mcp_audio_config_cap_for]
    // Tests [@ANCHOR: bench_mcp_audio_process_run]
    // Tests [@ANCHOR: bench_mcp_audio_call_with]
    // Tests [@ANCHOR: bench_mcp_audio_instrument]
    // Tests [@ANCHOR: bench_mcp_audio_list_devices]
    #[test]
    fn the_device_list_reads_levels_for_free_devices_and_not_for_reserved_ones() {
        let i = insts(r#","outputs":{"desk":"Speaker"}"#);
        let all = devices::list(&Scripted::new(&[]), &recorded(), &i[0].audio).unwrap();
        let speaker = all
            .iter()
            .find(|d| d.alias.as_deref() == Some("desk"))
            .unwrap();
        let run = Scripted::new(&[(speaker.id, 0.52, false)]);
        let v = call_with(
            &run,
            &recorded(),
            720,
            &i,
            "bench_audio_devices",
            &json!({}),
        )
        .unwrap();
        let list = v["devices"].as_array().unwrap();
        let sp = list.iter().find(|d| d["alias"] == "desk").unwrap();
        assert_eq!(
            (sp["volume"].as_f64(), sp["muted"].as_bool()),
            (Some(0.52), Some(false))
        );
        let reserved: Vec<&Value> = list.iter().filter(|d| d["reserved"] == true).collect();
        assert!(reserved.len() >= 2);
        assert!(reserved
            .iter()
            .all(|d| d.get("volume").is_none() && d["usb_id"] == "08bb:29c0"));
        let asked: Vec<String> = run.calls.borrow().clone();
        for r in &reserved {
            let id = r["pipewire_id"].to_string();
            assert!(
                !asked.iter().any(|c| c.ends_with(&format!(" {id}"))),
                "a reserved device was queried: {id}"
            );
        }
        let no_res = call_with(
            &run,
            &recorded(),
            720,
            &i,
            "bench_audio_devices",
            &json!({"include_reserved": false, "direction": "output", "levels": false}),
        )
        .unwrap();
        assert!(no_res["devices"]
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d["reserved"] == false && d["direction"] == "output"));
    }

    #[test]
    fn one_device_by_alias_and_a_reserved_one_is_refused() {
        let i = insts(r#","outputs":{"desk":"Speaker"}"#);
        let run = Scripted::new(&[]);
        let one = call_with(
            &run,
            &recorded(),
            720,
            &i,
            "bench_audio_devices",
            &json!({"device": "desk", "levels": false}),
        )
        .unwrap();
        assert_eq!(one["devices"].as_array().unwrap().len(), 1);
        let e = call_with(
            &run,
            &recorded(),
            720,
            &i,
            "bench_audio_devices",
            &json!({"device": "USB_AUDIO_CODEC", "direction": "input"}),
        )
        .unwrap_err();
        assert!(e.contains("reserved"), "{e}");
    }

    #[test]
    fn status_reports_limits_and_the_tier_gate_works() {
        let i = insts(r#","tier":"read_only""#);
        let v = call_with(
            &Scripted::new(&[]),
            &recorded(),
            720,
            &i,
            "bench_audio_status",
            &json!({}),
        )
        .unwrap();
        assert_eq!(v["volume_cap"], 0.5);
        assert!(v["programs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["program"] == "wpctl"));
        let defs = tool_defs(&i);
        assert!(defs.iter().all(|t| t.name.starts_with("bench_audio_")));
        assert!(call_with(
            &Scripted::new(&[]),
            &recorded(),
            720,
            &i,
            "bench_audio_nope",
            &json!({})
        )
        .is_err());
    }
}
