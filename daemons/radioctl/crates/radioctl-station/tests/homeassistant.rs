// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Home Assistant driver against a fake Home Assistant on a loopback socket: the WebSocket
//! handshake, authentication, `subscribe_events`, `get_states`, state events and pings on one side,
//! REST service calls with a bearer token on the other. Covers entity discovery, confirmation by
//! event, unknown (unavailable, link down, wrong token) blocking dependent steps, the lowest
//! assurance level, no PTT, the audit log, and that the token never leaves the wire.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use radioctl_core::backoff::Backoff;
use radioctl_station::channel::{Assurance, ChannelDriver, Switchboard};
use radioctl_station::error::StationErrorKind;
use radioctl_station::homeassistant::{
    check_entity, parse_ha_address, AuditLog, FileSecrets, HaConfig, HomeAssistant, MemoryAudit,
    MemorySecrets, Secret, SecretStore,
};
use radioctl_station::model::{ChannelId, DeviceId, StationDef, Value};
use radioctl_station::station::Station;
use radioctl_station::ws::{accept_key, encode_frame, Decoder, Opcode};
use serde_json::{json, Value as Json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

const TOKEN: &str = "llat-SECRET-0123456789";

#[derive(Default)]
struct Fake {
    entities: BTreeMap<String, (String, String)>,
    calls: Vec<(String, String, bool)>,
    clients: Vec<mpsc::UnboundedSender<Push>>,
    ignore_commands: bool,
    mute_pings: bool,
    reject_rest: bool,
    ws_connections: u32,
}

enum Push {
    Frame(Vec<u8>),
    Drop,
}

struct Ha {
    state: Arc<Mutex<Fake>>,
    addr: std::net::SocketAddr,
    task: Option<JoinHandle<()>>,
}

fn event(entity: &str, state: &str) -> Vec<u8> {
    let msg = json!({"id": 1, "type": "event", "event": {"event_type": "state_changed",
        "data": {"entity_id": entity, "new_state": {"entity_id": entity, "state": state,
        "attributes": {"friendly_name": entity}}}}});
    encode_frame(Opcode::Text, msg.to_string().as_bytes(), None)
}

impl Ha {
    async fn start() -> Ha {
        let mut fake = Fake::default();
        for (id, state) in [
            ("switch.radio_plug", "off"),
            ("switch.amp_plug", "off"),
            ("binary_sensor.door", "off"),
            ("lock.front", "locked"),
        ] {
            fake.entities
                .insert(id.to_string(), (state.to_string(), format!("Name of {id}")));
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let mut ha = Ha {
            state: Arc::new(Mutex::new(fake)),
            addr: listener.local_addr().expect("addr"),
            task: None,
        };
        ha.serve(listener);
        ha
    }

    fn serve(&mut self, listener: TcpListener) {
        let state = Arc::clone(&self.state);
        self.task = Some(tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(connection(stream, Arc::clone(&state)));
            }
        }));
    }

    fn set_state(&self, entity: &str, value: &str) {
        let mut g = self.state.lock().unwrap();
        if let Some(e) = g.entities.get_mut(entity) {
            e.0 = value.to_string();
        }
        g.clients
            .retain(|c| c.send(Push::Frame(event(entity, value))).is_ok());
    }

    fn drop_websockets(&self) {
        let mut g = self.state.lock().unwrap();
        for c in g.clients.drain(..) {
            let _ = c.send(Push::Drop);
        }
    }

    fn calls(&self) -> Vec<(String, String, bool)> {
        self.state.lock().unwrap().calls.clone()
    }

    fn ws_connections(&self) -> u32 {
        self.state.lock().unwrap().ws_connections
    }

    async fn stop(&mut self) {
        if let Some(t) = self.task.take() {
            t.abort();
            let _ = t.await;
        }
    }
}

async fn read_head(stream: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buf.split_off(at + 4);
            return Some((String::from_utf8_lossy(&buf).into_owned(), rest));
        }
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

