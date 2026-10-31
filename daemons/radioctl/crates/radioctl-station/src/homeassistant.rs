// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Home Assistant driver: channels whose lines are Home Assistant entities, switched by REST
//! service calls and confirmed by state events on the WebSocket API
//! (`RELAY_EXTERNAL_SWITCHES_AND_POWER_CONTROL.md` section 4.3a).
//!
//! What this is and is not:
//! - **Lowest assurance.** The driver reports [`Assurance::Mediated`]: Home Assistant, the network
//!   and the device behind it can each be down or slow. It is never a PTT path (the switchboard
//!   refuses a `ptt` channel on it), nothing time-critical goes through it, and an amplifier
//!   powered through it must declare an independent stop (`external_deadman` in the station
//!   definition). The interlocks run in the station before a call is made, not in Home Assistant.
//! - **State comes from events.** The WebSocket session subscribes to `state_changed` and takes a
//!   `get_states` snapshot; a channel's confirmed state is the entity's state in that cache. While
//!   the session is down or not yet synced, or when the entity reports `unavailable` or `unknown`,
//!   or anything but `on` and `off`, the state is **unknown**: [`ChannelDriver::read_line`] fails,
//!   the switchboard records no confirmed value, and the dependency checks that need the channel
//!   confirmed on refuse. A command is still sent while the state is unknown (an off must always be
//!   tried); it is confirmed only by the event that follows.
//! - **The token** is read once from a [`SecretStore`] by name, held in a [`Secret`] whose `Debug`
//!   is redacted, and sent only as the bearer header and the WebSocket auth message. It is never
//!   in a config file, an error message, the audit log or a log line.
//! - **Private addresses only**: an IP literal in a loopback, private or link-local range, never a
//!   name, never the internet. The connection is plain HTTP and WebSocket (no TLS is implemented),
//!   which is why the address must be on the owner's own network.
//! - **Audit**: every service call, session start, authentication result, loss and discovery goes
//!   to an [`AuditLog`].
//! - **Allow-list**: only the configured entities can be written. Discovery lists what Home
//!   Assistant offers so the owner can choose; it writes nothing.

use crate::channel::{Assurance, BoxFuture, ChannelDriver};
use crate::error::{StationError, StationErrorKind};
use crate::http_relay::is_private_or_loopback;
use crate::ws::{
    check_handshake_response, client_key, encode_frame, handshake_request, random_bytes, Decoder,
    Opcode,
};
use radioctl_core::backoff::Backoff;
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::runtime::Handle;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout, Instant};

/// Domains whose entities can be switched on and off.
const WRITABLE: [&str; 4] = ["switch", "input_boolean", "light", "fan"];
/// Domains that can only be read (a sensor channel).
const READ_ONLY: [&str; 1] = ["binary_sensor"];
const MAX_ENTITIES: usize = 64;
const MAX_MESSAGE: usize = 16 * 1024 * 1024;
const MAX_CATALOG: usize = 20_000;
const MAX_REPLY: u64 = 64 * 1024;
const DEFAULT_PORT: u16 = 8123;

/// A secret value. Its `Debug` hides it and it has no `Display`; the only way to the text is
/// [`Secret::expose`], used where it must go on the wire.
pub struct Secret(String);

impl Secret {
    // [@ANCHOR: radioctl_station_ha_secret_new]
    /// Wraps a value.
    pub fn new(value: impl Into<String>) -> Secret {
        Secret(value.into())
    }

    // [@ANCHOR: radioctl_station_ha_secret_expose]
    /// The text, for the wire only.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    // [@ANCHOR: radioctl_station_ha_secret_debug]
    /// `Secret(<redacted>)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// Where long-lived secrets come from: not a config file, not the environment of a log.
pub trait SecretStore: Send + Sync {
    /// The secret called `name`. The error never contains a value.
    fn secret(&self, name: &str) -> Result<Secret, StationError>;
}

/// Secrets held in memory (tests, or a front end that has unlocked a vault).
#[derive(Default)]
pub struct MemorySecrets {
    values: Mutex<BTreeMap<String, String>>,
}

impl MemorySecrets {
    // [@ANCHOR: radioctl_station_ha_memsecrets_new]
    /// An empty store.
    pub fn new() -> MemorySecrets {
        MemorySecrets::default()
    }

