// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `radioctl-selftest` program: one read-only self-test for every rig description.
//!
//! By default it discovers the radios on this computer ([`crate::discovery`]), tests each
//! identified rig in turn (one at a time), writes one local report per rig and a run summary, and
//! uploads one report per rig to hams.com ([`crate::upload`], docs/RIG_REPORT_UPLOAD.md). It
//! reads the description's capabilities and runs only the read-only checks it claims; it never
//! transmits and has no way to open the transmit gate. See docs/VOLUNTEER_TESTING.md.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use radioctl_core::baud_probe::{probe, rates_text, SerialProbePort, DEFAULT_RATES};
use radioctl_core::description::RigDescription;
use radioctl_core::rigfile::{self, LoadedDescription};
use radioctl_core::serial::{LineControl, SerialConfig};
use radioctl_core::usb::PortInfo;
use radioctl_protocols::conformance::engine_for;
use tokio::runtime::Handle;

use crate::builtin::{choose_source, load_texts};
use crate::candidates::{render_listing, summaries_from, RigSummary};
use crate::discovery::{
    discover_ports, identified, listen_flex, render_table, Finding, NetFinding, Outcome,
};
use crate::knob::{knob_check, KNOB_WAIT};
use crate::selftest::{
    open, plan_checks, redact_target, render_report, run_checks_logged, utc_stamp, CheckResult,
    ReportHeader, Verdict,
};
use crate::update::{
    download, fetch_latest, is_newer, platform, replace_exe, restart, verify_and_extract,
    UpdateConfig,
};
use crate::upload::{
    build_payload, new_report_id, notice, post_report, prompt_contact, resolve_contact, Contact,
    PayloadInput, DEFAULT_URL,
};

/// The program's usage text.
const USAGE: &str = "usage: radioctl-selftest [--list] [--version] [--no-discover] [--rigs-dir DIR]
       [--description ID] [--port PATH|usb:VID:PID[:SERIAL]|virtual] [--baud N|auto] [--unmanaged]
       [--update] [--no-update-check] [--out FILE] [--out-dir DIR] [--name NAME] [--email EMAIL] [--no-upload] [--server URL]
       [ID PORT [BAUD]]
With no --description/--port it discovers the radios on this computer and tests each one.";

/// The baud rate to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaudArg {
    /// This rate.
    Fixed(u32),
    /// Try the usual rates with the description's own frequency read (`baud = "auto"`).
    Auto,
}

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// Print the ports and descriptions, test nothing.
    pub list: bool,
    /// Print the version.
    pub version: bool,
    /// Do not discover; a description and a port are required.
    pub no_discover: bool,
    /// A directory of descriptions instead of the embedded set.
    pub rigs_dir: Option<PathBuf>,
    /// Test this description id.
    pub description: Option<String>,
    /// Test the rig on this port.
    pub port: Option<String>,
    /// The baud setting.
    pub baud: BaudArg,
    /// Do not manage DTR and RTS (pseudo-terminals have none).
    pub unmanaged: bool,
    /// The report file when one rig is tested.
    pub out: Option<PathBuf>,
    /// The directory for reports and the summary.
    pub out_dir: Option<PathBuf>,
    /// Contact name given on the command line.
    pub name: Option<String>,
    /// Contact email given on the command line.
    pub email: Option<String>,
    /// Write the local report only.
    pub no_upload: bool,
    /// Install a newer version of this program if there is one, without asking.
    pub update: bool,
    /// Do not check for a newer version.
    pub no_update_check: bool,
    /// Upload here instead of hams.com (https, or http to this machine).
    pub server: Option<String>,
}

// [@ANCHOR: radioctld_cli_parse_args]
/// Parses the command line (without the program name). `ID PORT [BAUD]` positionals are the
/// old spelling of `--description ID --port PORT --baud BAUD`.
pub fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        list: false,
        version: false,
        no_discover: false,
        rigs_dir: None,
        description: None,
        port: None,
        baud: BaudArg::Fixed(19_200),
        unmanaged: false,
        out: None,
        out_dir: None,
        name: None,
        email: None,
        no_upload: false,
        update: false,
        no_update_check: false,
        server: None,
    };
    let mut positional = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--list" => o.list = true,
            "--version" => o.version = true,
            "--all" => {}
            "--no-discover" => o.no_discover = true,
            "--unmanaged" => o.unmanaged = true,
            "--no-upload" => o.no_upload = true,
            "--update" => o.update = true,
            "--no-update-check" => o.no_update_check = true,
            "--rigs-dir" => o.rigs_dir = Some(value("--rigs-dir")?.into()),
            "--description" | "--rig" => o.description = Some(value("--description")?),
            "--port" => o.port = Some(value("--port")?),
            "--baud" => o.baud = parse_baud(&value("--baud")?)?,
            "--out" => o.out = Some(value("--out")?.into()),
            "--out-dir" => o.out_dir = Some(value("--out-dir")?.into()),
            "--name" => o.name = Some(value("--name")?),
            "--email" => o.email = Some(value("--email")?),
            "--server" => o.server = Some(value("--server")?),
            flag if flag.starts_with("--") => {
                return Err(format!("unknown option {flag}\n{USAGE}"))
            }
            _ => positional.push(arg.clone()),
        }
    }
    match positional.as_slice() {
        [] => {}
        [id, port, rest @ ..] if rest.len() <= 1 => {
            o.description = Some(id.clone());
            o.port = Some(port.clone());
            if let Some(b) = rest.first() {
                o.baud = parse_baud(b)?;
            }
        }
        _ => return Err(format!("expected ID PORT [BAUD]\n{USAGE}")),
    }
    Ok(o)
}

