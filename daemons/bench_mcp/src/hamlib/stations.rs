// SPDX-License-Identifier: AGPL-3.0-or-later
//! The table of reference stations that the load plausibility check listens for. Never assume a station is on the air: each entry
//! carries a note with where its status was checked and when, and a `last_heard` date that this module fills from its own
//! detections on this bench. A station that is silent now is "not heard", never a failure, and does not weaken the check, which
//! refuses when any reference IS heard through the load path and needs no reference to be heard. Entries are retired over time.
//!
//! The shipped table lists carrier frequencies, which are public facts, with notes in our own words. The owner reviews it and adds
//! local stations (a repeater, NOAA weather radio, strong broadcast stations). It is stored as `stations.json` in the managed
//! directory once the owner changes it; until then the defaults apply.

use super::control::MODES;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// What to expect in the audio: `wwv` (1 kHz ticks, 440/500/600 Hz tones), `wwvh` (1.2 kHz ticks), `carrier` (a steady carrier),
/// `voice` (speech or music), `none` (nothing special: judged by the signal strength alone).
pub const AUDIO_KINDS: &[&str] = &["wwv", "wwvh", "carrier", "voice", "none"];
const HEARD_RECENTLY_DAYS: i64 = 30;

#[derive(Clone, Debug, PartialEq)]
pub struct Station {
    pub id: String,
    pub name: String,
    pub frequency_hz: f64,
    pub mode: String,
    pub audio: String,
    /// `time_station`, `am_band`, `weather` or `local`.
    pub kind: String,
    pub note: String,
    pub last_heard: Option<String>,
    pub retired: bool,
    pub user_added: bool,
}

impl Station {
    pub fn json(&self, today: &str) -> Value {
        json!({
            "id": self.id, "name": self.name, "frequency_hz": self.frequency_hz, "mode": self.mode, "audio": self.audio, "kind": self.kind,
            "note": self.note, "last_heard": self.last_heard, "heard_recently": heard_recently(self.last_heard.as_deref(), today),
            "retired": self.retired, "added_by_owner": self.user_added,
        })
    }

    /// `hf` below 30 MHz, `vhf` to 300 MHz, `uhf` above.
    pub fn class(&self) -> &'static str {
        class_of(self.frequency_hz)
    }
}

pub fn class_of(hz: f64) -> &'static str {
    // [@ANCHOR: hamlib_station_class]
    if hz < 30e6 {
        "hf"
    } else if hz < 300e6 {
        "vhf"
    } else {
        "uhf"
    }
}

/// Days since 1970-01-01 of a `YYYY-MM-DD` date.
pub fn days(date: &str) -> Option<i64> {
    // [@ANCHOR: hamlib_station_days]
    let p: Vec<i64> = date.split('-').filter_map(|x| x.parse().ok()).collect();
    let [y, m, d] = p.as_slice() else { return None };
    if !(1..=12).contains(m) || !(1..=31).contains(d) {
        return None;
    }
    let (y, m) = if *m <= 2 { (y - 1, m + 12) } else { (*y, *m) };
    Some(365 * y + y / 4 - y / 100 + y / 400 + (153 * (m - 3) + 2) / 5 + d - 719_469 + 305 - 305)
}

pub fn heard_recently(last: Option<&str>, today: &str) -> bool {
    // [@ANCHOR: hamlib_stations_recent]
    match (last.and_then(days), days(today)) {
        (Some(l), Some(t)) => (0..=HEARD_RECENTLY_DAYS).contains(&(t - l)),
        _ => false,
    }
}

/// Today's local date and hour.
pub fn now_local() -> (String, u32) {
    // SAFETY: time and localtime_r are given valid pointers to plain data.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        (
            format!(
                "{:04}-{:02}-{:02}",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday
            ),
            tm.tm_hour as u32,
        )
    }
}

fn st(id: &str, name: &str, mhz: f64, mode: &str, audio: &str, kind: &str, note: &str) -> Station {
    Station {
        id: id.into(),
        name: name.into(),
        frequency_hz: (mhz * 1e6).round(),
        mode: mode.into(),
        audio: audio.into(),
        kind: kind.into(),
        note: note.into(),
        last_heard: None,
        retired: false,
        user_added: false,
    }
}

