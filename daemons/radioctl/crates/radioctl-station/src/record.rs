// SPDX-License-Identifier: AGPL-3.0-or-later
//! The durable radio record: what the relay knows about a radio that may be switched off.
//!
//! One JSON file for the station, owner-only (0600), replaced atomically (write a temporary file in
//! the same directory, fsync, rename, fsync the directory). A file that cannot be parsed, or a
//! record that fails validation, is never trusted and never crashes the daemon: the bad file is set
//! aside under a new name for the owner to look at, the bad record is dropped, and the loader
//! reports what it did.

use crate::error::{StationError, StationErrorKind};
use crate::lifecycle::RadioState;
use crate::model::{DeviceId, PowerSource};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Version of the file layout.
pub const RECORD_SCHEMA: u32 = 1;
/// Largest record file that is read; anything bigger is treated as corrupt.
pub const MAX_RECORD_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Longest expected boot time a record may claim (one hour).
pub const MAX_BOOT_MS: u64 = 3_600_000;

/// How to find the radio.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    /// USB serial number, when it has one.
    #[serde(default)]
    pub usb_serial: Option<String>,
    /// Network address (host or IP, optionally `:port`), when it has one.
    #[serde(default)]
    pub network_address: Option<String>,
}

/// How the relay connects after the radio is powered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectMethod {
    /// Wait for the USB hotplug event.
    Hotplug,
    /// Ping the network address until it answers.
    Ping,
    /// Wait the expected boot time, then try.
    BootTime,
}

/// The connect procedure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectProcedure {
    /// What to wait for before connecting.
    pub method: ConnectMethod,
    /// Free text for the owner (for example "set CI-V address 94h, 19200 baud").
    #[serde(default)]
    pub notes: String,
}

/// The state last read from the radio, with when. Served with its age while the radio is off.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KnownState {
    /// Frequency in hertz.
    #[serde(default)]
    pub frequency_hz: Option<u64>,
    /// Mode name.
    #[serde(default)]
    pub mode: Option<String>,
    /// Filter name or width.
    #[serde(default)]
    pub filter: Option<String>,
    /// Levels by name (power, AF gain and so on).
    #[serde(default)]
    pub levels: BTreeMap<String, f64>,
    /// The antenna selected.
    #[serde(default)]
    pub antenna: Option<String>,
    /// Unix seconds when this was read from the radio.
    pub seen_unix: u64,
}

/// Health counters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    /// Unix seconds when the link was last up.
    #[serde(default)]
    pub last_seen_unix: Option<u64>,
    /// Failed connects and drops, all time.
    #[serde(default)]
    pub failures: u32,
    /// Failures since the last success.
    #[serde(default)]
    pub consecutive_failures: u32,
}

/// Everything durable about one radio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RadioRecord {
    /// The device id in the station definition; the key in the file.
    pub id: DeviceId,
    /// Manufacturer.
    pub make: String,
    /// Model.
    pub model: String,
    /// How to find it.
    #[serde(default)]
    pub identity: Identity,
    /// What the owner calls it.
    #[serde(default)]
    pub description: String,
    /// Capability names from the last connection.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// The state last read, if ever.
    #[serde(default)]
    pub last_state: Option<KnownState>,
    /// The lifecycle state when the record was last written.
    pub lifecycle: RadioState,
    /// Unix seconds when `lifecycle` was recorded.
    #[serde(default)]
    pub lifecycle_unix: u64,
    /// How it is powered (a switch channel or a momentary line); `None`: no power button.
    #[serde(default)]
    pub power: Option<PowerSource>,
    /// Expected boot time in milliseconds.
    #[serde(default)]
    pub boot_time_ms: u64,
    /// The connect procedure.
    pub connect: ConnectProcedure,
    /// Health.
    #[serde(default)]
    pub health: Health,
}

/// The cached state with its age.
#[derive(Debug, Clone, PartialEq)]
pub struct Cached {
    /// The state.
    pub state: KnownState,
    /// How old it is.
    pub age: Duration,
}

impl RadioRecord {
    // [@ANCHOR: radioctl_station_record_new]
    /// A bare record: unknown state, Unreachable, no history.
    pub fn new(id: DeviceId, make: &str, model: &str, connect: ConnectProcedure) -> RadioRecord {
        RadioRecord {
            id,
            make: make.to_string(),
            model: model.to_string(),
            identity: Identity::default(),
            description: String::new(),
            capabilities: Vec::new(),
            last_state: None,
            lifecycle: RadioState::Unreachable,
            lifecycle_unix: 0,
            power: None,
            boot_time_ms: 0,
            connect,
            health: Health::default(),
        }
    }