// [@ANCHOR: radioctld_cli_parse_baud]
/// `auto`, or a rate between 300 and 4,000,000.
pub fn parse_baud(text: &str) -> Result<BaudArg, String> {
    if text == "auto" {
        return Ok(BaudArg::Auto);
    }
    match text.parse::<u32>() {
        Ok(rate) if (300..=4_000_000).contains(&rate) => Ok(BaudArg::Fixed(rate)),
        _ => Err(format!("{text}: the baud rate is a number or auto")),
    }
}

/// What the program needs from the machine it runs on (injected so tests need no hardware).
pub struct Host {
    /// The serial ports present.
    pub ports: Vec<PortInfo>,
    /// True when a person is at a terminal and can be asked.
    pub interactive: bool,
    /// The file the contact answers are kept in, if there is a configuration directory.
    pub contact_file: Option<PathBuf>,
    /// Contact details from environment variables.
    pub env_contact: Contact,
    /// How long to listen for network radios (zero: do not).
    pub lan_window: Duration,
    /// The release channel to check for a newer version; `None` when this build has none.
    pub update: Option<UpdateConfig>,
    /// Start the new program after an update (tests turn this off).
    pub restart_after_update: bool,
    /// Polled during the knob check: true once the volunteer pressed Enter. `None`: never.
    pub enter_pressed: Option<EnterPoll>,
}

/// One rig to test.
#[derive(Debug, Clone)]
struct Target {
    target: String,
    port: Option<PortInfo>,
}

/// What a tested rig leaves for the upload.
struct Tested {
    results: Vec<CheckResult>,
    transport: &'static str,
    baud: u32,
    usb_vid_pid: Option<String>,
    probe_tried: Option<Vec<u32>>,
}

// [@ANCHOR: radioctld_cli_find_description]
/// The loaded description with this id, or an error that lists the ids there are.
pub fn find_description<'a>(
    loaded: &'a [LoadedDescription],
    id: &str,
) -> Result<&'a LoadedDescription, String> {
    loaded
        .iter()
        .find(|l| l.description.id == id)
        .ok_or_else(|| {
            let ids: Vec<&str> = loaded.iter().map(|l| l.description.id.as_str()).collect();
            format!("no description {id}; there are: {}", ids.join(", "))
        })
}

// [@ANCHOR: radioctld_cli_port_identity]
/// The USB identity of the port named by `target` (a path or `usb:VID:PID`), with no serial number.
pub fn port_identity(target: &str, ports: &[PortInfo]) -> Option<PortInfo> {
    let usb = target.strip_prefix("usb:").map(|ids| {
        let mut parts = ids.split(':');
        (
            parts.next().and_then(|v| u16::from_str_radix(v, 16).ok()),
            parts.next().and_then(|p| u16::from_str_radix(p, 16).ok()),
        )
    });
    ports
        .iter()
        .find(|p| match usb {
            Some((vid, pid)) => p.vid == vid && p.pid == pid,
            None => p.path == Path::new(target),
        })
        .map(|p| PortInfo {
            serial: None,
            ..p.clone()
        })
}

// [@ANCHOR: radioctld_cli_describe_identity]
/// `usb 10c4:ea60, product "..."` for the report; empty when the port has no USB ids.
pub fn describe_identity(port: &PortInfo) -> String {
    match (port.vid, port.pid) {
        (Some(v), Some(p)) => format!(
            "usb {v:04x}:{p:04x}, product {:?} (serial number not recorded)",
            port.product.as_deref().unwrap_or("(none)")
        ),
        _ => String::new(),
    }
}

// [@ANCHOR: radioctld_cli_report_path]
/// Where a rig's local report goes: `--out` for a single rig, else
/// `radioctl-selftest-ID.txt` (`-N` added when several tested rigs share an id) in `out_dir`.
pub fn report_path(o: &Options, id: &str, index: usize, total: usize) -> PathBuf {
    if total == 1 {
        if let Some(out) = &o.out {
            return out.clone();
        }
    }
    let name = if index == 0 {
        format!("radioctl-selftest-{id}.txt")
    } else {
        format!("radioctl-selftest-{id}-{}.txt", index + 1)
    };
    o.out_dir.clone().unwrap_or_default().join(name)
}

// [@ANCHOR: radioctld_cli_firmware_text]
/// The version read from the rig by the `identify: firmware` check, as text (`Firmware(1, 2)`
/// becomes `1.2`), or `None` when the rig did not give one.
pub fn firmware_text(results: &[CheckResult]) -> Option<String> {
    results
        .iter()
        .find(|r| r.name == "identify: firmware")
        .and_then(|r| match &r.verdict {
            Verdict::Pass(text) => {
                let inner = text.strip_prefix("Firmware(")?.strip_suffix(')')?;
                let (a, b) = inner.split_once(", ")?;
                Some(format!(
                    "{}.{}",
                    a.trim().parse::<u32>().ok()?,
                    b.trim().parse::<u32>().ok()?
                ))
            }
            _ => None,
        })
}