/// The table that ships. Review it; the notes say what was checked and when, and none of it is a promise that a station is on the air.
pub fn defaults() -> Vec<Station> {
    // [@ANCHOR: hamlib_stations_defaults]
    let wwv = "NIST time station near Fort Collins, Colorado. Carries a 1 kHz tick every second and 440, 500 and 600 Hz tones at set minutes. Status checked against NIST's published station information in January 2026: on the air then; budget proposals threaten WWV and WWVH, so check it again before relying on it.";
    let wwvh = "NIST time station on Kauai, Hawaii. Ticks at 1.2 kHz every second, tones at set minutes. Same status note as WWV: on the air in January 2026, future uncertain.";
    let mut v = vec![];
    for (mhz, tag) in [
        (2.5, "2_5"),
        (5.0, "5"),
        (10.0, "10"),
        (15.0, "15"),
        (20.0, "20"),
    ] {
        v.push(st(
            &format!("wwv-{tag}"),
            &format!("WWV {mhz} MHz"),
            mhz,
            "AM",
            "wwv",
            "time_station",
            wwv,
        ));
    }
    for (mhz, tag) in [(2.5, "2_5"), (5.0, "5"), (10.0, "10"), (15.0, "15")] {
        v.push(st(
            &format!("wwvh-{tag}"),
            &format!("WWVH {mhz} MHz"),
            mhz,
            "AM",
            "wwvh",
            "time_station",
            wwvh,
        ));
    }
    // The Canadian time station CHU stopped its shortwave transmissions on 2026-06-22; it is not listed.
    let am = "A US medium-wave channel centre. Local stations are strong, but which channels carry a strong signal where you are is for you to learn; replace these samples with the stations you know. Not a promise that anything is on this channel.";
    for khz in [
        640, 680, 710, 760, 810, 880, 1010, 1070, 1120, 1200, 1500, 1600,
    ] {
        v.push(st(
            &format!("am-{khz}"),
            &format!("AM broadcast {khz} kHz"),
            f64::from(khz) / 1000.0,
            "AM",
            "voice",
            "am_band",
            am,
        ));
    }
    let wx = "A NOAA Weather Radio channel (public carrier frequency). Which channel serves your area and whether it is on the air is for you to check; keep the ones you hear and retire the others.";
    for mhz in [
        162.400, 162.425, 162.450, 162.475, 162.500, 162.525, 162.550,
    ] {
        v.push(st(
            &format!("wx-{}", (mhz * 1000.0f64).round() as u32),
            &format!("NOAA Weather Radio {mhz:.3} MHz"),
            mhz,
            "FM",
            "voice",
            "weather",
            wx,
        ));
    }
    v
}

pub fn file(dir: &Path) -> PathBuf {
    dir.join("stations.json")
}

fn from_json(v: &Value) -> Result<Station, String> {
    // [@ANCHOR: hamlib_station_from_json]
    let text = |k: &str| v.get(k).and_then(|x| x.as_str()).map(String::from);
    let s = Station {
        id: text("id").ok_or("a station needs id")?,
        name: text("name").unwrap_or_default(),
        frequency_hz: v
            .get("frequency_hz")
            .and_then(|x| x.as_f64())
            .ok_or("a station needs frequency_hz")?,
        mode: text("mode").unwrap_or_else(|| "AM".into()),
        audio: text("audio").unwrap_or_else(|| "none".into()),
        kind: text("kind").unwrap_or_else(|| "local".into()),
        note: text("note").unwrap_or_default(),
        last_heard: text("last_heard"),
        retired: v.get("retired").and_then(|x| x.as_bool()).unwrap_or(false),
        user_added: v
            .get("added_by_owner")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
    };
    check(&s)?;
    Ok(s)
}

/// An entry is valid when its id is a short slug, its frequency is sane, and mode, audio kind and dates are known forms.
pub fn check(s: &Station) -> Result<(), String> {
    // [@ANCHOR: hamlib_station_check]
    if s.id.is_empty()
        || s.id.len() > 40
        || !s
            .id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        return Err(format!(
            "station id {:?} must be 1 to 40 characters of a-z, 0-9, - and _",
            s.id
        ));
    }
    if !(s.frequency_hz.is_finite() && (10_000.0..=6.0e9).contains(&s.frequency_hz)) {
        return Err(format!(
            "station {}: frequency_hz must be 10 kHz to 6 GHz",
            s.id
        ));
    }
    if !MODES.contains(&s.mode.as_str()) {
        return Err(format!(
            "station {}: mode {:?} is not a Hamlib mode name",
            s.id, s.mode
        ));
    }
    if !AUDIO_KINDS.contains(&s.audio.as_str()) {
        return Err(format!(
            "station {}: audio must be one of {}",
            s.id,
            AUDIO_KINDS.join(", ")
        ));
    }
    if s.name.len() > 80 || s.note.len() > 600 {
        return Err(format!(
            "station {}: name is at most 80 characters and note at most 600",
            s.id
        ));
    }
    if s.last_heard.as_deref().is_some_and(|d| days(d).is_none()) {
        return Err(format!("station {}: last_heard must be YYYY-MM-DD", s.id));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub stations: Vec<Station>,
}