    // [@ANCHOR: radioctl_station_ha_memsecrets_set]
    /// Stores a secret under a name.
    pub fn set(&self, name: &str, value: &str) {
        lock(&self.values).insert(name.to_string(), value.to_string());
    }
}

impl SecretStore for MemorySecrets {
    // [@ANCHOR: radioctl_station_ha_memsecrets_secret]
    /// The named secret, if stored.
    fn secret(&self, name: &str) -> Result<Secret, StationError> {
        lock(&self.values)
            .get(name)
            .map(|v| Secret::new(v.clone()))
            .ok_or_else(|| missing_secret(name))
    }
}

/// Secrets in files of a directory, one file per secret, readable by the owner only.
pub struct FileSecrets {
    dir: PathBuf,
}

impl FileSecrets {
    // [@ANCHOR: radioctl_station_ha_filesecrets_new]
    /// A store over `dir`. Nothing is read.
    pub fn new(dir: impl Into<PathBuf>) -> FileSecrets {
        FileSecrets { dir: dir.into() }
    }
}

impl SecretStore for FileSecrets {
    // [@ANCHOR: radioctl_station_ha_filesecrets_secret]
    /// Reads `<dir>/<name>`. The name must be plain (letters, digits, `_`, `-`, `.`, no
    /// separators); the file must be a regular file that group and others cannot read; one
    /// trailing newline is dropped; an empty file is refused.
    fn secret(&self, name: &str) -> Result<Secret, StationError> {
        let plain = !name.is_empty()
            && !name.starts_with('.')
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
        if !plain {
            return Err(StationError::new(
                StationErrorKind::Config,
                "a secret name is letters, digits, underscore, hyphen and dot",
            ));
        }
        let path = self.dir.join(name);
        let meta = std::fs::metadata(&path).map_err(|_| missing_secret(name))?;
        if !meta.is_file() {
            return Err(missing_secret(name));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o077 != 0 {
                return Err(StationError::new(
                    StationErrorKind::Config,
                    format!("the secret file for {name} can be read by others; make it mode 0600"),
                ));
            }
        }
        let text = std::fs::read_to_string(&path).map_err(|_| missing_secret(name))?;
        let value = text.strip_suffix('\n').unwrap_or(&text);
        if value.is_empty() {
            return Err(missing_secret(name));
        }
        Ok(Secret::new(value))
    }
}

// [@ANCHOR: radioctl_station_ha_missing_secret]
fn missing_secret(name: &str) -> StationError {
    StationError::new(
        StationErrorKind::Config,
        format!("no secret named {name} in the secret store"),
    )
}

/// One line of the audit log. It never holds a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    /// Unix seconds.
    pub at_unix: u64,
    /// What was done: `service switch.turn_on`, `websocket connect`, `discover`.
    pub action: String,
    /// The entity concerned, if any.
    pub entity: Option<String>,
    /// `ok`, or `error: ` and why.
    pub result: String,
}

/// Where the driver records what it does.
pub trait AuditLog: Send + Sync {
    /// Records one entry.
    fn record(&self, entry: AuditEntry);
}

/// An audit log held in memory.
#[derive(Default)]
pub struct MemoryAudit {
    entries: Mutex<Vec<AuditEntry>>,
}

impl MemoryAudit {
    // [@ANCHOR: radioctl_station_ha_memaudit_new]
    /// An empty log.
    pub fn new() -> MemoryAudit {
        MemoryAudit::default()
    }

    // [@ANCHOR: radioctl_station_ha_memaudit_entries]
    /// Everything recorded, oldest first.
    pub fn entries(&self) -> Vec<AuditEntry> {
        lock(&self.entries).clone()
    }
}

impl AuditLog for MemoryAudit {
    // [@ANCHOR: radioctl_station_ha_memaudit_record]
    /// Keeps the entry (the most recent 10000).
    fn record(&self, entry: AuditEntry) {
        let mut g = lock(&self.entries);
        g.push(entry);
        let excess = g.len().saturating_sub(10_000);
        g.drain(..excess);
    }
}