// [@ANCHOR: radioctld_cli_thanks]
/// The thank-you printed after a successful upload. `rig` is manufacturer and model; the version
/// is left out, with no stray space, when unknown. Not-applicable checks are not counted; if none
/// failed the rig's operation is called verified, otherwise the failures are counted.
pub fn thanks(rig: &str, results: &[CheckResult]) -> String {
    let who = match firmware_text(results) {
        Some(version) => format!("{rig} {version}"),
        None => rig.to_string(),
    };
    let counted: Vec<&CheckResult> = results
        .iter()
        .filter(|r| !matches!(r.verdict, Verdict::NotApplicable(_)))
        .collect();
    let failed = counted
        .iter()
        .filter(|r| matches!(r.verdict, Verdict::Fail(_)))
        .count();
    let tail = "Please feel free to re-run this program any time and with new radios. We may contact you again if we see bugs and want to resolve them.";
    if failed == 0 {
        format!("Thank You! You have verified the operation of {who}. {tail}\n")
    } else {
        format!(
            "Thank You! Your report on {who} is in. {failed} of {} checks did not pass. That is a problem in our software, not something you did wrong, and we will look into it. {tail}\n",
            counted.len()
        )
    }
}

// [@ANCHOR: radioctld_cli_render_summary]
/// The run summary: the discovery table and one line per tested rig.
pub fn render_summary(table: &str, tested: &[(String, PathBuf, usize, usize, String)]) -> String {
    let mut text = format!("radioctl-selftest run summary\n\n{table}\n");
    for (id, path, pass, fail, uploaded) in tested {
        text.push_str(&format!(
            "{id}: {pass} pass, {fail} did not pass; report {}; upload: {uploaded}\n",
            path.display()
        ));
    }
    if tested.is_empty() {
        text.push_str("no rig was tested\n");
    }
    text
}

// [@ANCHOR: radioctld_cli_choose_targets]
/// The rigs to test: the one named by `--description` and `--port`, or what discovery
/// identified. Naming one without the other uses the sole port or the port's own product string.
pub fn choose_targets(
    o: &Options,
    ports: &[PortInfo],
    findings: &[Finding],
) -> Result<Vec<(String, String, Option<PortInfo>)>, String> {
    match (&o.description, &o.port) {
        (Some(id), Some(port)) => Ok(vec![(id.clone(), port.clone(), port_identity(port, ports))]),
        (Some(id), None) => match ports {
            [one] => Ok(vec![(
                id.clone(),
                one.path.display().to_string(),
                Some(one.clone()),
            )]),
            _ => Err(format!(
                "--description {id} needs --port: {} ports found",
                ports.len()
            )),
        },
        (None, Some(port)) => {
            let found = findings
                .iter()
                .find(|f| f.port.path == Path::new(port))
                .map(|f| (&f.outcome, &f.port));
            match found {
                Some((Outcome::Identified { id, .. }, p)) => {
                    Ok(vec![(id.clone(), port.clone(), Some(p.clone()))])
                }
                _ => Err(format!(
                    "{port}: the device does not name one description; use --description"
                )),
            }
        }
        (None, None) => Ok(identified(findings)
            .into_iter()
            .map(|(id, path, port)| (id, path.display().to_string(), Some(port)))
            .collect()),
    }
}

// [@ANCHOR: radioctld_cli_probe_baud]
/// Probes the baud for `description` on `path`, the way `baud = "auto"` does.
async fn probe_baud(
    description: &RigDescription,
    target: &str,
    unmanaged: bool,
) -> Result<Vec<u32>, String> {
    let mut engine = engine_for(description).map_err(|e| e.detail)?;
    let port_target = crate::selftest::parse_target(target)?;
    let mut template =
        SerialConfig::new(port_target, DEFAULT_RATES[1]).with_description_line(description);
    if unmanaged {
        template.lines = LineControl::Unmanaged;
    }
    let mut port = SerialProbePort::new(Handle::current(), template);
    let found = probe(&mut port, &mut engine, &DEFAULT_RATES)
        .await
        .map_err(|e| format!("baud probe: {e}"))?;
    Ok(found.tried)
}

// [@ANCHOR: radioctld_cli_test_one]
/// Tests one rig and writes its local report. `Err` only for a rig that could not be opened.
async fn test_one(
    o: &Options,
    t: &Target,
    loaded: &LoadedDescription,
    report: &Path,
    interactive: bool,
    enter: &mut dyn FnMut() -> bool,
    out: &mut dyn Write,
) -> Result<Tested, String> {
    let description = &loaded.description;
    let wire_path = PathBuf::from(format!("{}.wire.tmp", report.display()));
    let wire = Arc::new(Mutex::new(
        std::fs::File::create(&wire_path)
            .map_err(|e| format!("cannot write {}: {e}", wire_path.display()))?,
    ));
    let started = utc_stamp(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    );
    let mut tried = None;
    let baud = match o.baud {
        BaudArg::Fixed(rate) => rate,
        BaudArg::Auto if t.target == "virtual" => 19_200,
        BaudArg::Auto => {
            let rates = probe_baud(description, &t.target, o.unmanaged).await?;
            let rate = rates.last().copied().unwrap_or(19_200);
            tried = Some(rates);
            rate
        }
    };
    let rig = open(description, &t.target, baud, o.unmanaged, wire).await?;
    let mut results = run_checks_logged(&rig, plan_checks(description), Some(&wire_path)).await;
    // A virtual rig has no knob to turn.
    if t.target != "virtual" {
        results.extend(knob_check(description, &rig, interactive, enter, out, KNOB_WAIT).await);
    }
    let identity = t.port.as_ref().map(describe_identity).unwrap_or_default();
    let header = ReportHeader {
        rig_id: description.id.clone(),
        rig_name: description.name.clone(),
        family: format!("{:?}", description.family),
        target: redact_target(&t.target),
        baud,
        started,
        verified: loaded
            .verification_summary()
            .into_iter()
            .collect::<BTreeMap<_, _>>(),
        port_identity: identity,
        baud_probe: tried.as_ref().map(|r| rates_text(r)).unwrap_or_default(),
    };
    let wire_text = std::fs::read_to_string(&wire_path).unwrap_or_default();
    std::fs::write(report, render_report(&header, &results, &wire_text))
        .map_err(|e| format!("cannot write {}: {e}", report.display()))?;
    let _ = std::fs::remove_file(&wire_path);
    let usb = t
        .port
        .as_ref()
        .and_then(|p| Some(format!("{:04x}:{:04x}", p.vid?, p.pid?)));
    Ok(Tested {
        results,
        transport: if t.target == "virtual" {
            "virtual"
        } else if usb.is_some() {
            "usb"
        } else {
            "serial"
        },
        baud,
        usb_vid_pid: usb,
        probe_tried: tried,
    })
}

