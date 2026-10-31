// SPDX-License-Identifier: AGPL-3.0-or-later
//! The self-test's report upload to hams.com (docs/RIG_REPORT_UPLOAD.md is the contract).
//!
//! At the end of every run, unless `--no-upload`, the program POSTs one JSON document: what was
//! tested, the check results and (capped) raw bytes, the radioctl version, OS and architecture,
//! and the contact name and email the volunteer gave. Never sent: host name, user name, file paths,
//! IP address, USB serial number. A failed upload is one line on the screen and never changes the
//! test's exit status. Nothing here can transmit on the air.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;

use crate::selftest::{utc_stamp, CheckResult, Verdict};

/// Where reports go unless `--server` says otherwise (tests always do).
pub const DEFAULT_URL: &str = "https://hams.com/api/rig-reports";
/// The largest request body sent, in bytes.
pub const MAX_BODY: usize = 256 * 1024;
/// The most raw hexadecimal characters sent for one check.
pub const MAX_RAW_HEX: usize = 2048;
/// The whole upload gives up after this long.
pub const UPLOAD_TIMEOUT: Duration = Duration::from_secs(10);

/// What the volunteer chose to tell hams.com so it can contact them about the rig.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Contact {
    /// Name, if given.
    pub name: Option<String>,
    /// Email address, if given.
    pub email: Option<String>,
}

/// Everything the payload is built from. It has no field for a path, serial number or host name.
#[derive(Debug, Clone)]
pub struct PayloadInput<'a> {
    /// A random UUID v4 made for this run.
    pub report_id: String,
    /// Unix time of the upload.
    pub sent_at: u64,
    /// The rig description id.
    pub description_id: String,
    /// The description's display name, such as `Icom IC-7300`.
    pub rig_name: String,
    /// `serial`, `usb` or `virtual`.
    pub transport: &'static str,
    /// The baud rate used.
    pub baud: u32,
    /// USB vendor and product id as `vvvv:pppp`, when known.
    pub usb_vid_pid: Option<String>,
    /// The rates the baud probe opened (winner last), when it ran.
    pub probe_tried: Option<Vec<u32>>,
    /// The checks that ran.
    pub results: &'a [CheckResult],
    /// The contact details given, if any.
    pub contact: Contact,
}

// [@ANCHOR: radioctld_upload_uuid_v4]
/// Formats 16 random bytes as a version 4 UUID.
pub fn uuid_v4(mut bytes: [u8; 16]) -> String {
    bytes[6] = bytes[6] & 0x0f | 0x40;
    bytes[8] = bytes[8] & 0x3f | 0x80;
    let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let h = hex.concat();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

// [@ANCHOR: radioctld_upload_new_report_id]
/// A fresh random report id, or an error if the system has no randomness.
pub fn new_report_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| format!("no random numbers: {e}"))?;
    Ok(uuid_v4(bytes))
}

// [@ANCHOR: radioctld_upload_rfc3339]
/// `YYYY-MM-DDTHH:MM:SSZ` for a Unix time.
pub fn rfc3339(unix_seconds: u64) -> String {
    utc_stamp(unix_seconds)
        .replacen(' ', "T", 1)
        .trim_end_matches(" UTC")
        .to_string()
        + "Z"
}