/// How to reach Home Assistant and which entities are lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HaConfig {
    /// An IP literal in a private range, optionally with `:port` (default 8123).
    pub address: String,
    /// The name of the long-lived access token in the secret store.
    pub token_name: String,
    /// Line `n` is `entities[n]`, as `domain.object_id`.
    pub entities: Vec<String>,
    /// Longest wait for one HTTP exchange or one connection step.
    pub request_timeout: Duration,
    /// Longest wait for the state event that confirms a command.
    pub confirm_wait: Duration,
    /// How often the session pings Home Assistant; a missed pong ends the session.
    pub ping_every: Duration,
    /// Waits between reconnect attempts of the session.
    pub backoff: Backoff,
}

impl HaConfig {
    // [@ANCHOR: radioctl_station_ha_config_new]
    /// A configuration with five-second request limit, three-second confirmation wait, fifteen
    /// second ping and the default backoff.
    pub fn new(
        address: impl Into<String>,
        token_name: impl Into<String>,
        entities: Vec<String>,
    ) -> HaConfig {
        HaConfig {
            address: address.into(),
            token_name: token_name.into(),
            entities,
            request_timeout: Duration::from_secs(5),
            confirm_wait: Duration::from_secs(3),
            ping_every: Duration::from_secs(15),
            backoff: Backoff::default(),
        }
    }
}

/// An entity Home Assistant offers, for the owner to choose from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    /// `domain.object_id`.
    pub entity_id: String,
    /// Its state now (`on`, `off`, `unavailable`, ...).
    pub state: String,
    /// The name Home Assistant shows, if it has one.
    pub friendly_name: Option<String>,
    /// Whether it can be switched (otherwise it can only be read).
    pub writable: bool,
    /// Whether it is already a line of this driver.
    pub mapped: bool,
}

#[derive(Default)]
struct Cache {
    synced: bool,
    /// Entity id to (state, friendly name), every entity in an allowed domain.
    states: HashMap<String, (String, Option<String>)>,
}

struct Inner {
    addr: SocketAddr,
    token: Secret,
    cfg: HaConfig,
    cache: Mutex<Cache>,
    changed: Notify,
    expect: Mutex<HashMap<String, bool>>,
    audit: Arc<dyn AuditLog>,
}

/// The driver. Dropping it ends the session task.
pub struct HomeAssistant {
    inner: Arc<Inner>,
    task: JoinHandle<()>,
}

// [@ANCHOR: radioctl_station_ha_lock]
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// [@ANCHOR: radioctl_station_ha_unix_now]
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

// [@ANCHOR: radioctl_station_ha_driver_err]
fn driver_err(m: impl Into<String>) -> StationError {
    StationError::new(StationErrorKind::Driver, m)
}

// [@ANCHOR: radioctl_station_ha_parse_address]
/// The owner's address: an IP literal with an optional port (default 8123), private only.
pub fn parse_ha_address(text: &str) -> Result<SocketAddr, StationError> {
    let text = text.trim();
    let addr = if let Ok(sa) = text.parse::<SocketAddr>() {
        sa
    } else if let Ok(ip) = text.parse::<IpAddr>() {
        SocketAddr::new(ip, DEFAULT_PORT)
    } else {
        return Err(StationError::new(
            StationErrorKind::Config,
            "the Home Assistant address must be an IP address on your own network, such as 192.168.1.20",
        ));
    };
    if !is_private_or_loopback(addr.ip()) {
        return Err(StationError::new(
            StationErrorKind::Config,
            "Home Assistant must be on a private network address, never the public internet",
        ));
    }
    Ok(addr)
}