    // [@ANCHOR: radioctl_station_record_validate]
    /// Rejects a record that cannot be trusted: empty ids, absurd boot time, non-finite levels,
    /// text that is far too long.
    pub fn validate(&self) -> Result<(), StationError> {
        let bad = |m: &str| {
            Err(StationError::new(
                StationErrorKind::Store,
                format!("record {}: {m}", self.id.0),
            ))
        };
        let long = |s: &str| s.len() > 4096;
        if self.id.0.is_empty() || self.id.0.len() > 128 {
            return bad("bad id");
        }
        if self.boot_time_ms > MAX_BOOT_MS {
            return bad("boot time over one hour");
        }
        if long(&self.make)
            || long(&self.model)
            || long(&self.description)
            || long(&self.connect.notes)
        {
            return bad("text too long");
        }
        if self.capabilities.len() > 1024 || self.capabilities.iter().any(|c| long(c)) {
            return bad("too many capabilities");
        }
        if let Some(k) = &self.last_state {
            if k.levels.len() > 256 || k.levels.values().any(|v| !v.is_finite()) {
                return bad("bad levels");
            }
        }
        Ok(())
    }

    // [@ANCHOR: radioctl_station_record_cached]
    /// The last-known state with its age at `now_unix` (a clock that went backwards gives age 0).
    /// `None` when the radio was never read.
    pub fn cached(&self, now_unix: u64) -> Option<Cached> {
        self.last_state.as_ref().map(|k| Cached {
            state: k.clone(),
            age: Duration::from_secs(now_unix.saturating_sub(k.seen_unix)),
        })
    }
}

/// What a load found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Loaded {
    /// The trusted records by id.
    pub records: BTreeMap<DeviceId, RadioRecord>,
    /// Things the loader had to do or drop, in words.
    pub problems: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct FileImage {
    schema: u32,
    radios: BTreeMap<String, serde_json::Value>,
}

/// The record file.
#[derive(Debug, Clone)]
pub struct RecordStore {
    path: PathBuf,
}

impl RecordStore {
    // [@ANCHOR: radioctl_station_store_new]
    /// A store over `path`. Nothing is read or written.
    pub fn new(path: impl Into<PathBuf>) -> RecordStore {
        RecordStore { path: path.into() }
    }