// [@ANCHOR: radioctld_cli_update_flow]
/// The start-of-run version check. Offline or any error is one line and the test carries on
/// (`None`). A newer version is only offered: asked of an interactive user, or taken with `--update`;
/// otherwise a notice says how. A failed or unverifiable download leaves the old program untouched.
/// `Some(status)` means the program was replaced and the run is over (the new one took over).
pub async fn update_flow(
    o: &Options,
    host: &Host,
    cfg: &UpdateConfig,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> Option<i32> {
    let platform = platform()?;
    let latest = match fetch_latest(&cfg.base_url, platform).await {
        Ok(latest) => latest,
        Err(e) => {
            let _ = writeln!(
                out,
                "Could not check for a newer radioctl-selftest ({e}); continuing."
            );
            return None;
        }
    };
    match is_newer(&latest.version, &cfg.current_version) {
        Ok(true) => {}
        Ok(false) => return None,
        Err(e) => {
            let _ = writeln!(
                out,
                "Could not check for a newer radioctl-selftest ({e}); continuing."
            );
            return None;
        }
    }
    let yes = o.update
        || host.interactive && {
            let _ = write!(
                out,
                "A newer radioctl-selftest ({}) is available; update now? [y/N] ",
                latest.version
            );
            let _ = out.flush();
            let mut answer = String::new();
            let _ = input.read_line(&mut answer);
            answer.trim().to_ascii_lowercase().starts_with('y')
        };
    if !yes {
        let _ = writeln!(
            out,
            "A newer radioctl-selftest ({}) is available; run it with --update to install it. Continuing with this version.",
            latest.version
        );
        return None;
    }
    let installed = async {
        let bundle = download(&cfg.base_url, platform).await?;
        let program = verify_and_extract(&bundle, &latest.sha256, &cfg.keys)?;
        replace_exe(&cfg.exe, &program)
    }
    .await;
    match installed {
        Ok(prev) => {
            let _ = writeln!(
                out,
                "Updated to {}; the old program is kept as {}. Restarting.",
                latest.version,
                prev.display()
            );
            Some(if host.restart_after_update {
                restart(&cfg.exe)
            } else {
                0
            })
        }
        Err(e) => {
            let _ = writeln!(
                out,
                "The update failed ({e}); the existing program is unchanged. Continuing."
            );
            None
        }
    }
}

// [@ANCHOR: radioctld_cli_run]
/// Runs the program for `o`; returns the exit status: 0 when every check of every rig passed or
/// was not applicable, 1 when a check failed or a rig could not be opened, 2 for a usage error.
/// An upload that fails never changes it.
pub async fn run(o: Options, mut host: Host, input: &mut dyn BufRead, out: &mut dyn Write) -> i32 {
    if o.version {
        let _ = writeln!(
            out,
            "radioctl-selftest {} ({} {})",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        return 0;
    }
    let source = choose_source(
        o.rigs_dir.as_deref(),
        std::env::var_os("RADIOCTL_RIGS").as_deref(),
    );
    let setup = (|| {
        let texts = load_texts(&source)?;
        let summaries = summaries_from(&texts)?;
        let loaded: Result<Vec<_>, String> = texts
            .iter()
            .map(|(label, text)| rigfile::parse(text).map_err(|e| format!("{label}: {e}")))
            .collect();
        Ok::<_, String>((summaries, loaded?))
    })();
    let (summaries, loaded): (Vec<RigSummary>, Vec<LoadedDescription>) = match setup {
        Ok(pair) => pair,
        Err(e) => {
            let _ = writeln!(out, "radioctl-selftest: {e}");
            return 2;
        }
    };
    if o.list {
        let _ = write!(out, "{}", render_listing(&host.ports, &summaries));
        return 0;
    }
    if let (Some(cfg), false) = (&host.update, o.no_update_check) {
        if let Some(status) = update_flow(&o, &host, cfg, input, out).await {
            return status;
        }
    }
    let mut enter_pressed = host.enter_pressed.take();
    let mut no_enter = || false;
    let enter: &mut dyn FnMut() -> bool = match enter_pressed.as_mut() {
        Some(f) => f,
        None => &mut no_enter,
    };
    let named_both = o.description.is_some() && o.port.is_some();
    if o.no_discover && !named_both {
        let _ = writeln!(out, "--no-discover needs --description and --port\n{USAGE}");
        return 2;
    }
    let descriptions: Vec<RigDescription> = loaded.iter().map(|l| l.description.clone()).collect();
    let (findings, net): (Vec<Finding>, Vec<NetFinding>) = if o.no_discover || named_both {
        (Vec::new(), Vec::new())
    } else {
        let findings = discover_ports(&host.ports, &descriptions, &summaries, o.unmanaged).await;
        let net = if host.lan_window.is_zero() {
            Vec::new()
        } else {
            listen_flex(host.lan_window, &summaries).await
        };
        (findings, net)
    };
    let table = if findings.is_empty() && net.is_empty() {
        String::new()
    } else {
        render_table(&findings, &net)
    };
    let _ = write!(out, "{table}");
    let chosen = match choose_targets(&o, &host.ports, &findings) {
        Ok(chosen) => chosen,
        Err(e) => {
            let _ = writeln!(out, "radioctl-selftest: {e}");
            return 2;
        }
    };
    if chosen.is_empty() {
        let _ = writeln!(
            out,
            "No radio was identified. Name one with --description ID --port PORT; see --list."
        );
    }
    let total = chosen.len();
    let url = o.server.clone().unwrap_or_else(|| DEFAULT_URL.to_string());
    let mut contact: Option<Contact> = None;
    let mut status = 0;
    let mut thanked = 0;
    let mut tested: Vec<(String, PathBuf, usize, usize, String)> = Vec::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for (id, target, port) in chosen {
        let index = seen.entry(id.clone()).or_insert(0);
        let report = report_path(&o, &id, *index, total);
        *index += 1;
        let t = Target { target, port };
        let one = match find_description(&loaded, &id) {
            Err(e) => Err(e),
            Ok(l) => {
                let _ = writeln!(
                    out,
                    "radioctl-selftest: {id} on {} (read only)",
                    redact_target(&t.target)
                );
                test_one(&o, &t, l, &report, host.interactive, &mut *enter, out).await
            }
        };
        let tested_rig = match one {
            Ok(done) => done,
            Err(e) => {
                let _ = writeln!(out, "radioctl-selftest: {id}: {e}");
                status = 1;
                tested.push((id, report, 0, 0, format!("not run: {e}")));
                continue;
            }
        };
        let results = &tested_rig.results;
        for r in results {
            let _ = writeln!(out, "{:?} {} ({} ms)", r.verdict, r.name, r.millis);
        }
        let fails = results
            .iter()
            .filter(|r| matches!(r.verdict, Verdict::Fail(_)))
            .count();
        let passes = results
            .iter()
            .filter(|r| matches!(r.verdict, Verdict::Pass(_)))
            .count();
        if fails > 0 {
            status = 1;
            let _ = writeln!(out, "{}", crate::selftest::FAILURE_NOTE);
        }
        let abs = std::fs::canonicalize(&report).unwrap_or_else(|_| report.clone());
        let uploaded = if o.no_upload {
            "skipped (--no-upload)".to_string()
        } else {
            let c = contact.get_or_insert_with(|| {
                let (c, save_me) = resolve_contact(
                    Contact {
                        name: o.name.clone(),
                        email: o.email.clone(),
                    },
                    host.env_contact.clone(),
                    host.contact_file
                        .as_deref()
                        .and_then(crate::upload::load_saved),
                    host.interactive,
                    &mut || prompt_contact(input, out),
                );
                if let (Some(saved), Some(file)) = (save_me, &host.contact_file) {
                    if let Err(e) = crate::upload::save(file, &saved) {
                        let _ = writeln!(out, "could not save your answers: {e}");
                    }
                }
                c
            });
            let _ = write!(out, "{}", notice(&url, &abs, c));
            let payload = PayloadInput {
                report_id: new_report_id().unwrap_or_default(),
                sent_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs()),
                description_id: id.clone(),
                rig_name: find_description(&loaded, &id)
                    .map(|l| l.description.name.clone())
                    .unwrap_or_default(),
                transport: tested_rig.transport,
                baud: tested_rig.baud,
                usb_vid_pid: tested_rig.usb_vid_pid.clone(),
                probe_tried: tested_rig.probe_tried.clone(),
                results,
                contact: c.clone(),
            };
            match post_report(&url, &build_payload(&payload)).await {
                Ok(()) => {
                    let _ = write!(out, "{}", thanks(&payload.rig_name, results));
                    thanked += 1;
                    "uploaded".to_string()
                }
                Err(e) => {
                    let _ = writeln!(out, "upload failed ({e}); your local report is kept");
                    format!("failed ({e})")
                }
            }
        };
        let _ = writeln!(out, "report written to {}", abs.display());
        tested.push((id, report, passes, fails, uploaded));
    }
    if total > 1 {
        let _ = writeln!(
            out,
            "Tested {total} radios; {thanked} reports uploaded. Thank you."
        );
    }
    if !table.is_empty() || total > 1 {
        let path = o
            .out_dir
            .clone()
            .unwrap_or_default()
            .join("radioctl-selftest-summary.txt");
        match std::fs::write(&path, render_summary(&table, &tested)) {
            Ok(()) => {
                let _ = writeln!(out, "run summary written to {}", path.display());
            }
            Err(e) => {
                let _ = writeln!(out, "cannot write the summary: {e}");
            }
        }
    }
    status
}

/// A poll that is true once the volunteer pressed Enter.
pub type EnterPoll = Box<dyn FnMut() -> bool + Send>;

/// Standard input read by one thread, one line at a time, so the program can both wait for a line
/// (a prompt) and ask whether one has arrived (the knob check's Enter to skip).
pub struct StdinLines {
    rx: std::sync::Arc<std::sync::Mutex<std::sync::mpsc::Receiver<String>>>,
    buf: Vec<u8>,
    pos: usize,
}

impl StdinLines {
    // [@ANCHOR: radioctld_cli_stdin_lines_from]
    /// Lines from `rx` as a [`BufRead`], and a poll that is true once a line was waiting (and
    /// takes it).
    pub fn from_receiver(
        rx: std::sync::mpsc::Receiver<String>,
    ) -> (StdinLines, impl FnMut() -> bool + Send) {
        let shared = std::sync::Arc::new(std::sync::Mutex::new(rx));
        let poll_rx = std::sync::Arc::clone(&shared);
        let poll = move || poll_rx.lock().is_ok_and(|rx| rx.try_recv().is_ok());
        (
            StdinLines {
                rx: shared,
                buf: Vec::new(),
                pos: 0,
            },
            poll,
        )
    }

    // [@ANCHOR: radioctld_cli_stdin_lines_spawn]
    /// Starts the thread that reads the process's standard input line by line.
    pub fn spawn() -> (StdinLines, impl FnMut() -> bool + Send) {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            loop {
                let mut line = String::new();
                if !matches!(stdin.read_line(&mut line), Ok(n) if n > 0) || tx.send(line).is_err() {
                    break;
                }
            }
        });
        StdinLines::from_receiver(rx)
    }
}