/// The first `max` characters of `text`.
fn cap(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

// [@ANCHOR: radioctld_upload_build_payload]
/// The upload document, at most [`MAX_BODY`] bytes: raw bytes are dropped from the last check
/// backwards, then summaries shortened, until it fits. Contact fields appear only when given.
pub fn build_payload(input: &PayloadInput<'_>) -> Vec<u8> {
    let (maker, model) = input
        .rig_name
        .split_once(' ')
        .unwrap_or(("", input.rig_name.as_str()));
    let firmware = input
        .results
        .iter()
        .find(|r| r.name == "identify: firmware")
        .and_then(|r| match &r.verdict {
            Verdict::Pass(text) => Some(cap(text, 80)),
            _ => None,
        });
    let mut checks: Vec<Value> = input
        .results
        .iter()
        .map(|r| {
            let (ok, summary) = match &r.verdict {
                Verdict::Pass(s) => (true, s.as_str()),
                Verdict::NotApplicable(s) | Verdict::Fail(s) => (false, s.as_str()),
            };
            let mut check =
                json!({"name": cap(&r.name, 80), "ok": ok, "summary": cap(summary, 300)});
            if !r.wire.is_empty() {
                check["raw_hex"] = json!(cap(&r.wire, MAX_RAW_HEX));
            }
            check
        })
        .collect();
    let mut contact = serde_json::Map::new();
    if let Some(name) = &input.contact.name {
        contact.insert("name".into(), json!(cap(name, 120)));
    }
    if let Some(email) = &input.contact.email {
        contact.insert("email".into(), json!(cap(email, 254)));
    }
    let mut link = json!({"transport": input.transport, "baud": input.baud});
    if let Some(ids) = &input.usb_vid_pid {
        link["usb_vid_pid"] = json!(ids);
    }
    let mut doc = json!({
        "schema_version": 1,
        "report_id": input.report_id,
        "sent_at": rfc3339(input.sent_at),
        "radioctl_version": env!("CARGO_PKG_VERSION"),
        "description_id": input.description_id,
        "rig": {"manufacturer": maker, "model": model, "firmware_or_version": firmware},
        "host": {"os": std::env::consts::OS, "arch": std::env::consts::ARCH},
        "link": link,
        "checks": [],
        "contact": contact,
    });
    if let Some(tried) = &input.probe_tried {
        doc["baud_probe"] = json!({"tried": tried, "chosen": tried.last()});
    }
    let mut stage = 0;
    loop {
        doc["checks"] = Value::Array(checks.clone());
        let body = serde_json::to_vec(&doc).unwrap_or_default();
        if body.len() <= MAX_BODY || stage > checks.len() * 2 {
            return body;
        }
        // Drop raw bytes from the end first, then shorten summaries from the end.
        let (index, shorten) = (
            checks.len() - 1 - stage % checks.len(),
            stage >= checks.len(),
        );
        if let Some(check) = checks.get_mut(index).and_then(Value::as_object_mut) {
            if shorten {
                check.insert("summary".into(), json!(""));
            } else {
                check.remove("raw_hex");
            }
        }
        stage += 1;
    }
}

/// What is kept between runs: whether the volunteer was asked, and the answers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedContact {
    /// The saved contact details (either may be absent: Enter skips).
    pub contact: Contact,
}

// [@ANCHOR: radioctld_upload_config_dir]
/// The per-user configuration directory for radioctl: `$XDG_CONFIG_HOME` or `~/.config` on
/// Linux, `~/Library/Application Support` on macOS, `%APPDATA%` on Windows, plus `radioctl`.
/// `var` reads an environment variable (injected so tests do not touch the real one).
pub fn config_dir(var: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let base = if cfg!(windows) {
        PathBuf::from(var("APPDATA")?)
    } else if cfg!(target_os = "macos") {
        PathBuf::from(var("HOME")?).join("Library/Application Support")
    } else {
        match var("XDG_CONFIG_HOME").filter(|d| !d.is_empty()) {
            Some(dir) => PathBuf::from(dir),
            None => PathBuf::from(var("HOME")?).join(".config"),
        }
    };
    Some(base.join("radioctl"))
}

// [@ANCHOR: radioctld_upload_load_saved]
/// The saved contact answers, or `None` when the volunteer has not been asked yet.
pub fn load_saved(path: &Path) -> Option<SavedContact> {
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let get = |k: &str| doc.get(k).and_then(Value::as_str).map(str::to_string);
    Some(SavedContact {
        contact: Contact {
            name: get("name"),
            email: get("email"),
        },
    })
}

// [@ANCHOR: radioctld_upload_save]
/// Saves the answers in a file only the owner can read (mode 0600 where the system has modes).
pub fn save(path: &Path, saved: &SavedContact) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let doc = json!({"name": saved.contact.name, "email": saved.contact.email});
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(path)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    file.write_all(doc.to_string().as_bytes())
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

// [@ANCHOR: radioctld_upload_valid_email]
/// A plain shape check (something, `@`, a dotted domain, no spaces); it does not verify the address.
pub fn valid_email(text: &str) -> bool {
    match text.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && text.len() <= 254
                && !text.chars().any(char::is_whitespace)
        }
        None => false,
    }
}