    // [@ANCHOR: radioctl_station_store_path]
    /// The file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    // [@ANCHOR: radioctl_station_store_load]
    /// Reads the file. A missing file is an empty store. A file that is too big, not JSON, of
    /// another schema, or not UTF-8 is set aside and treated as empty; a record that fails to
    /// parse, validate or match its key is dropped. Never panics, never trusts bad data. Group or
    /// other permission bits are removed and reported.
    pub fn load(&self) -> Loaded {
        let mut out = Loaded::default();
        let mut file = match std::fs::File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return out,
            Err(e) => {
                out.problems.push(format!("cannot open record file: {e}"));
                return out;
            }
        };
        #[cfg(unix)]
        self.tighten(&file, &mut out.problems);
        let mut bytes = Vec::new();
        let read = (&mut file)
            .take(MAX_RECORD_FILE_BYTES + 1)
            .read_to_end(&mut bytes);
        if let Err(e) = read {
            out.problems.push(format!("cannot read record file: {e}"));
            return out;
        }
        if bytes.len() as u64 > MAX_RECORD_FILE_BYTES {
            self.set_aside("too large", &mut out.problems);
            return out;
        }
        let image: FileImage = match serde_json::from_slice(&bytes) {
            Ok(i) => i,
            Err(e) => {
                out.problems.push(format!("record file is corrupt: {e}"));
                self.set_aside("corrupt", &mut out.problems);
                return out;
            }
        };
        if image.schema != RECORD_SCHEMA {
            out.problems.push(format!(
                "record file schema {} is not understood",
                image.schema
            ));
            self.set_aside("schema", &mut out.problems);
            return out;
        }
        for (key, value) in image.radios {
            match serde_json::from_value::<RadioRecord>(value) {
                Ok(r) if r.id.0 == key => match r.validate() {
                    Ok(()) => {
                        out.records.insert(r.id.clone(), r);
                    }
                    Err(e) => out.problems.push(format!("dropped: {}", e.detail)),
                },
                Ok(_) => out
                    .problems
                    .push(format!("dropped record {key}: id mismatch")),
                Err(e) => out.problems.push(format!("dropped record {key}: {e}")),
            }
        }
        out
    }

    // [@ANCHOR: radioctl_station_store_tighten]
    /// Removes group and other permissions from an existing record file.
    #[cfg(unix)]
    fn tighten(&self, file: &std::fs::File, problems: &mut Vec<String>) {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = file.metadata() {
            let mode = meta.permissions().mode();
            if mode & 0o077 != 0 {
                match file.set_permissions(std::fs::Permissions::from_mode(0o600)) {
                    Ok(()) => problems.push(format!(
                        "record file mode {:o} tightened to 600",
                        mode & 0o777
                    )),
                    Err(e) => problems.push(format!(
                        "record file mode {:o} could not be tightened: {e}",
                        mode & 0o777
                    )),
                }
            }
        }
    }

    // [@ANCHOR: radioctl_station_store_set_aside]
    /// Renames a bad file to `<name>.bad-<n>` (first free n) so it is kept but not read again.
    fn set_aside(&self, why: &str, problems: &mut Vec<String>) {
        for n in 0..1000u32 {
            let mut name = self.path.clone().into_os_string();
            name.push(format!(".bad-{n}"));
            let target = PathBuf::from(name);
            if target.exists() {
                continue;
            }
            match std::fs::rename(&self.path, &target) {
                Ok(()) => problems.push(format!("{why}: set aside as {}", target.display())),
                Err(e) => problems.push(format!("{why}: could not set aside: {e}")),
            }
            return;
        }
        problems.push(format!("{why}: too many set-aside files"));
    }

    // [@ANCHOR: radioctl_station_store_save]
    /// Writes every record atomically with mode 0600: a temporary file in the same directory,
    /// fsync, rename over the file, fsync of the directory. A failure leaves the old file intact.
    pub fn save(&self, records: &BTreeMap<DeviceId, RadioRecord>) -> Result<(), StationError> {
        let io = |what: &str, e: std::io::Error| {
            StationError::new(StationErrorKind::Store, format!("{what}: {e}"))
        };
        let mut radios = BTreeMap::new();
        for (id, r) in records {
            r.validate()?;
            let v = serde_json::to_value(r)
                .map_err(|e| StationError::new(StationErrorKind::Store, e.to_string()))?;
            radios.insert(id.0.clone(), v);
        }
        let image = FileImage {
            schema: RECORD_SCHEMA,
            radios,
        };
        let bytes = serde_json::to_vec_pretty(&image)
            .map_err(|e| StationError::new(StationErrorKind::Store, e.to_string()))?;
        let dir = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut tmp_name = self.path.clone().into_os_string();
        tmp_name.push(format!(".tmp-{}", std::process::id()));
        let tmp = PathBuf::from(tmp_name);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let written = (|| {
            let mut f = options.open(&tmp)?;
            f.write_all(&bytes)?;
            f.sync_all()
        })();
        if let Err(e) = written {
            let _ = std::fs::remove_file(&tmp);
            return Err(io("write record file", e));
        }
        if let Err(e) = std::fs::rename(&tmp, &self.path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(io("replace record file", e));
        }
        if let Ok(d) = std::fs::File::open(dir) {
            let _ = d.sync_all();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("radioctl-station-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sample(id: &str) -> RadioRecord {
        let mut r = RadioRecord::new(
            DeviceId(id.into()),
            "Icom",
            "IC-7300",
            ConnectProcedure {
                method: ConnectMethod::Hotplug,
                notes: "USB, CI-V 94h".into(),
            },
        );
        r.identity.usb_serial = Some("IC7300-0123".into());
        r.capabilities = vec!["ReadPtt".into(), "SetFrequency".into()];
        r.boot_time_ms = 8000;
        r.power = Some(PowerSource::AlwaysOn);
        r.last_state = Some(KnownState {
            frequency_hz: Some(14_074_000),
            mode: Some("USB".into()),
            levels: BTreeMap::from([("rf_power".to_string(), 0.5)]),
            seen_unix: 1_000,
            ..KnownState::default()
        });
        r.health.failures = 2;
        r
    }

    fn all(rs: &[RadioRecord]) -> BTreeMap<DeviceId, RadioRecord> {
        rs.iter().map(|r| (r.id.clone(), r.clone())).collect()
    }

    // Tests [@ANCHOR: radioctl_station_record_new]
    // Tests [@ANCHOR: radioctl_station_record_validate]
    // Tests [@ANCHOR: radioctl_station_record_cached]
    // Tests [@ANCHOR: radioctl_station_store_new]
    // Tests [@ANCHOR: radioctl_station_store_path]
    // Tests [@ANCHOR: radioctl_station_store_save]
    // Tests [@ANCHOR: radioctl_station_store_load]
    #[test]
    fn a_record_round_trips_with_owner_only_permissions() {
        let d = dir("roundtrip");
        let store = RecordStore::new(d.join("radios.json"));
        assert_eq!(store.path(), d.join("radios.json"));
        assert!(store.load().records.is_empty());
        let rec = all(&[sample("ic7300"), sample("k3")]);
        store.save(&rec).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.records, rec);
        assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(store.path())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // no temporary file is left behind
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
        let c = rec[&DeviceId("ic7300".into())].cached(1_090).unwrap();
        assert_eq!(c.age, Duration::from_secs(90));
        assert_eq!(c.state.frequency_hz, Some(14_074_000));
        // a clock that went backwards does not underflow
        assert_eq!(
            rec[&DeviceId("k3".into())].cached(5).unwrap().age,
            Duration::ZERO
        );
        assert!(
            RadioRecord::new(DeviceId("x".into()), "a", "b", sample("x").connect)
                .cached(1)
                .is_none()
        );
        let _ = std::fs::remove_dir_all(d);
    }

    // Tests [@ANCHOR: radioctl_station_store_load]
    // Tests [@ANCHOR: radioctl_station_store_set_aside]
    #[test]
    fn a_corrupt_file_is_set_aside_and_not_trusted() {
        let d = dir("corrupt");
        let store = RecordStore::new(d.join("radios.json"));
        for junk in [
            &b"{ not json"[..],
            &b""[..],
            &[0xff, 0xfe, 0x00][..],
            b"[1,2,3]",
            b"null",
        ] {
            std::fs::write(store.path(), junk).unwrap();
            let l = store.load();
            assert!(l.records.is_empty());
            assert!(
                l.problems.iter().any(|p| p.contains("set aside")),
                "{:?}",
                l.problems
            );
            assert!(!store.path().exists());
        }
        // a saved file after corruption works, and the bad files were kept
        store.save(&all(&[sample("a")])).unwrap();
        assert_eq!(store.load().records.len(), 1);
        assert!(std::fs::read_dir(&d).unwrap().count() >= 6);
        // a wrong schema is not understood
        std::fs::write(store.path(), br#"{"schema":99,"radios":{}}"#).unwrap();
        assert!(store.load().problems.iter().any(|p| p.contains("schema")));
        let _ = std::fs::remove_dir_all(d);
    }

    // Tests [@ANCHOR: radioctl_station_store_load]
    // Tests [@ANCHOR: radioctl_station_record_validate]
    #[test]
    fn one_bad_record_is_dropped_and_the_rest_kept() {
        let d = dir("badrecord");
        let store = RecordStore::new(d.join("radios.json"));
        let good = serde_json::to_value(sample("good")).unwrap();
        let mut absurd = sample("absurd");
        absurd.boot_time_ms = u64::MAX;
        let absurd = serde_json::to_value(absurd).unwrap();
        let moved = serde_json::to_value(sample("moved")).unwrap();
        let image = serde_json::json!({
            "schema": 1,
            "radios": {
                "good": good, "absurd": absurd, "moved": moved,
                "broken": {"id": "broken", "make": 7},
                "other": serde_json::to_value(sample("not-other")).unwrap(),
            }
        });
        std::fs::write(store.path(), serde_json::to_vec(&image).unwrap()).unwrap();
        let l = store.load();
        assert_eq!(l.records.len(), 2, "{:?}", l.problems);
        assert!(l.records.contains_key(&DeviceId("good".into())));
        assert!(l.records.contains_key(&DeviceId("moved".into())));
        assert_eq!(
            l.problems.iter().filter(|p| p.contains("dropped")).count(),
            3,
            "{:?}",
            l.problems
        );
        // saving refuses an invalid record rather than writing it
        let mut bad = sample("bad");
        bad.boot_time_ms = MAX_BOOT_MS + 1;
        assert!(store.save(&all(&[bad])).is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    // Tests [@ANCHOR: radioctl_station_store_load]
    // Tests [@ANCHOR: radioctl_station_store_tighten]
    // Tests [@ANCHOR: radioctl_station_store_save]
    #[cfg(unix)]
    #[test]
    fn a_world_readable_file_is_tightened_and_a_huge_one_refused() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir("perm");
        let store = RecordStore::new(d.join("radios.json"));
        store.save(&all(&[sample("a")])).unwrap();
        std::fs::set_permissions(store.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
        let l = store.load();
        assert_eq!(l.records.len(), 1);
        assert!(l.problems.iter().any(|p| p.contains("tightened")));
        assert_eq!(
            std::fs::metadata(store.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let f = std::fs::File::create(store.path()).unwrap();
        f.set_len(MAX_RECORD_FILE_BYTES + 10).unwrap();
        let l = store.load();
        assert!(l.records.is_empty());
        assert!(l.problems.iter().any(|p| p.contains("too large")));
        // a save into a missing directory fails cleanly
        let missing = RecordStore::new(d.join("nope").join("radios.json"));
        assert_eq!(
            missing.save(&all(&[sample("a")])).unwrap_err().kind,
            StationErrorKind::Store
        );
        let _ = std::fs::remove_dir_all(d);
    }
}