impl std::io::Read for StdinLines {
    // [@ANCHOR: radioctld_cli_stdin_lines_read]
    /// Reads from the current line.
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let data = BufRead::fill_buf(self)?;
        let n = data.len().min(out.len());
        out[..n].copy_from_slice(&data[..n]);
        BufRead::consume(self, n);
        Ok(n)
    }
}

impl BufRead for StdinLines {
    // [@ANCHOR: radioctld_cli_stdin_lines_fill_buf]
    /// Waits for the next line when the current one is used up; end of input gives no bytes.
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        if self.pos >= self.buf.len() {
            self.buf = self
                .rx
                .lock()
                .ok()
                .and_then(|rx| rx.recv().ok())
                .unwrap_or_default()
                .into_bytes();
            self.pos = 0;
        }
        Ok(self.buf.get(self.pos..).unwrap_or(&[]))
    }

    fn consume(&mut self, amount: usize) {
        self.pos += amount;
    }
}

// [@ANCHOR: radioctld_cli_main_from_env]
/// The whole program for a binary's `main`: arguments, terminal, environment and the real serial
/// ports in, an exit status out. The relay's `radioctl-selftest` and radioctl's own are both this.
pub fn main_from_env() -> i32 {
    main_with_bundle(None)
}

// [@ANCHOR: radioctld_cli_main_with_bundle]
/// [`main_from_env`] for a program shipped inside a signed release bundle: `bundle` is the bundle's version
/// and the release keys, and enables the start-of-run update check against [`crate::update::DEFAULT_BASE`].
/// A run that was just restarted by an update (`RADIOCTL_SELFTEST_UPDATED`) does not check again.
pub fn main_with_bundle(bundle: Option<(&str, Vec<[u8; 32]>)>) -> i32 {
    use std::io::IsTerminal;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match parse_args(&args) {
        Ok(options) => options,
        Err(e) => {
            eprintln!("radioctl-selftest: {e}");
            return 2;
        }
    };
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    // At a terminal one thread owns stdin, so a prompt and the knob check's "press Enter to skip"
    // read the same lines; without one nothing is ever read from stdin but the plain lock.
    let (stdin_lines, stdin_poll): (Option<StdinLines>, Option<EnterPoll>) = if interactive {
        let (lines, poll) = StdinLines::spawn();
        (Some(lines), Some(Box::new(poll)))
    } else {
        (None, None)
    };
    let host = Host {
        ports: radioctl_core::usb::list_ports(),
        interactive,
        contact_file: crate::upload::config_dir(&var).map(|d| d.join("selftest-contact.json")),
        env_contact: Contact {
            name: var("RADIOCTL_SELFTEST_NAME"),
            email: var("RADIOCTL_SELFTEST_EMAIL"),
        },
        lan_window: Duration::from_secs(3),
        update: bundle.and_then(|(version, keys)| {
            if var("RADIOCTL_SELFTEST_UPDATED").is_some() {
                return None;
            }
            Some(UpdateConfig {
                base_url: crate::update::DEFAULT_BASE.to_string(),
                current_version: version.to_string(),
                keys,
                exe: std::env::current_exe().ok()?,
            })
        }),
        restart_after_update: true,
        enter_pressed: stdin_poll,
    };
    let mut stdin_lines = stdin_lines;
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("radioctl-selftest: cannot start: {e}");
            return 2;
        }
    };
    let mut plain = std::io::stdin().lock();
    let input: &mut dyn BufRead = match stdin_lines.as_mut() {
        Some(lines) => lines,
        None => &mut plain,
    };
    runtime.block_on(run(options, host, input, &mut std::io::stdout()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn port(path: &str, product: Option<&str>) -> PortInfo {
        PortInfo {
            path: PathBuf::from(path),
            vid: Some(0x10c4),
            pid: Some(0xea60),
            serial: Some("SERIAL1".into()),
            product: product.map(str::to_string),
            cdc_acm: false,
        }
    }

    // Tests [@ANCHOR: radioctld_cli_parse_args]
    // Tests [@ANCHOR: radioctld_cli_parse_baud]
    #[test]
    fn flags_and_the_old_positional_spelling_parse() {
        let o = parse_args(&args(&[
            "--no-upload",
            "--baud",
            "auto",
            "--rigs-dir",
            "/r",
            "--name",
            "A",
        ]))
        .expect("flags");
        assert!(o.no_upload && o.baud == BaudArg::Auto && o.rigs_dir == Some("/r".into()));
        assert!(o.description.is_none() && !o.no_discover);
        let o = parse_args(&args(&["icom-ic-7300", "/dev/ttyUSB0", "115200"])).expect("positional");
        assert_eq!(o.description.as_deref(), Some("icom-ic-7300"));
        assert_eq!(
            (o.port.as_deref(), o.baud),
            (Some("/dev/ttyUSB0"), BaudArg::Fixed(115_200))
        );
        for bad in [
            vec!["--baud"],
            vec!["--nope"],
            vec!["a", "b", "1", "2"],
            vec!["--baud", "fast"],
            vec!["--baud", "5"],
        ] {
            assert!(parse_args(&args(&bad)).is_err(), "{bad:?}");
        }
    }

    // Tests [@ANCHOR: radioctld_cli_probe_baud]
    // Tests [@ANCHOR: radioctld_cli_test_one]
    #[tokio::test]
    async fn an_unopenable_port_is_an_error_not_a_hang() {
        let loaded = rigfile::parse(&crate::builtin::embedded_texts()[0].1).expect("parse");
        assert!(
            probe_baud(&loaded.description, "/dev/no-such-selftest-port", true)
                .await
                .is_err()
        );
        let o = parse_args(&[]).expect("defaults");
        let t = Target {
            target: "/dev/no-such-selftest-port".into(),
            port: None,
        };
        let report = std::env::temp_dir().join(format!("test-one-{}.txt", std::process::id()));
        assert!(test_one(
            &o,
            &t,
            &loaded,
            &report,
            false,
            &mut || false,
            &mut Vec::new()
        )
        .await
        .is_err());
        std::fs::remove_file(format!("{}.wire.tmp", report.display())).ok();
    }

    // Tests [@ANCHOR: radioctld_cli_stdin_lines_read]
    // Tests [@ANCHOR: radioctld_cli_stdin_lines_fill_buf]
    // Tests [@ANCHOR: radioctld_cli_stdin_lines_from]
    // Tests [@ANCHOR: radioctld_cli_stdin_lines_spawn]
    #[test]
    fn a_line_can_be_read_by_a_prompt_or_taken_by_the_poll_but_not_both() {
        let (tx, rx) = std::sync::mpsc::channel();
        let (mut lines, mut poll) = StdinLines::from_receiver(rx);
        assert!(!poll());
        tx.send("Ann\n".to_string()).expect("send");
        assert!(poll());
        assert!(!poll());
        tx.send("Bob\n".to_string()).expect("send");
        let mut line = String::new();
        lines.read_line(&mut line).expect("read");
        assert_eq!(line, "Bob\n");
        drop(tx);
        line.clear();
        assert_eq!(lines.read_line(&mut line).expect("eof"), 0);
        let (tx, rx) = std::sync::mpsc::channel();
        let (mut lines, _poll) = StdinLines::from_receiver(rx);
        tx.send("xy\n".to_string()).expect("send");
        let mut two = [0u8; 2];
        std::io::Read::read_exact(&mut lines, &mut two).expect("read");
        assert_eq!(&two, b"xy");
        // The real reader starts and is simply left running (it ends at the end of input).
        let (_lines, mut real_poll) = StdinLines::spawn();
        let _ = real_poll();
    }

    // Tests [@ANCHOR: radioctld_cli_find_description]
    #[test]
    fn a_missing_description_lists_the_ones_there_are() {
        let texts = crate::builtin::embedded_texts();
        let loaded: Vec<_> = texts
            .iter()
            .map(|(_, t)| rigfile::parse(t).expect("parse"))
            .collect();
        assert!(find_description(&loaded, "icom-ic-7300").is_ok());
        let e = find_description(&loaded, "nope").expect_err("error");
        assert!(e.contains("icom-ic-7300"));
    }

    // Tests [@ANCHOR: radioctld_cli_port_identity]
    // Tests [@ANCHOR: radioctld_cli_describe_identity]
    #[test]
    fn the_identity_never_carries_the_serial_number() {
        let ports = [port("/dev/ttyUSB0", Some("IC-7300"))];
        for target in ["/dev/ttyUSB0", "usb:10c4:ea60:SERIAL1"] {
            let found = port_identity(target, &ports).expect("found");
            assert_eq!(found.serial, None);
            let text = describe_identity(&found);
            assert!(
                text.contains("10c4:ea60") && !text.contains("SERIAL1"),
                "{text}"
            );
        }
        assert!(port_identity("/dev/other", &ports).is_none());
        assert_eq!(
            describe_identity(&PortInfo {
                vid: None,
                ..ports[0].clone()
            }),
            ""
        );
    }

    // Tests [@ANCHOR: radioctld_cli_report_path]
    #[test]
    fn report_files_do_not_collide() {
        let mut o = parse_args(&[]).expect("defaults");
        assert_eq!(
            report_path(&o, "x", 0, 2),
            PathBuf::from("radioctl-selftest-x.txt")
        );
        assert_eq!(
            report_path(&o, "x", 1, 2),
            PathBuf::from("radioctl-selftest-x-2.txt")
        );
        o.out = Some("mine.txt".into());
        assert_eq!(report_path(&o, "x", 0, 1), PathBuf::from("mine.txt"));
        assert_eq!(
            report_path(&o, "x", 0, 2),
            PathBuf::from("radioctl-selftest-x.txt")
        );
    }

    fn res(name: &str, v: Verdict) -> CheckResult {
        CheckResult {
            name: name.into(),
            op: String::new(),
            verdict: v,
            millis: 0,
            wire: String::new(),
        }
    }

    // Tests [@ANCHOR: radioctld_cli_firmware_text]
    // Tests [@ANCHOR: radioctld_cli_thanks]
    #[test]
    fn the_thanks_has_three_wordings_and_no_dangling_space_without_a_version() {
        let tail = " Please feel free to re-run this program any time and with new radios. We may contact you again if we see bugs and want to resolve them.\n";
        let fw = res("identify: firmware", Verdict::Pass("Firmware(1, 2)".into()));
        let ok = res("read mode", Verdict::Pass("x".into()));
        let na = res("read RIT", Verdict::NotApplicable("n".into()));
        let bad = res("read level", Verdict::Fail("t".into()));
        assert_eq!(
            firmware_text(std::slice::from_ref(&fw)).as_deref(),
            Some("1.2")
        );
        assert_eq!(
            thanks("Icom IC-7300", &[fw.clone(), ok.clone(), na.clone()]),
            format!("Thank You! You have verified the operation of Icom IC-7300 1.2.{tail}")
        );
        assert_eq!(
            thanks("Icom IC-7300", &[ok.clone(), na]),
            format!("Thank You! You have verified the operation of Icom IC-7300.{tail}")
        );
        assert_eq!(
            thanks("Icom IC-7300", &[fw, ok.clone(), bad.clone()]),
            format!("Thank You! Your report on Icom IC-7300 1.2 is in. 1 of 3 checks did not pass. That is a problem in our software, not something you did wrong, and we will look into it.{tail}")
        );
        assert!(thanks("Icom IC-7300", &[ok, bad.clone()])
            .contains("report on Icom IC-7300 is in. 1 of 2"));
        let unreadable = res("identify: firmware", Verdict::Fail("t".into()));
        assert_eq!(firmware_text(&[unreadable]), None);
        assert_eq!(
            firmware_text(&[res("identify: firmware", Verdict::Pass("Other".into()))]),
            None
        );
    }

    // Tests [@ANCHOR: radioctld_cli_render_summary]
    #[test]
    fn the_summary_has_the_table_and_a_line_per_rig() {
        let text = render_summary(
            "T\n",
            &[("x".into(), "r.txt".into(), 3, 1, "uploaded".into())],
        );
        assert!(
            text.contains("T\n")
                && text.contains("x: 3 pass, 1 did not pass; report r.txt; upload: uploaded")
        );
        assert!(render_summary("", &[]).contains("no rig was tested"));
    }

    // Tests [@ANCHOR: radioctld_cli_choose_targets]
    #[test]
    fn naming_one_half_uses_the_sole_port_or_the_product_string() {
        let ports = [port("/dev/a", Some("IC-7300"))];
        let mut o = parse_args(&[]).expect("defaults");
        o.description = Some("icom-ic-7300".into());
        assert_eq!(choose_targets(&o, &ports, &[]).expect("sole").len(), 1);
        assert!(choose_targets(&o, &[], &[]).is_err());
        o.description = None;
        o.port = Some("/dev/a".into());
        assert!(choose_targets(&o, &ports, &[]).is_err());
        assert!(choose_targets(&parse_args(&[]).expect("d"), &ports, &[])
            .expect("none")
            .is_empty());
    }

    // Tests [@ANCHOR: radioctld_cli_run]
    // Tests [@ANCHOR: radioctld_cli_main_from_env]
    #[tokio::test]
    async fn version_list_and_usage_errors_need_no_radio() {
        let host = || Host {
            ports: vec![],
            interactive: false,
            contact_file: None,
            env_contact: Contact::default(),
            lan_window: Duration::ZERO,
            update: None,
            restart_after_update: false,
            enter_pressed: None,
        };
        let mut out = Vec::new();
        let o = parse_args(&args(&["--version"])).expect("o");
        assert_eq!(
            run(o, host(), &mut std::io::Cursor::new(""), &mut out).await,
            0
        );
        assert!(String::from_utf8_lossy(&out).contains("radioctl-selftest "));
        let mut out = Vec::new();
        let o = parse_args(&args(&["--list"])).expect("o");
        assert_eq!(
            run(o, host(), &mut std::io::Cursor::new(""), &mut out).await,
            0
        );
        assert!(String::from_utf8_lossy(&out).contains("icom-ic-7300"));
        let o = parse_args(&args(&["--no-discover"])).expect("o");
        assert_eq!(
            run(o, host(), &mut std::io::Cursor::new(""), &mut Vec::new()).await,
            2
        );
        let o = parse_args(&args(&[
            "--description",
            "virtual-nonexistent",
            "--port",
            "virtual",
            "--no-upload",
        ]))
        .expect("o");
        assert_eq!(
            run(o, host(), &mut std::io::Cursor::new(""), &mut Vec::new()).await,
            1
        );
    }
}