// [@ANCHOR: radioctl_station_ha_check_entity]
/// Checks one entity id: `domain.object_id` in lower-case letters, digits and underscores, in a
/// domain this driver handles. Returns whether it may be written.
pub fn check_entity(entity: &str) -> Result<bool, StationError> {
    let bad = |why: &str| {
        StationError::new(
            StationErrorKind::Config,
            format!("entity \"{entity}\": {why}"),
        )
    };
    let (domain, object) = entity
        .split_once('.')
        .ok_or_else(|| bad("expected domain.object_id"))?;
    let plain = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    };
    if !plain(domain) || !plain(object) {
        return Err(bad("only lower-case letters, digits and underscores"));
    }
    if WRITABLE.contains(&domain) {
        Ok(true)
    } else if READ_ONLY.contains(&domain) {
        Ok(false)
    } else {
        Err(bad(
            "this domain cannot be a channel (switch, input_boolean, light, fan, binary_sensor)",
        ))
    }
}

impl HomeAssistant {
    // [@ANCHOR: radioctl_station_ha_start]
    /// Validates the configuration, reads the token from `secrets` and starts the session task on
    /// `runtime`. Nothing is switched. A bad address, an entity this driver does not handle or a
    /// missing token is an error here, before anything touches the network.
    pub fn start(
        runtime: &Handle,
        config: HaConfig,
        secrets: &dyn SecretStore,
        audit: Arc<dyn AuditLog>,
    ) -> Result<HomeAssistant, StationError> {
        let addr = parse_ha_address(&config.address)?;
        if config.entities.is_empty() || config.entities.len() > MAX_ENTITIES {
            return Err(StationError::new(
                StationErrorKind::Config,
                format!("a Home Assistant bank has 1 to {MAX_ENTITIES} entities"),
            ));
        }
        for e in &config.entities {
            check_entity(e)?;
        }
        let token = secrets.secret(&config.token_name)?;
        let inner = Arc::new(Inner {
            addr,
            token,
            cfg: config,
            cache: Mutex::new(Cache::default()),
            changed: Notify::new(),
            expect: Mutex::new(HashMap::new()),
            audit,
        });
        let task = runtime.spawn(run_sessions(Arc::clone(&inner)));
        Ok(HomeAssistant { inner, task })
    }

    // [@ANCHOR: radioctl_station_ha_is_synced]
    /// Whether the session is up and has the full state: the condition for any state to be known.
    pub fn is_synced(&self) -> bool {
        lock(&self.inner.cache).synced
    }

    // [@ANCHOR: radioctl_station_ha_discover]
    /// The switchable and readable entities Home Assistant has, sorted by id, for the owner to
    /// choose from. Reads only. Fails while the session is not synced.
    pub fn discover(&self) -> Result<Vec<Discovered>, StationError> {
        let found = {
            let cache = lock(&self.inner.cache);
            if !cache.synced {
                None
            } else {
                let mut v: Vec<Discovered> = cache
                    .states
                    .iter()
                    .map(|(id, (state, name))| Discovered {
                        entity_id: id.clone(),
                        state: state.clone(),
                        friendly_name: name.clone(),
                        writable: check_entity(id).unwrap_or(false),
                        mapped: self.inner.cfg.entities.contains(id),
                    })
                    .collect();
                v.sort_by(|a, b| a.entity_id.cmp(&b.entity_id));
                Some(v)
            }
        };
        match found {
            Some(v) => {
                self.inner.record("discover", None, Ok(()));
                Ok(v)
            }
            None => {
                let e = driver_err("Home Assistant is not connected, so nothing can be discovered");
                self.inner.record("discover", None, Err(&e.detail));
                Err(e)
            }
        }
    }

    // [@ANCHOR: radioctl_station_ha_entity]
    /// The entity of a line, bounds-checked.
    fn entity(&self, line: u32) -> Result<&str, StationError> {
        self.inner
            .cfg
            .entities
            .get(line as usize)
            .map(String::as_str)
            .ok_or_else(|| {
                StationError::new(
                    StationErrorKind::Config,
                    format!(
                        "line {line} is past the {} entities",
                        self.inner.cfg.entities.len()
                    ),
                )
            })
    }
}