// [@ANCHOR: radioctld_upload_prompt_contact]
/// Asks for the name, then the email, saying why. Enter skips either; an email that does not look
/// like one is skipped with a message. Reads lines from `input`, writes the prompt to `output`.
pub fn prompt_contact(input: &mut dyn BufRead, output: &mut dyn Write) -> SavedContact {
    let mut ask = |question: &str| -> Option<String> {
        let _ = write!(output, "{question}");
        let _ = output.flush();
        let mut line = String::new();
        input.read_line(&mut line).ok()?;
        let answer = line.trim().to_string();
        (!answer.is_empty()).then_some(answer)
    };
    let name = ask(
        "So hams.com can contact you about your rig (sent with the report, asked only once),\nwhat is your name? Press Enter to skip: ",
    );
    let email = ask("And your email address? Press Enter to skip: ").filter(|e| valid_email(e));
    SavedContact {
        contact: Contact { name, email },
    }
}

// [@ANCHOR: radioctld_upload_resolve_contact]
/// The contact to send. A flag beats an environment variable beats the saved answers. The
/// volunteer is asked (once, ever) only when `interactive`, nothing was supplied and nothing is
/// saved; the answers to save come back as `Some`. A non-interactive run never asks.
pub fn resolve_contact(
    flags: Contact,
    env: Contact,
    saved: Option<SavedContact>,
    interactive: bool,
    ask: &mut dyn FnMut() -> SavedContact,
) -> (Contact, Option<SavedContact>) {
    let supplied =
        flags.name.is_some() || flags.email.is_some() || env.name.is_some() || env.email.is_some();
    let (base, to_save) = match saved {
        Some(saved) => (saved.contact, None),
        None if interactive && !supplied => {
            let answers = ask();
            (answers.contact.clone(), Some(answers))
        }
        None => (Contact::default(), None),
    };
    let contact = Contact {
        name: flags.name.or(env.name).or(base.name),
        email: flags.email.or(env.email).or(base.email),
    };
    (contact, to_save)
}

/// Where to POST: parsed `--server` or default URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerUrl {
    /// True for `https`.
    pub tls: bool,
    /// Host name or address (no brackets).
    pub host: String,
    /// TCP port.
    pub port: u16,
    /// Path and query, starting with `/`.
    pub path: String,
}

// [@ANCHOR: radioctld_upload_parse_server]
/// `https://HOST[:PORT]/PATH`, or `http://` only for a loopback host (`localhost`, `127.0.0.1`,
/// `[::1]`), so a report can never go in the clear to another machine.
pub fn parse_server(url: &str) -> Result<ServerUrl, String> {
    let (tls, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (false, rest)
    } else {
        return Err(format!("{url}: the server URL must start with https://"));
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = if let Some(bracketed) = authority.strip_prefix('[') {
        let (host, tail) = bracketed
            .split_once(']')
            .ok_or_else(|| format!("{url}: bad address"))?;
        (host.to_string(), tail.strip_prefix(':').map(str::to_string))
    } else {
        match authority.split_once(':') {
            Some((h, p)) => (h.to_string(), Some(p.to_string())),
            None => (authority.to_string(), None),
        }
    };
    let port = match port {
        Some(p) => p.parse().map_err(|_| format!("{url}: bad port"))?,
        None if tls => 443,
        None => 80,
    };
    if host.is_empty() || host.contains(char::is_whitespace) {
        return Err(format!("{url}: no host"));
    }
    if !tls && !matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1") {
        return Err(format!(
            "{url}: plain http is only allowed to this machine; use https"
        ));
    }
    Ok(ServerUrl {
        tls,
        host,
        port,
        path: path.to_string(),
    })
}

/// The status code in an HTTP response head, such as `HTTP/1.1 204 No Content`.
fn status_code(head: &str) -> Option<u16> {
    head.lines().next()?.split_whitespace().nth(1)?.parse().ok()
}