async fn connection(mut stream: TcpStream, state: Arc<Mutex<Fake>>) {
    let Some((head, mut rest)) = read_head(&mut stream).await else {
        return;
    };
    let first = head.lines().next().unwrap_or("").to_string();
    let header = |name: &str| {
        head.lines().find_map(|l| {
            let (n, v) = l.split_once(':')?;
            n.trim()
                .eq_ignore_ascii_case(name)
                .then(|| v.trim().to_string())
        })
    };
    if first.starts_with("GET /api/websocket") {
        let key = header("sec-websocket-key").unwrap_or_default();
        let reply = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\r\n",
            accept_key(&key)
        );
        let _ = stream.write_all(reply.as_bytes()).await;
        websocket(stream, state).await;
        return;
    }
    // REST
    let length: usize = header("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    while rest.len() < length {
        let mut chunk = [0u8; 512];
        match stream.read(&mut chunk).await {
            Ok(n) if n > 0 => rest.extend_from_slice(&chunk[..n]),
            _ => break,
        }
    }
    let authorized = header("authorization").as_deref() == Some(&format!("Bearer {TOKEN}"));
    let body = String::from_utf8_lossy(&rest).into_owned();
    let path = first.split_whitespace().nth(1).unwrap_or("").to_string();
    let (reject, ignore) = {
        let mut g = state.lock().unwrap();
        g.calls.push((path.clone(), body.clone(), authorized));
        (g.reject_rest, g.ignore_commands)
    };
    let status = if !authorized || reject {
        "401 Unauthorized"
    } else if path.starts_with("/api/services/") {
        "200 OK"
    } else {
        "404 Not Found"
    };
    if authorized && !reject && !ignore {
        let service = path.rsplit('/').next().unwrap_or("");
        let entity = serde_json::from_str::<Json>(&body).ok().and_then(|v| {
            v.get("entity_id")
                .and_then(Json::as_str)
                .map(str::to_string)
        });
        if let Some(entity) = entity {
            let value = if service == "turn_on" { "on" } else { "off" };
            tokio::time::sleep(Duration::from_millis(30)).await; // Home Assistant is not instant
            let mut g = state.lock().unwrap();
            if let Some(e) = g.entities.get_mut(&entity) {
                e.0 = value.to_string();
            }
            g.clients
                .retain(|c| c.send(Push::Frame(event(&entity, value))).is_ok());
        }
    }
    let _ = stream
        .write_all(format!("HTTP/1.0 {status}\r\nContent-Length: 2\r\n\r\n[]").as_bytes())
        .await;
}

async fn websocket(mut stream: TcpStream, state: Arc<Mutex<Fake>>) {
    let (tx, mut rx) = mpsc::unbounded_channel::<Push>();
    state.lock().unwrap().ws_connections += 1;
    let send = |v: Json| encode_frame(Opcode::Text, v.to_string().as_bytes(), None);
    let _ = stream
        .write_all(&send(
            json!({"type": "auth_required", "ha_version": "2026.10.0"}),
        ))
        .await;
    let mut decoder = Decoder::new(1 << 20);
    let mut buf = [0u8; 4096];
    loop {
        tokio::select! {
            read = stream.read(&mut buf) => {
                let Ok(n) = read else { return };
                if n == 0 { return; }
                let Ok(messages) = decoder.push(&buf[..n]) else { return };
                for m in messages {
                    if m.opcode == Opcode::Close { return; }
                    let Ok(v) = serde_json::from_slice::<Json>(&m.payload) else { return };
                    let id = v.get("id").cloned().unwrap_or(Json::Null);
                    let reply = match v.get("type").and_then(Json::as_str).unwrap_or("") {
                        "auth" => {
                            if v.get("access_token").and_then(Json::as_str) == Some(TOKEN) {
                                Some(json!({"type": "auth_ok"}))
                            } else {
                                let _ = stream.write_all(&send(json!({"type": "auth_invalid", "message": "Invalid password"}))).await;
                                return;
                            }
                        }
                        "subscribe_events" => {
                            state.lock().unwrap().clients.push(tx.clone());
                            Some(json!({"id": id, "type": "result", "success": true, "result": null}))
                        }
                        "get_states" => {
                            let list: Vec<Json> = state.lock().unwrap().entities.iter()
                                .map(|(e, (s, n))| json!({"entity_id": e, "state": s, "attributes": {"friendly_name": n}}))
                                .collect();
                            Some(json!({"id": id, "type": "result", "success": true, "result": list}))
                        }
                        "ping" => {
                            if state.lock().unwrap().mute_pings { None } else { Some(json!({"id": id, "type": "pong"})) }
                        }
                        _ => None,
                    };
                    if let Some(r) = reply {
                        if stream.write_all(&send(r)).await.is_err() { return; }
                    }
                }
            }
            push = rx.recv() => match push {
                Some(Push::Frame(f)) => { if stream.write_all(&f).await.is_err() { return; } }
                Some(Push::Drop) | None => return,
            },
        }
    }
}