impl Drop for HomeAssistant {
    // [@ANCHOR: radioctl_station_ha_drop]
    /// Ends the session task.
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl ChannelDriver for HomeAssistant {
    // [@ANCHOR: radioctl_station_ha_assurance]
    /// The lowest level: another system sits between the station and the device.
    fn assurance(&self) -> Assurance {
        Assurance::Mediated
    }

    // [@ANCHOR: radioctl_station_ha_write_line]
    /// Calls `turn_on` or `turn_off` for the line's entity. Sent even while the state is unknown.
    /// Confirmation is the state event that follows ([`ChannelDriver::read_line`] waits for it).
    fn write_line(&self, line: u32, on: bool) -> BoxFuture<'_, Result<(), StationError>> {
        Box::pin(async move {
            let entity = self.entity(line)?.to_string();
            if !check_entity(&entity)? {
                let e = StationError::new(
                    StationErrorKind::Refused,
                    format!("{entity} is a sensor and cannot be switched"),
                );
                self.inner
                    .record("service refused", Some(&entity), Err(&e.detail));
                return Err(e);
            }
            let domain = entity.split_once('.').map_or("", |(d, _)| d).to_string();
            let service = if on { "turn_on" } else { "turn_off" };
            lock(&self.inner.expect).insert(entity.clone(), on);
            let result = self
                .inner
                .post(
                    &format!("/api/services/{domain}/{service}"),
                    &json!({ "entity_id": entity }).to_string(),
                )
                .await;
            let action = format!("service {domain}.{service}");
            match result {
                Ok(()) => {
                    self.inner.record(&action, Some(&entity), Ok(()));
                    Ok(())
                }
                Err(e) => {
                    lock(&self.inner.expect).remove(&entity);
                    self.inner.record(&action, Some(&entity), Err(&e.detail));
                    Err(e)
                }
            }
        })
    }

    // [@ANCHOR: radioctl_station_ha_read_line]
    /// The entity's state from the event cache: `on` or `off`. After a command the call waits (up
    /// to `confirm_wait`) for the event that shows the commanded state. Unknown (an error) when
    /// the session is down, the entity is missing, `unavailable` or `unknown`, or the state is
    /// anything else.
    fn read_line(&self, line: u32) -> BoxFuture<'_, Result<bool, StationError>> {
        Box::pin(async move {
            let entity = self.entity(line)?.to_string();
            let deadline = Instant::now() + self.inner.cfg.confirm_wait;
            loop {
                let changed = self.inner.changed.notified();
                let known = self.inner.state_of(&entity);
                let want = lock(&self.inner.expect).get(&entity).copied();
                match (&known, want) {
                    (Ok(have), Some(want)) if *have == want => {
                        lock(&self.inner.expect).remove(&entity);
                        return known;
                    }
                    (_, Some(_)) if Instant::now() < deadline => {
                        let _ =
                            timeout(deadline.saturating_duration_since(Instant::now()), changed)
                                .await;
                    }
                    _ => {
                        lock(&self.inner.expect).remove(&entity);
                        return known;
                    }
                }
            }
        })
    }

    // [@ANCHOR: radioctl_station_ha_can_read_back]
    /// Home Assistant reports entity state.
    fn can_read_back(&self) -> bool {
        true
    }
}

impl Inner {
    // [@ANCHOR: radioctl_station_ha_record]
    /// Writes one audit entry.
    fn record(&self, action: &str, entity: Option<&str>, result: Result<(), &str>) {
        self.audit.record(AuditEntry {
            at_unix: unix_now(),
            action: action.to_string(),
            entity: entity.map(str::to_string),
            result: match result {
                Ok(()) => "ok".to_string(),
                Err(why) => format!("error: {why}"),
            },
        });
    }

    // [@ANCHOR: radioctl_station_ha_state_of]
    /// The state of an entity as `on` (true) or `off` (false), or why it is unknown.
    fn state_of(&self, entity: &str) -> Result<bool, StationError> {
        let cache = lock(&self.cache);
        if !cache.synced {
            return Err(driver_err(
                "Home Assistant is not connected: the state of every entity is unknown",
            ));
        }
        match cache.states.get(entity).map(|(s, _)| s.as_str()) {
            Some("on") => Ok(true),
            Some("off") => Ok(false),
            Some(other) => Err(driver_err(format!(
                "{entity} reports \"{other}\": its state is unknown"
            ))),
            None => Err(driver_err(format!(
                "Home Assistant has not reported {entity}: its state is unknown"
            ))),
        }
    }