// [@ANCHOR: radioctld_upload_decode_body]
/// The body of a complete HTTP/1.1 response read to the end of the connection: the bytes after
/// the head, with chunked transfer coding undone when the head says it is used. `None` when the
/// response has no head or a broken chunk.
pub fn decode_body(response: &[u8]) -> Option<(u16, Vec<u8>)> {
    let split = response.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(response.get(..split)?).to_string();
    let code = status_code(&head)?;
    let raw = response.get(split + 4..)?;
    if !head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        return Some((code, raw.to_vec()));
    }
    let (mut body, mut rest) = (Vec::new(), raw);
    loop {
        let eol = rest.windows(2).position(|w| w == b"\r\n")?;
        let size_text = String::from_utf8_lossy(rest.get(..eol)?).to_string();
        let size = usize::from_str_radix(size_text.split(';').next()?.trim(), 16).ok()?;
        rest = rest.get(eol + 2..)?;
        if size == 0 {
            return Some((code, body));
        }
        body.extend_from_slice(rest.get(..size)?);
        rest = rest.get(size + 2..)?;
    }
}

// [@ANCHOR: radioctld_upload_exchange]
/// One request and the status and body it earned; any network trouble is an `Err` with a short
/// reason. At most `max_response` bytes of answer are read.
async fn exchange(
    server: &ServerUrl,
    method: &str,
    body: &[u8],
    max_response: usize,
) -> Result<(u16, Vec<u8>), String> {
    let tcp = TcpStream::connect((server.host.as_str(), server.port))
        .await
        .map_err(|e| format!("cannot connect: {e}"))?;
    let head = format!(
        "{method} {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nUser-Agent: radioctl-selftest/{}\r\nConnection: close\r\n\r\n",
        server.path,
        server.host,
        body.len(),
        env!("CARGO_PKG_VERSION")
    );
    let mut response = Vec::new();
    if server.tls {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let provider = Arc::new(tokio_rustls::rustls::crypto::ring::default_provider());
        let config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| format!("TLS setup: {e}"))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let name = ServerName::try_from(server.host.clone()).map_err(|e| format!("host: {e}"))?;
        let mut stream = TlsConnector::from(Arc::new(config))
            .connect(name, tcp)
            .await
            .map_err(|e| format!("TLS: {e}"))?;
        stream
            .write_all(head.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        stream.write_all(body).await.map_err(|e| e.to_string())?;
        let _ = (&mut stream)
            .take(max_response as u64 + 65_536)
            .read_to_end(&mut response)
            .await;
    } else {
        let mut stream = tcp;
        stream
            .write_all(head.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        stream.write_all(body).await.map_err(|e| e.to_string())?;
        let _ = (&mut stream)
            .take(max_response as u64 + 65_536)
            .read_to_end(&mut response)
            .await;
    }
    decode_body(&response).ok_or_else(|| "no HTTP answer".to_string())
}

// [@ANCHOR: radioctld_upload_request]
/// `method` (`POST` with a JSON `body`, or `GET` with none) to `url` within `timeout`; returns the
/// status and body (at most `max_response` bytes). `url` follows [`parse_server`]'s rules. Any
/// failure is an `Err` holding one short line.
pub async fn request(
    method: &str,
    url: &str,
    body: &[u8],
    max_response: usize,
    timeout: Duration,
) -> Result<(u16, Vec<u8>), String> {
    let server = parse_server(url)?;
    match tokio::time::timeout(timeout, exchange(&server, method, body, max_response)).await {
        Err(_) => Err(format!("timed out after {} s", timeout.as_secs())),
        Ok(answer) => answer,
    }
}

// [@ANCHOR: radioctld_upload_post_report]
/// Sends `body` to `url` within [`UPLOAD_TIMEOUT`]. `Ok` only for a 2xx answer; every failure
/// (bad URL, offline, TLS, timeout, 4xx, 5xx) is an `Err` holding one short line.
pub async fn post_report(url: &str, body: &[u8]) -> Result<(), String> {
    match request("POST", url, body, 4096, UPLOAD_TIMEOUT).await? {
        (code, _) if (200..300).contains(&code) => Ok(()),
        (code, _) => Err(format!("the server answered {code}")),
    }
}

// [@ANCHOR: radioctld_upload_notice]
/// What is said before uploading: what is sent, to where, and where the local copy is.
pub fn notice(url: &str, local_copy: &Path, contact: &Contact) -> String {
    let who = match (&contact.name, &contact.email) {
        (None, None) => "no contact details".to_string(),
        _ => "your name and email as you gave them".to_string(),
    };
    format!(
        "Uploading to {url}: the check results, the bytes exchanged with the radio, the radioctl\nversion, this computer's OS and CPU type, and {who}. No host name, user name, file path or\nUSB serial number is sent. Your local copy: {}\n",
        local_copy.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn result(name: &str, verdict: Verdict, wire: &str) -> CheckResult {
        CheckResult {
            name: name.into(),
            op: "GetModelId".into(),
            verdict,
            millis: 3,
            wire: wire.into(),
        }
    }

    fn input(results: &[CheckResult], contact: Contact) -> PayloadInput<'_> {
        PayloadInput {
            report_id: "00000000-0000-4000-8000-000000000000".into(),
            sent_at: 0,
            description_id: "icom-ic-7300".into(),
            rig_name: "Icom IC-7300".into(),
            transport: "usb",
            baud: 19_200,
            usb_vid_pid: Some("10c4:ea60".into()),
            probe_tried: Some(vec![4800, 9600]),
            results,
            contact,
        }
    }

    // Tests [@ANCHOR: radioctld_upload_uuid_v4]
    // Tests [@ANCHOR: radioctld_upload_new_report_id]
    #[test]
    fn report_ids_are_version_4_uuids_and_differ() {
        assert_eq!(uuid_v4([0xff; 16]), "ffffffff-ffff-4fff-bfff-ffffffffffff");
        let (a, b) = (new_report_id().expect("a"), new_report_id().expect("b"));
        assert_ne!(a, b);
        assert_eq!((a.len(), a.as_bytes()[14]), (36, b'4'));
    }

    // Tests [@ANCHOR: radioctld_upload_rfc3339]
    #[test]
    fn times_are_rfc3339() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
    }

    // Tests [@ANCHOR: radioctld_upload_build_payload]
    #[test]
    fn the_payload_has_the_contract_fields_and_nothing_private() {
        let results = [
            result(
                "identify: firmware",
                Verdict::Pass("Firmware(1, 2)".into()),
                "0.1 tx fe fe",
            ),
            result("read mode", Verdict::Fail("Timeout".into()), ""),
        ];
        let body = build_payload(&input(&results, Contact::default()));
        let text = String::from_utf8(body).expect("utf8");
        let doc: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(doc["schema_version"], 1);
        assert_eq!(doc["rig"]["manufacturer"], "Icom");
        assert_eq!(doc["rig"]["model"], "IC-7300");
        assert_eq!(doc["rig"]["firmware_or_version"], "Firmware(1, 2)");
        assert_eq!(doc["link"]["usb_vid_pid"], "10c4:ea60");
        assert_eq!(doc["checks"][0]["raw_hex"], "0.1 tx fe fe");
        assert_eq!(doc["checks"][1]["ok"], false);
        assert_eq!(doc["baud_probe"]["tried"][1], 9600);
        assert_eq!(doc["contact"], json!({}));
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home".into());
        for private in ["/dev/", "ABC123", home.as_str(), "serial"] {
            assert!(!text.contains(private), "{private} in {text}");
        }
    }

    // Tests [@ANCHOR: radioctld_upload_build_payload]
    #[test]
    fn contact_fields_appear_only_when_given_and_the_size_is_capped() {
        let big = "ab ".repeat(2000);
        let results: Vec<CheckResult> = (0..400)
            .map(|i| result(&format!("c{i}"), Verdict::Pass("x".repeat(300)), &big))
            .collect();
        let contact = Contact {
            name: Some("Ann".into()),
            email: None,
        };
        let body = build_payload(&input(&results, contact));
        assert!(body.len() <= MAX_BODY, "{}", body.len());
        let doc: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(doc["contact"], json!({"name": "Ann"}));
        assert_eq!(doc["checks"].as_array().expect("array").len(), 400);
    }

    // Tests [@ANCHOR: radioctld_upload_config_dir]
    #[test]
    fn the_config_directory_follows_the_platform_rules() {
        let none = |_: &str| None;
        assert_eq!(config_dir(&none), None);
        let home = |k: &str| (k == "HOME").then(|| "/h".to_string());
        if cfg!(all(unix, not(target_os = "macos"))) {
            assert_eq!(
                config_dir(&home),
                Some(PathBuf::from("/h/.config/radioctl"))
            );
            let xdg = |k: &str| (k == "XDG_CONFIG_HOME").then(|| "/x".to_string());
            assert_eq!(config_dir(&xdg), Some(PathBuf::from("/x/radioctl")));
        }
    }

    // Tests [@ANCHOR: radioctld_upload_save]
    // Tests [@ANCHOR: radioctld_upload_load_saved]
    #[test]
    fn saved_answers_round_trip_and_are_private() {
        let dir = std::env::temp_dir().join(format!("upload-save-{}", std::process::id()));
        let path = dir.join("radioctl/contact.json");
        assert_eq!(load_saved(&path), None);
        let saved = SavedContact {
            contact: Contact {
                name: Some("Ann".into()),
                email: None,
            },
        };
        save(&path, &saved).expect("saves");
        assert_eq!(load_saved(&path), Some(saved));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    // Tests [@ANCHOR: radioctld_upload_valid_email]
    #[test]
    fn email_shape_is_checked() {
        assert!(valid_email("a@b.org"));
        for bad in ["", "a@b", "@b.org", "a b@c.org", "a@.org", "plain"] {
            assert!(!valid_email(bad), "{bad}");
        }
    }

    // Tests [@ANCHOR: radioctld_upload_prompt_contact]
    #[test]
    fn the_prompt_asks_name_first_says_why_and_enter_skips() {
        let mut out = Vec::new();
        let saved = prompt_contact(&mut Cursor::new("Ann Ham\nann@example.org\n"), &mut out);
        assert_eq!(saved.contact.name.as_deref(), Some("Ann Ham"));
        assert_eq!(saved.contact.email.as_deref(), Some("ann@example.org"));
        let shown = String::from_utf8(out).expect("utf8");
        assert!(shown.find("your name").expect("name") < shown.find("email").expect("email"));
        assert!(shown.contains("contact you about your rig"));
        let skipped = prompt_contact(&mut Cursor::new("\n\n"), &mut Vec::new());
        assert_eq!(skipped, SavedContact::default());
        let bad = prompt_contact(&mut Cursor::new("Ann\nnot-an-email\n"), &mut Vec::new());
        assert_eq!(bad.contact.email, None);
        assert_eq!(
            prompt_contact(&mut Cursor::new(""), &mut Vec::new()),
            SavedContact::default()
        );
    }

    // Tests [@ANCHOR: radioctld_upload_resolve_contact]
    #[test]
    fn asking_happens_once_interactively_and_never_when_scripted() {
        let given = |n: &str| Contact {
            name: Some(n.into()),
            email: None,
        };
        let mut asked = 0;
        let mut ask = || {
            asked += 1;
            SavedContact {
                contact: given("Typed"),
            }
        };
        let (c, save) =
            resolve_contact(Contact::default(), Contact::default(), None, true, &mut ask);
        assert_eq!((c.name.as_deref(), save.is_some()), (Some("Typed"), true));
        let (c, save) = resolve_contact(
            Contact::default(),
            Contact::default(),
            None,
            false,
            &mut ask,
        );
        assert_eq!((c, save), (Contact::default(), None));
        let (c, _) = resolve_contact(given("Flag"), given("Env"), None, true, &mut ask);
        assert_eq!(c.name.as_deref(), Some("Flag"));
        let saved = Some(SavedContact {
            contact: given("Saved"),
        });
        let (c, save) = resolve_contact(
            Contact::default(),
            given("Env"),
            saved.clone(),
            true,
            &mut ask,
        );
        assert_eq!((c.name.as_deref(), save), (Some("Env"), None));
        let (c, _) = resolve_contact(
            Contact::default(),
            Contact::default(),
            saved,
            true,
            &mut ask,
        );
        assert_eq!(c.name.as_deref(), Some("Saved"));
        assert_eq!(asked, 1);
    }

    // Tests [@ANCHOR: radioctld_upload_parse_server]
    #[test]
    fn plain_http_is_loopback_only() {
        assert_eq!(
            parse_server(DEFAULT_URL).expect("default"),
            ServerUrl {
                tls: true,
                host: "hams.com".into(),
                port: 443,
                path: "/api/rig-reports".into()
            }
        );
        let local = parse_server("http://127.0.0.1:8080/r").expect("loopback");
        assert_eq!((local.port, local.path.as_str()), (8080, "/r"));
        assert!(parse_server("http://[::1]:9/").is_ok());
        for bad in [
            "http://hams.com/x",
            "ftp://a/",
            "hams.com",
            "https://",
            "http://localhost.evil.com/",
        ] {
            assert!(parse_server(bad).is_err(), "{bad}");
        }
    }

    /// A one-shot HTTP server on loopback that answers `status` and hands back the request.
    async fn fake_server(status: &'static str) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let url = format!(
            "http://{}/api/rig-reports",
            listener.local_addr().expect("addr")
        );
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = socket.read(&mut buf).await.expect("read");
                got.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&got).to_string();
                if let Some((head, body)) = text.split_once("\r\n\r\n") {
                    let want: usize = head
                        .lines()
                        .find_map(|l| l.strip_prefix("Content-Length: "))
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    if body.len() >= want {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            socket
                .write_all(
                    format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .as_bytes(),
                )
                .await
                .expect("answer");
            String::from_utf8_lossy(&got).to_string()
        });
        (url, task)
    }

    // Tests [@ANCHOR: radioctld_upload_post_report]
    #[tokio::test]
    async fn a_report_is_posted_as_json_and_failures_are_one_line_errors() {
        let (url, task) = fake_server("204 No Content").await;
        post_report(&url, b"{\"a\":1}").await.expect("accepted");
        let request = task.await.expect("server");
        assert!(
            request.starts_with("POST /api/rig-reports HTTP/1.1"),
            "{request}"
        );
        assert!(request.contains("Content-Type: application/json"));
        assert!(request.ends_with("{\"a\":1}"));
        let (url, task) = fake_server("503 Service Unavailable").await;
        assert_eq!(
            post_report(&url, b"{}").await,
            Err("the server answered 503".into())
        );
        task.await.expect("server");
        let (url, task) = fake_server("403 Forbidden").await;
        assert!(post_report(&url, b"{}").await.is_err());
        task.await.expect("server");
        // Nothing listens here: offline.
        let closed = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let url = format!("http://{}/x", closed.local_addr().expect("addr"));
        drop(closed);
        assert!(post_report(&url, b"{}")
            .await
            .expect_err("offline")
            .contains("cannot connect"));
        assert!(post_report("http://example.com/", b"{}").await.is_err());
    }

    // Tests [@ANCHOR: radioctld_upload_exchange]
    // Tests [@ANCHOR: radioctld_upload_decode_body]
    // Tests [@ANCHOR: radioctld_upload_request]
    #[tokio::test]
    async fn bodies_are_read_plain_and_chunked_and_a_slow_server_times_out() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi";
        assert_eq!(decode_body(plain), Some((200, b"hi".to_vec())));
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n";
        assert_eq!(decode_body(chunked), Some((200, b"abcde".to_vec())));
        assert_eq!(decode_body(b"garbage"), None);
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let url = format!("http://{}/x", listener.local_addr().expect("addr"));
        let held = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("accept");
            tokio::time::sleep(Duration::from_millis(600)).await;
            drop(socket);
        });
        let late = request("GET", &url, b"", 10, Duration::from_millis(100)).await;
        assert!(late.expect_err("slow").contains("timed out"));
        held.await.expect("task");
    }

    // Tests [@ANCHOR: radioctld_upload_notice]
    #[test]
    fn the_notice_names_the_destination_the_contents_and_the_local_copy() {
        let text = notice("https://x/y", Path::new("/tmp/r.txt"), &Contact::default());
        assert!(text.contains("https://x/y") && text.contains("/tmp/r.txt"));
        assert!(text.contains("no contact details") && text.contains("No host name"));
    }
}