fn secrets() -> MemorySecrets {
    let s = MemorySecrets::new();
    s.set("ha_token", TOKEN);
    s
}

fn config(ha: &Ha) -> HaConfig {
    let mut c = HaConfig::new(
        ha.addr.to_string(),
        "ha_token",
        vec![
            "switch.radio_plug".into(),
            "switch.amp_plug".into(),
            "binary_sensor.door".into(),
        ],
    );
    c.request_timeout = Duration::from_secs(2);
    c.confirm_wait = Duration::from_millis(600);
    c.ping_every = Duration::from_millis(300);
    c.backoff = Backoff {
        initial: Duration::from_millis(30),
        max: Duration::from_millis(120),
        factor: 2,
    };
    c
}

async fn until(what: &str, mut check: impl FnMut() -> bool) {
    timeout(Duration::from_secs(10), async {
        while !check() {
            sleep(Duration::from_millis(15)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("never happened: {what}"));
}

fn driver(ha: &Ha, audit: &Arc<MemoryAudit>) -> Arc<HomeAssistant> {
    Arc::new(
        HomeAssistant::start(
            &Handle::current(),
            config(ha),
            &secrets(),
            Arc::clone(audit) as Arc<dyn AuditLog>,
        )
        .expect("start"),
    )
}

const DEF: &str = r#"
[[channels]]
id = "radio_pwr"
type = "power"
driver = "ha"
line = 0
safe = "off"
[[channels]]
id = "amp_pwr"
type = "power"
driver = "ha"
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
external_deadman = true
"#;

fn ch(s: &str) -> ChannelId {
    ChannelId(s.into())
}

fn board(driver: &Arc<HomeAssistant>) -> Switchboard {
    let def = StationDef::from_toml(DEF).expect("def");
    let mut b = Switchboard::new();
    for c in def.channels {
        b.add_channel(c, Arc::clone(driver) as Arc<dyn ChannelDriver>)
            .expect("channel");
    }
    b
}

// Tests [@ANCHOR: radioctl_station_ha_start]
// Tests [@ANCHOR: radioctl_station_ha_run_sessions]
// Tests [@ANCHOR: radioctl_station_ha_session]
// Tests [@ANCHOR: radioctl_station_ha_read_head]
// Tests [@ANCHOR: radioctl_station_ha_send_json]
// Tests [@ANCHOR: radioctl_station_ha_is_synced]
// Tests [@ANCHOR: radioctl_station_ha_discover]
// Tests [@ANCHOR: radioctl_station_ha_entity]
// Tests [@ANCHOR: radioctl_station_ha_apply_state]
// Tests [@ANCHOR: radioctl_station_ha_set_synced]
// Tests [@ANCHOR: radioctl_station_ha_state_of]
// Tests [@ANCHOR: radioctl_station_ha_record]
// Tests [@ANCHOR: radioctl_station_ha_post]
// Tests [@ANCHOR: radioctl_station_ha_write_line]
// Tests [@ANCHOR: radioctl_station_ha_read_line]
// Tests [@ANCHOR: radioctl_station_ha_can_read_back]
// Tests [@ANCHOR: radioctl_station_ha_assurance]
// Tests [@ANCHOR: radioctl_station_ha_drop]
// Tests [@ANCHOR: radioctl_station_ha_config_new]
// Tests [@ANCHOR: radioctl_station_ha_memsecrets_new]
// Tests [@ANCHOR: radioctl_station_ha_memsecrets_set]
// Tests [@ANCHOR: radioctl_station_ha_memsecrets_secret]
// Tests [@ANCHOR: radioctl_station_ha_memaudit_new]
// Tests [@ANCHOR: radioctl_station_ha_memaudit_entries]
// Tests [@ANCHOR: radioctl_station_ha_memaudit_record]
// Tests [@ANCHOR: radioctl_station_ha_lock]
// Tests [@ANCHOR: radioctl_station_ha_unix_now]
// Tests [@ANCHOR: radioctl_station_ha_driver_err]
// Tests [@ANCHOR: radioctl_station_driver_assurance]
// Tests [@ANCHOR: radioctl_station_board_assurance]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discovery_switching_and_confirmation_by_event() {
    let mut ha = Ha::start().await;
    let audit = Arc::new(MemoryAudit::new());
    let d = driver(&ha, &audit);
    until("synced", || d.is_synced()).await;
    assert_eq!(d.assurance(), Assurance::Mediated);
    assert!(d.can_read_back());

    // Discovery offers switches and the sensor, not the lock; it marks what is already a line.
    let found = d.discover().expect("discover");
    let ids: Vec<&str> = found.iter().map(|f| f.entity_id.as_str()).collect();
    assert_eq!(
        ids,
        ["binary_sensor.door", "switch.amp_plug", "switch.radio_plug"]
    );
    assert!(found.iter().all(|f| f.mapped));
    assert!(!found[0].writable && found[1].writable);
    assert_eq!(
        found[1].friendly_name.as_deref(),
        Some("Name of switch.amp_plug")
    );

    // A power channel through the driver: commanded, sent with the bearer token, confirmed by event.
    let b = Arc::new(board(&d));
    let station = Station::new(
        StationDef::from_toml(DEF).unwrap(),
        Arc::clone(&b),
        vec![],
        None,
    )
    .expect("station");
    assert_eq!(b.assurance(&ch("radio_pwr")).unwrap(), Assurance::Mediated);
    station
        .set_channel(&ch("radio_pwr"), Value::On)
        .await
        .expect("set on");
    assert_eq!(
        b.status(&ch("radio_pwr")).unwrap().confirmed,
        Some(Value::On)
    );
    let calls = ha.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "/api/services/switch/turn_on");
    assert!(calls[0].1.contains("switch.radio_plug"));
    assert!(calls[0].2, "the call carried the bearer token");
    station
        .set_channel(&ch("radio_pwr"), Value::Off)
        .await
        .expect("set off");
    assert_eq!(
        b.status(&ch("radio_pwr")).unwrap().confirmed,
        Some(Value::Off)
    );

    // The audit log has the calls and never the token.
    let entries = audit.entries();
    assert!(entries.iter().any(|e| e.action == "service switch.turn_on"
        && e.entity.as_deref() == Some("switch.radio_plug")
        && e.result == "ok"));
    assert!(entries
        .iter()
        .any(|e| e.action == "websocket auth" && e.result == "ok"));
    assert!(entries.iter().any(|e| e.action == "discover"));
    assert!(!format!("{entries:?}").contains(TOKEN));
    drop(station);
    drop(b);
    drop(d);
    ha.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unavailable_entity_is_unknown_and_blocks_what_depends_on_it() {
    let mut ha = Ha::start().await;
    let audit = Arc::new(MemoryAudit::new());
    let d = driver(&ha, &audit);
    until("synced", || d.is_synced()).await;
    let b = Arc::new(board(&d));
    let def = StationDef::from_toml(DEF).unwrap();
    let station = Station::new(def, Arc::clone(&b), vec![], None).expect("station");
    assert_eq!(
        station.power_assurance(&DeviceId("amp".into())),
        Some(Assurance::Mediated)
    );

    // The radio's plug goes unavailable (the plug lost its WiFi): its state is unknown.
    ha.set_state("switch.radio_plug", "unavailable");
    until("unknown", || {
        d.discover().is_ok_and(|f| {
            f.iter()
                .any(|x| x.entity_id == "switch.radio_plug" && x.state == "unavailable")
        })
    })
    .await;
    let err = b.read_back(&ch("radio_pwr")).await.expect_err("unknown");
    assert_eq!(err.kind, StationErrorKind::Driver);
    assert!(err.detail.contains("unknown"), "{}", err.detail);
    assert_eq!(b.status(&ch("radio_pwr")).unwrap().confirmed, None);
    // The amplifier depends on the radio being confirmed on: not even tried.
    let before = ha.calls().len();
    let refused = station
        .power_on(&DeviceId("amp".into()))
        .await
        .expect_err("blocked");
    assert_eq!(refused.kind, StationErrorKind::Refused);
    assert_eq!(
        ha.calls().len(),
        before,
        "nothing was sent to Home Assistant"
    );

    // It comes back, on: the dependency is satisfied from the event, and the amplifier powers on.
    ha.set_state("switch.radio_plug", "on");
    until("radio on in cache", || {
        d.discover().is_ok_and(|f| {
            f.iter()
                .any(|x| x.entity_id == "switch.radio_plug" && x.state == "on")
        })
    })
    .await;
    b.read_back(&ch("radio_pwr")).await.expect("known again");
    station
        .power_on(&DeviceId("amp".into()))
        .await
        .expect("amp on");
    drop(station);
    drop(b);
    drop(d);
    ha.stop().await;
}

// Tests [@ANCHOR: radioctl_station_station_new]
// Tests [@ANCHOR: radioctl_station_station_power_assurance]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_amplifier_through_home_assistant_needs_an_independent_stop() {
    let mut ha = Ha::start().await;
    let audit = Arc::new(MemoryAudit::new());
    let d = driver(&ha, &audit);
    let b = Arc::new(board(&d));
    let without = DEF.replace("external_deadman = true\n", "");
    let def = StationDef::from_toml(&without).unwrap();
    let error = match Station::new(def, Arc::clone(&b), vec![], None) {
        Ok(_) => panic!("an amplifier on a mediated channel with no declared stop"),
        Err(e) => e,
    };
    assert_eq!(error.kind, StationErrorKind::Config);
    assert!(
        error.detail.contains("external_deadman"),
        "{}",
        error.detail
    );
    drop(b);
    drop(d);
    ha.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ptt_never_goes_through_home_assistant() {
    let mut ha = Ha::start().await;
    let audit = Arc::new(MemoryAudit::new());
    let d = driver(&ha, &audit);
    let mut b = Switchboard::new();
    let ptt = r#"
[[channels]]
id = "ptt1"
type = "ptt"
driver = "ha"
line = 0
safe = "off"
max_on_ms = 1000
[[devices]]
id = "x"
kind = "radio"
power = { source = "always_on" }
"#;
    let def = StationDef::from_toml(ptt).expect("a ptt channel is valid in the model");
    let err = b
        .add_channel(
            def.channels[0].clone(),
            Arc::clone(&d) as Arc<dyn ChannelDriver>,
        )
        .expect_err("refused");
    assert_eq!(err.kind, StationErrorKind::Config);
    assert!(err.detail.contains("ptt"), "{}", err.detail);
    drop(b);
    drop(d);
    ha.stop().await;
}

// Tests [@ANCHOR: radioctl_station_ha_write_line]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_command_that_is_not_confirmed_is_a_mismatch_and_a_sensor_cannot_be_switched() {
    let mut ha = Ha::start().await;
    ha.state.lock().unwrap().ignore_commands = true;
    let audit = Arc::new(MemoryAudit::new());
    let d = driver(&ha, &audit);
    until("synced", || d.is_synced()).await;
    let b = Arc::new(board(&d));
    let station = Station::new(
        StationDef::from_toml(DEF).unwrap(),
        Arc::clone(&b),
        vec![],
        None,
    )
    .expect("station");
    // Home Assistant accepts the call and the device does nothing: no event, the state stays off.
    let err = station
        .set_channel(&ch("radio_pwr"), Value::On)
        .await
        .expect_err("not confirmed");
    assert!(
        matches!(err.kind, StationErrorKind::NotConfirmed),
        "{err:?}"
    );
    assert_eq!(
        b.status(&ch("radio_pwr")).unwrap().confirmed,
        Some(Value::Off)
    );
    drop(station);
    // Line 2 is a binary sensor: it can be read, never switched.
    let sensor = d.write_line(2, true).await.expect_err("sensor");
    assert_eq!(sensor.kind, StationErrorKind::Refused);
    assert!(d.write_line(9, true).await.is_err());
    assert!(!d.read_line(2).await.unwrap());
    ha.set_state("binary_sensor.door", "on");
    until("door open", || {
        d.discover().is_ok_and(|f| {
            f.iter()
                .any(|x| x.entity_id == "binary_sensor.door" && x.state == "on")
        })
    })
    .await;
    assert!(d.read_line(2).await.unwrap());
    drop(b);
    drop(d);
    ha.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn losing_the_session_makes_every_state_unknown_and_it_recovers_with_fresh_state() {
    let mut ha = Ha::start().await;
    let audit = Arc::new(MemoryAudit::new());
    let d = driver(&ha, &audit);
    until("synced", || d.is_synced()).await;
    assert!(!d.read_line(0).await.unwrap());

    ha.drop_websockets();
    until("unsynced", || !d.is_synced()).await;
    assert!(d.read_line(0).await.is_err());
    assert!(d.discover().is_err());
    // A change while the session is down is seen after the reconnect.
    ha.state
        .lock()
        .unwrap()
        .entities
        .get_mut("switch.radio_plug")
        .unwrap()
        .0 = "on".into();
    until("resynced", || d.is_synced()).await;
    assert!(d.read_line(0).await.unwrap());
    assert!(ha.ws_connections() >= 2);
    assert!(audit.entries().iter().any(|e| e.action == "websocket lost"));

    // A command is still sent while the state is unknown: an off must always be tried.
    ha.drop_websockets();
    until("unsynced again", || !d.is_synced()).await;
    d.write_line(0, false).await.expect("the call goes out");
    assert!(ha.calls().iter().any(|c| c.0.ends_with("turn_off")));
    drop(d);
    ha.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_silent_home_assistant_is_noticed_by_ping() {
    let mut ha = Ha::start().await;
    let audit = Arc::new(MemoryAudit::new());
    let d = driver(&ha, &audit);
    until("synced", || d.is_synced()).await;
    ha.state.lock().unwrap().mute_pings = true;
    until("ping timeout drops the session", || !d.is_synced()).await;
    assert!(audit
        .entries()
        .iter()
        .any(|e| e.action == "websocket lost" && e.result.contains("pings")));
    ha.state.lock().unwrap().mute_pings = false;
    until("recovers", || d.is_synced()).await;
    drop(d);
    ha.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrong_token_never_syncs_and_is_never_logged() {
    let mut ha = Ha::start().await;
    let audit = Arc::new(MemoryAudit::new());
    let wrong = MemorySecrets::new();
    wrong.set("ha_token", "not-the-token-0000");
    let d = Arc::new(
        HomeAssistant::start(
            &Handle::current(),
            config(&ha),
            &wrong,
            Arc::clone(&audit) as Arc<dyn AuditLog>,
        )
        .expect("start"),
    );
    until("the rejection is audited", || {
        audit
            .entries()
            .iter()
            .any(|e| e.action == "websocket auth" && e.result.contains("rejected"))
    })
    .await;
    assert!(!d.is_synced());
    let err = d.write_line(0, true).await.expect_err("401");
    assert!(err.detail.contains("rejected"), "{}", err.detail);
    let everything = format!("{:?} {err:?} {}", audit.entries(), err.detail);
    assert!(!everything.contains("not-the-token"));
    assert!(!everything.contains(TOKEN));
    drop(d);
    ha.stop().await;
}

// Tests [@ANCHOR: radioctl_station_ha_post]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rejected_service_call_is_an_error_and_an_audit_entry() {
    let mut ha = Ha::start().await;
    let audit = Arc::new(MemoryAudit::new());
    let d = driver(&ha, &audit);
    until("synced", || d.is_synced()).await;
    ha.state.lock().unwrap().reject_rest = true;
    let err = d.write_line(1, true).await.expect_err("rejected");
    assert!(
        err.detail.contains("rejected the access token"),
        "{}",
        err.detail
    );
    assert!(audit
        .entries()
        .iter()
        .any(|e| e.action == "service switch.turn_on" && e.result.starts_with("error: ")));
    ha.stop().await;
    let down = d.write_line(1, true).await.expect_err("unreachable");
    assert_eq!(down.kind, StationErrorKind::Driver);
    drop(d);
}

// Tests [@ANCHOR: radioctl_station_ha_parse_address]
// Tests [@ANCHOR: radioctl_station_ha_check_entity]
// Tests [@ANCHOR: radioctl_station_ha_missing_secret]
// Tests [@ANCHOR: radioctl_station_ha_filesecrets_new]
// Tests [@ANCHOR: radioctl_station_ha_filesecrets_secret]
// Tests [@ANCHOR: radioctl_station_ha_secret_new]
#[tokio::test]
async fn start_refuses_what_it_should_before_touching_the_network() {
    let audit: Arc<dyn AuditLog> = Arc::new(MemoryAudit::new());
    let ok = secrets();
    let start = |c: HaConfig, s: &dyn SecretStore| {
        HomeAssistant::start(&Handle::current(), c, s, Arc::clone(&audit)).map(|_| ())
    };
    let base = || HaConfig::new("127.0.0.1:1", "ha_token", vec!["switch.a".into()]);
    for bad_address in ["8.8.8.8", "homeassistant.local", "203.0.113.5:8123"] {
        let mut c = base();
        c.address = bad_address.into();
        assert_eq!(
            start(c, &ok).unwrap_err().kind,
            StationErrorKind::Config,
            "{bad_address}"
        );
    }
    let mut c = base();
    c.entities = vec!["lock.front".into()];
    assert!(start(c, &ok).is_err());
    let mut c = base();
    c.entities = vec![];
    assert!(start(c, &ok).is_err());
    let empty = MemorySecrets::new();
    let e = start(base(), &empty).unwrap_err();
    assert!(
        e.detail.contains("ha_token") && !e.detail.contains(TOKEN),
        "{}",
        e.detail
    );
    assert!(parse_ha_address("10.1.2.3").is_ok() && check_entity("switch.x").is_ok());

    // File secrets: mode 0600 only, plain names only.
    let dir = std::env::temp_dir().join(format!("rs-ha-secrets-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = FileSecrets::new(&dir);
    std::fs::write(dir.join("ha_token"), format!("{TOKEN}\n")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir.join("ha_token"), std::fs::Permissions::from_mode(0o644)).unwrap();
    let e = store.secret("ha_token").unwrap_err();
    assert!(e.detail.contains("mode 0600") && !e.detail.contains(TOKEN));
    std::fs::set_permissions(dir.join("ha_token"), std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(store.secret("ha_token").unwrap().expose(), TOKEN);
    for bad in ["", "../ha_token", "a/b", ".hidden", "no_such"] {
        assert!(store.secret(bad).is_err(), "{bad:?}");
    }
    std::fs::write(dir.join("blank"), "\n").unwrap();
    std::fs::set_permissions(dir.join("blank"), std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(store.secret("blank").is_err());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = Secret::new("x");
}