    // [@ANCHOR: radioctl_station_ha_post]
    /// One HTTP/1.0 POST with the bearer token; success is a 200 or 201.
    async fn post(&self, path: &str, body: &str) -> Result<(), StationError> {
        let exchange = async {
            let mut s = TcpStream::connect(self.addr)
                .await
                .map_err(|e| driver_err(format!("cannot reach Home Assistant: {e}")))?;
            let req = format!(
                "POST {path} HTTP/1.0\r\nHost: {}\r\nAuthorization: Bearer {}\r\n\
                 Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                self.addr,
                self.token.expose(),
                body.len()
            );
            s.write_all(req.as_bytes())
                .await
                .map_err(|e| driver_err(format!("cannot send to Home Assistant: {e}")))?;
            let mut buf = Vec::new();
            s.take(MAX_REPLY)
                .read_to_end(&mut buf)
                .await
                .map_err(|e| driver_err(format!("no answer from Home Assistant: {e}")))?;
            Ok::<_, StationError>(buf)
        };
        let buf = timeout(self.cfg.request_timeout, exchange)
            .await
            .map_err(|_| {
                StationError::new(StationErrorKind::Timeout, "Home Assistant did not answer")
            })??;
        let text = String::from_utf8_lossy(&buf);
        let status = text.lines().next().unwrap_or("").trim().to_string();
        if status.contains(" 200") || status.contains(" 201") {
            Ok(())
        } else if status.contains(" 401") || status.contains(" 403") {
            Err(driver_err(
                "Home Assistant rejected the access token (401 or 403)",
            ))
        } else {
            Err(driver_err(format!(
                "Home Assistant answered \"{status}\" to the service call"
            )))
        }
    }

    // [@ANCHOR: radioctl_station_ha_apply_state]
    /// Records one entity's state (a `state_changed` event or a snapshot entry). Entities in other
    /// domains are ignored; the catalog is bounded.
    fn apply_state(&self, entity: &str, state: &str, name: Option<String>) {
        let Some((domain, _)) = entity.split_once('.') else {
            return;
        };
        if !WRITABLE.contains(&domain) && !READ_ONLY.contains(&domain) {
            return;
        }
        let mut cache = lock(&self.cache);
        if cache.states.len() >= MAX_CATALOG && !cache.states.contains_key(entity) {
            return;
        }
        let keep = cache.states.get(entity).and_then(|(_, n)| n.clone());
        cache
            .states
            .insert(entity.to_string(), (state.to_string(), name.or(keep)));
    }

    // [@ANCHOR: radioctl_station_ha_set_synced]
    /// Marks the cache usable, or throws it away when the session is lost.
    fn set_synced(&self, synced: bool) {
        {
            let mut cache = lock(&self.cache);
            cache.synced = synced;
            if !synced {
                cache.states.clear();
            }
        }
        self.changed.notify_waiters();
    }
}

// [@ANCHOR: radioctl_station_ha_run_sessions]
/// The session task: connect, authenticate, follow events, and again after a backoff.
async fn run_sessions(inner: Arc<Inner>) {
    let mut failures = 0u32;
    loop {
        let result = session(&inner, &mut failures).await;
        inner.set_synced(false);
        let why = match result {
            Ok(()) => "session ended".to_string(),
            Err(e) => e,
        };
        inner.record("websocket lost", None, Err(&why));
        failures = failures.saturating_add(1);
        sleep(inner.cfg.backoff.delay(failures)).await;
    }
}

// [@ANCHOR: radioctl_station_ha_read_head]
/// Reads an HTTP response head (to the blank line), returning it and any bytes after it.
async fn read_head(stream: &mut TcpStream) -> Result<(String, Vec<u8>), String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buf.split_off(at + 4);
            return Ok((String::from_utf8_lossy(&buf).into_owned(), rest));
        }
        if buf.len() > 8192 {
            return Err("the server's answer has no end".into());
        }
        match stream.read(&mut chunk).await {
            Ok(0) => return Err("the server closed during the handshake".into()),
            Ok(n) => buf.extend_from_slice(chunk.get(..n).unwrap_or(&[])),
            Err(e) => return Err(format!("handshake read: {e}")),
        }
    }
}