impl Table {
    /// The saved table, or the defaults when none was saved. A damaged file is an error, never a silent fall back to the defaults.
    pub fn load(dir: &Path) -> Result<Table, String> {
        // [@ANCHOR: hamlib_stations_load]
        let path = file(dir);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Ok(Table {
                stations: defaults(),
            });
        };
        let v: Value = serde_json::from_str(&text).map_err(|e| {
            format!(
                "{}: {e}. Next: repair or remove the file; the defaults apply when it is absent.",
                path.display()
            )
        })?;
        let list = v
            .get("stations")
            .and_then(|s| s.as_array())
            .ok_or_else(|| format!("{}: no stations list", path.display()))?;
        Ok(Table {
            stations: list
                .iter()
                .map(from_json)
                .collect::<Result<_, _>>()
                .map_err(|e| format!("{}: {e}", path.display()))?,
        })
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        // [@ANCHOR: hamlib_stations_save]
        let list: Vec<Value> = self
            .stations
            .iter()
            .map(|s| {
                let mut j = s.json("");
                j.as_object_mut().map(|o| o.remove("heard_recently"));
                j
            })
            .collect();
        let path = file(dir);
        let tmp = path.with_extension("json.tmp");
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(&json!({"stations": list})).unwrap_or_default(),
        )
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn get_mut(&mut self, id: &str) -> Result<&mut Station, String> {
        self.stations
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| format!("no station {id:?} (bench_hamlib_stations lists them)"))
    }

    /// Records that the station was heard on `date`.
    pub fn mark_heard(&mut self, id: &str, date: &str) -> Result<(), String> {
        // [@ANCHOR: hamlib_stations_heard]
        self.get_mut(id)?.last_heard = Some(date.to_string());
        Ok(())
    }

    /// The references to listen to for a transmit frequency: stations of the same class (HF, VHF, UHF) that the rig can receive, not
    /// retired. On HF the order follows the time of day (low frequencies at night, high by day) with time stations before the AM
    /// band samples; the owner's own entries come first because they were chosen for this site. At most `limit` are returned.
    pub fn references(
        &self,
        tx_hz: f64,
        local_hour: u32,
        covers: &dyn Fn(f64) -> bool,
        limit: usize,
    ) -> Vec<&Station> {
        // [@ANCHOR: hamlib_stations_select]
        let class = class_of(tx_hz);
        let night = !(7..19).contains(&local_hour);
        let mut v: Vec<&Station> = self
            .stations
            .iter()
            .filter(|s| !s.retired && s.class() == class && covers(s.frequency_hz))
            .collect();
        let rank = |s: &Station| -> (u8, i64) {
            let group = match (s.user_added, s.kind.as_str()) {
                (true, _) => 0,
                (_, "time_station") => 1,
                (_, "am_band") => 2,
                _ => 3,
            };
            // Night: lower frequencies first. Day: higher first.
            let f = s.frequency_hz as i64;
            (group, if night { f } else { -f })
        };
        v.sort_by_key(|s| rank(s));
        v.truncate(limit);
        v
    }

    /// Owner changes: add, retire, restore, remove an owner-added entry, change the note.
    pub fn apply(&mut self, action: &str, args: &Value) -> Result<String, String> {
        // [@ANCHOR: hamlib_stations_edit]
        let id = args
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("id is required")?
            .to_string();
        match action {
            "add" => {
                if self.stations.iter().any(|s| s.id == id) {
                    return Err(format!("station {id:?} exists; retire or remove it first"));
                }
                let mut v = args.clone();
                v["added_by_owner"] = json!(true);
                v["kind"] = json!(args.get("kind").and_then(|k| k.as_str()).unwrap_or("local"));
                if v.get("note")
                    .and_then(|n| n.as_str())
                    .is_none_or(str::is_empty)
                {
                    return Err("an added station needs a note saying where its status was checked and when".into());
                }
                let s = from_json(&v)?;
                self.stations.push(s);
                Ok(format!("added {id}"))
            }
            "retire" | "restore" => {
                let retire = action == "retire";
                let s = self.get_mut(&id)?;
                s.retired = retire;
                if let Some(n) = args.get("note").and_then(|n| n.as_str()) {
                    s.note = format!("{n} | {}", s.note).chars().take(600).collect();
                }
                Ok(format!(
                    "{} {id}",
                    if retire { "retired" } else { "restored" }
                ))
            }
            "remove" => {
                let s = self.get_mut(&id)?;
                if !s.user_added {
                    return Err(format!("{id} is part of the shipped table; retire it instead (retiring keeps the record that it was checked)"));
                }
                self.stations.retain(|s| s.id != id);
                Ok(format!("removed {id}"))
            }
            "note" => {
                let n = args
                    .get("note")
                    .and_then(|n| n.as_str())
                    .ok_or("note is required")?;
                if n.len() > 600 {
                    return Err("note is at most 600 characters".into());
                }
                self.get_mut(&id)?.note = n.to_string();
                Ok(format!("note changed for {id}"))
            }
            other => Err(format!(
                "action {other:?} is not add, retire, restore, remove or note"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("bench_mcp-stations-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    // Tests [@ANCHOR: hamlib_stations_defaults]
    // Tests [@ANCHOR: hamlib_station_check]
    // Tests [@ANCHOR: hamlib_station_class]
    #[test]
    fn the_shipped_table_lists_wwv_wwvh_am_and_weather_but_not_chu_and_every_entry_is_valid() {
        let d = defaults();
        let ids: Vec<&str> = d.iter().map(|s| s.id.as_str()).collect();
        for want in ["wwv-2_5", "wwv-20", "wwvh-15", "am-760", "wx-162550"] {
            assert!(ids.contains(&want), "{want}");
        }
        assert!(
            !ids.iter().any(|i| i.contains("chu")),
            "CHU left the shortwave bands on 2026-06-22"
        );
        assert!(
            !ids.contains(&"wwvh-20"),
            "WWVH has no 20 MHz channel in the table"
        );
        for s in &d {
            check(s).unwrap_or_else(|e| panic!("{e}"));
            assert!(
                !s.note.is_empty() && s.last_heard.is_none() && !s.retired,
                "{}",
                s.id
            );
        }
        assert!(d
            .iter()
            .filter(|s| s.kind == "time_station")
            .all(|s| s.note.contains("January 2026")));
    }

    // Tests [@ANCHOR: hamlib_stations_recent]
    // Tests [@ANCHOR: hamlib_station_days]
    #[test]
    fn heard_recently_means_within_thirty_days_and_never_for_a_bad_date() {
        assert!(heard_recently(Some("2026-10-20"), "2026-10-29"));
        assert!(heard_recently(Some("2026-10-29"), "2026-10-29"));
        assert!(!heard_recently(Some("2026-09-01"), "2026-10-29"));
        assert!(
            !heard_recently(Some("2026-11-05"), "2026-10-29"),
            "a date in the future is not recent"
        );
        assert!(!heard_recently(None, "2026-10-29"));
        assert!(!heard_recently(Some("garbage"), "2026-10-29"));
        assert_eq!(days("1970-01-02"), Some(1));
        assert_eq!(days("2026-03-01").unwrap() - days("2026-02-28").unwrap(), 1);
    }

    // Tests [@ANCHOR: hamlib_stations_select]
    #[test]
    fn references_follow_the_band_the_rig_coverage_and_the_time_of_day() {
        let t = Table {
            stations: defaults(),
        };
        let all = |_f: f64| true;
        let day = t.references(7.1e6, 12, &all, 20);
        let night = t.references(7.1e6, 23, &all, 20);
        assert!(day.iter().all(|s| s.class() == "hf"));
        assert_eq!(
            day[0].frequency_hz, 20e6,
            "by day the highest time station first"
        );
        assert_eq!(night[0].frequency_hz, 2.5e6, "at night the lowest first");
        assert!(
            day.iter().position(|s| s.kind == "am_band")
                > day.iter().position(|s| s.kind == "time_station")
        );
        let vhf = t.references(146e6, 12, &all, 20);
        assert_eq!(vhf.len(), 7);
        assert!(vhf.iter().all(|s| s.kind == "weather"));
        // The rig's coverage limits it; the limit applies.
        let hf_only = t.references(7.1e6, 12, &|f: f64| f > 1.8e6, 3);
        assert_eq!(hf_only.len(), 3);
        assert!(hf_only.iter().all(|s| s.frequency_hz > 1.8e6));
        assert!(
            t.references(1.296e9, 12, &all, 5).is_empty(),
            "no UHF reference in the shipped table"
        );
    }

    // Tests [@ANCHOR: hamlib_stations_edit]
    #[test]
    fn the_owner_adds_retires_restores_and_removes_and_the_rules_hold() {
        let mut t = Table {
            stations: defaults(),
        };
        let add = json!({"id": "rpt-146940", "name": "Local repeater", "frequency_hz": 146.94e6, "mode": "FM", "audio": "carrier", "note": "Heard on 2026-10-29 by the owner."});
        assert_eq!(t.apply("add", &add).unwrap(), "added rpt-146940");
        assert!(
            t.stations.last().unwrap().user_added && t.stations.last().unwrap().kind == "local"
        );
        assert!(t.apply("add", &add).unwrap_err().contains("exists"));
        assert!(t
            .apply("add", &json!({"id": "x", "frequency_hz": 1e6, "note": ""}))
            .unwrap_err()
            .contains("note"));
        assert!(t
            .apply(
                "add",
                &json!({"id": "Bad Id", "frequency_hz": 1e6, "note": "n"})
            )
            .is_err());
        assert!(t
            .apply(
                "add",
                &json!({"id": "x1", "frequency_hz": 1e6, "note": "n", "mode": "ZZZ"})
            )
            .is_err());
        assert!(t
            .apply(
                "add",
                &json!({"id": "x2", "frequency_hz": 5.0, "note": "n"})
            )
            .is_err());
        t.apply(
            "retire",
            &json!({"id": "wwv-2_5", "note": "silent for a month, 2026-10-29"}),
        )
        .unwrap();
        assert!(t.stations[0].retired && t.stations[0].note.starts_with("silent for a month"));
        let refs = t.references(7.1e6, 23, &|_| true, 50);
        assert!(
            !refs.iter().any(|s| s.id == "wwv-2_5"),
            "a retired station is not a reference"
        );
        t.apply("restore", &json!({"id": "wwv-2_5"})).unwrap();
        assert!(!t.stations[0].retired);
        assert!(t
            .apply("remove", &json!({"id": "wwv-5"}))
            .unwrap_err()
            .contains("retire it instead"));
        assert_eq!(
            t.apply("remove", &json!({"id": "rpt-146940"})).unwrap(),
            "removed rpt-146940"
        );
        t.apply(
            "note",
            &json!({"id": "wwv-5", "note": "checked 2026-10-29"}),
        )
        .unwrap();
        assert!(t.apply("bogus", &json!({"id": "wwv-5"})).is_err());
        assert!(t.apply("retire", &json!({"id": "nope"})).is_err());
    }

    // Tests [@ANCHOR: hamlib_stations_save]
    // Tests [@ANCHOR: hamlib_stations_load]
    // Tests [@ANCHOR: hamlib_stations_heard]
    // Tests [@ANCHOR: hamlib_station_from_json]
    #[test]
    fn the_table_is_saved_and_read_back_and_a_damaged_file_is_an_error() {
        let d = dir("save");
        assert_eq!(
            Table::load(&d).unwrap().stations.len(),
            defaults().len(),
            "no file: the defaults"
        );
        let mut t = Table {
            stations: defaults(),
        };
        t.mark_heard("wwv-10", "2026-10-28").unwrap();
        t.apply("retire", &json!({"id": "am-640"})).unwrap();
        t.save(&d).unwrap();
        let back = Table::load(&d).unwrap();
        assert_eq!(back, t);
        assert_eq!(
            back.stations
                .iter()
                .find(|s| s.id == "wwv-10")
                .unwrap()
                .last_heard
                .as_deref(),
            Some("2026-10-28")
        );
        assert!(t.mark_heard("nope", "2026-10-28").is_err());
        std::fs::write(file(&d), "{not json").unwrap();
        assert!(Table::load(&d).unwrap_err().contains("Next:"));
        std::fs::write(
            file(&d),
            r#"{"stations":[{"id":"a","frequency_hz":1e6,"mode":"QQ"}]}"#,
        )
        .unwrap();
        assert!(Table::load(&d).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