// [@ANCHOR: radioctl_station_ha_send_json]
/// Sends one JSON text frame.
async fn send_json(stream: &mut TcpStream, value: &Json) -> Result<(), String> {
    let frame = encode_frame(
        Opcode::Text,
        value.to_string().as_bytes(),
        Some(random_bytes::<4>()),
    );
    stream
        .write_all(&frame)
        .await
        .map_err(|e| format!("write: {e}"))
}

// [@ANCHOR: radioctl_station_ha_session]
/// One WebSocket session until it fails.
async fn session(inner: &Arc<Inner>, failures: &mut u32) -> Result<(), String> {
    let limit = inner.cfg.request_timeout;
    let mut stream = timeout(limit, TcpStream::connect(inner.addr))
        .await
        .map_err(|_| "connect timed out".to_string())?
        .map_err(|e| format!("connect: {e}"))?;
    let _ = stream.set_nodelay(true);
    let key = client_key();
    stream
        .write_all(handshake_request(&inner.addr.to_string(), "/api/websocket", &key).as_bytes())
        .await
        .map_err(|e| format!("handshake write: {e}"))?;
    let (head, rest) = timeout(limit, read_head(&mut stream))
        .await
        .map_err(|_| "handshake timed out".to_string())??;
    check_handshake_response(&head, &key)?;
    let mut decoder = Decoder::new(MAX_MESSAGE);
    let mut pending: std::collections::VecDeque<crate::ws::Message> = decoder.push(&rest)?.into();
    let mut chunk = vec![0u8; 16 * 1024];

    let mut authenticated = false;
    let mut sent_subscribe = false;
    let mut next_ping = Instant::now() + inner.cfg.ping_every;
    let mut ping_sent: Option<(u64, Instant)> = None;
    let mut ping_id = 100u64;
    let mut started = false;
    loop {
        while let Some(message) = pending.pop_front() {
            match message.opcode {
                Opcode::Ping => {
                    let pong =
                        encode_frame(Opcode::Pong, &message.payload, Some(random_bytes::<4>()));
                    stream
                        .write_all(&pong)
                        .await
                        .map_err(|e| format!("write: {e}"))?;
                }
                Opcode::Pong => {}
                Opcode::Close => return Err("Home Assistant closed the connection".into()),
                Opcode::Binary | Opcode::Continuation => {}
                Opcode::Text => {
                    let v: Json = serde_json::from_slice(&message.payload)
                        .map_err(|e| format!("not JSON: {e}"))?;
                    match v.get("type").and_then(Json::as_str).unwrap_or("") {
                        "auth_required" => {
                            send_json(
                                &mut stream,
                                &json!({"type": "auth", "access_token": inner.token.expose()}),
                            )
                            .await?;
                        }
                        "auth_ok" => {
                            authenticated = true;
                            inner.record("websocket auth", None, Ok(()));
                        }
                        "auth_invalid" => {
                            inner.record("websocket auth", None, Err("token rejected"));
                            return Err("Home Assistant rejected the access token".into());
                        }
                        "event" => {
                            let data = v.pointer("/event/data");
                            let is_change = v.pointer("/event/event_type").and_then(Json::as_str)
                                == Some("state_changed");
                            if let (true, Some(data)) = (is_change, data) {
                                if let Some(id) = data.get("entity_id").and_then(Json::as_str) {
                                    let new = data.get("new_state");
                                    let state = new
                                        .and_then(|n| n.get("state"))
                                        .and_then(Json::as_str)
                                        .unwrap_or("unknown");
                                    let name = new
                                        .and_then(|n| n.pointer("/attributes/friendly_name"))
                                        .and_then(Json::as_str)
                                        .map(str::to_string);
                                    inner.apply_state(id, state, name);
                                    inner.changed.notify_waiters();
                                }
                            }
                        }
                        "result" => {
                            let id = v.get("id").and_then(Json::as_u64).unwrap_or(0);
                            let ok = v.get("success").and_then(Json::as_bool).unwrap_or(false);
                            if id == 2 {
                                if !ok {
                                    return Err("get_states failed".into());
                                }
                                for s in v
                                    .get("result")
                                    .and_then(Json::as_array)
                                    .into_iter()
                                    .flatten()
                                {
                                    if let (Some(e), Some(st)) = (
                                        s.get("entity_id").and_then(Json::as_str),
                                        s.get("state").and_then(Json::as_str),
                                    ) {
                                        let name = s
                                            .pointer("/attributes/friendly_name")
                                            .and_then(Json::as_str)
                                            .map(str::to_string);
                                        inner.apply_state(e, st, name);
                                    }
                                }
                                inner.set_synced(true);
                                if !started {
                                    started = true;
                                    *failures = 0;
                                    inner.record("websocket connect", None, Ok(()));
                                }
                            } else if id == 1 && !ok {
                                return Err("subscribe_events failed".into());
                            }
                        }
                        "pong" => {
                            let id = v.get("id").and_then(Json::as_u64).unwrap_or(0);
                            if ping_sent.is_some_and(|(sent, _)| sent == id) {
                                ping_sent = None;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if authenticated && !sent_subscribe {
            sent_subscribe = true;
            send_json(
                &mut stream,
                &json!({"id": 1, "type": "subscribe_events", "event_type": "state_changed"}),
            )
            .await?;
            send_json(&mut stream, &json!({"id": 2, "type": "get_states"})).await?;
        }
        let wait = ping_sent
            .map_or(next_ping, |(_, at)| at + inner.cfg.ping_every)
            .saturating_duration_since(Instant::now());
        tokio::select! {
            read = stream.read(&mut chunk) => {
                let n = read.map_err(|e| format!("read: {e}"))?;
                if n == 0 {
                    return Err("Home Assistant closed the connection".into());
                }
                pending.extend(decoder.push(chunk.get(..n).unwrap_or(&[]))?);
            }
            () = sleep(wait) => {
                if ping_sent.is_some() {
                    return Err("Home Assistant stopped answering pings".into());
                }
                ping_id += 1;
                ping_sent = Some((ping_id, Instant::now()));
                next_ping = Instant::now() + inner.cfg.ping_every;
                send_json(&mut stream, &json!({"id": ping_id, "type": "ping"})).await?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: radioctl_station_ha_secret_new]
    // Tests [@ANCHOR: radioctl_station_ha_secret_expose]
    // Tests [@ANCHOR: radioctl_station_ha_secret_debug]
    #[test]
    fn a_secret_never_shows_in_debug() {
        let s = Secret::new("hunter2-token");
        assert_eq!(format!("{s:?}"), "Secret(<redacted>)");
        assert_eq!(s.expose(), "hunter2-token");
    }

    // Tests [@ANCHOR: radioctl_station_ha_parse_address]
    #[test]
    fn only_private_ip_literals_are_accepted() {
        assert_eq!(
            parse_ha_address("192.168.1.20").unwrap(),
            "192.168.1.20:8123".parse().unwrap()
        );
        assert_eq!(parse_ha_address(" 10.0.0.5:9000 ").unwrap().port(), 9000);
        assert!(parse_ha_address("127.0.0.1").is_ok());
        for bad in [
            "8.8.8.8",
            "homeassistant.local",
            "example.com:8123",
            "",
            "1.1.1.1:80",
        ] {
            assert_eq!(
                parse_ha_address(bad).unwrap_err().kind,
                StationErrorKind::Config,
                "{bad}"
            );
        }
    }

    // Tests [@ANCHOR: radioctl_station_ha_check_entity]
    #[test]
    fn entities_are_checked() {
        assert!(check_entity("switch.amp_plug").unwrap());
        assert!(check_entity("input_boolean.test").unwrap());
        assert!(!check_entity("binary_sensor.door").unwrap());
        for bad in [
            "amp",
            "switch.",
            ".x",
            "Switch.amp",
            "switch.a/b",
            "switch.a b",
            "lock.front_door",
            "cover.garage",
            "script.run",
            "switch.a?x=1",
        ] {
            assert!(check_entity(bad).is_err(), "{bad}");
        }
    }
}
