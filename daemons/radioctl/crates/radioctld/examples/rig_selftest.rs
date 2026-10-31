// SPDX-License-Identifier: AGPL-3.0-or-later
//! The volunteer self-test: read-only checks of a rig description against a real radio, and a
//! plain-text report to file by hand as a GitHub issue. See docs/VOLUNTEER_TESTING.md.
//!
//! Usage:
//!   `rig_selftest --list`                                  ports found, and the descriptions that exist
//!   `rig_selftest [--unmanaged] [--out FILE] RIG_ID TARGET [BAUD]`
//!
//! `RIG_ID` names a file in `rigs/` (see `RADIOCTL_RIGS`); `TARGET` is a serial path
//! (`/dev/ttyUSB0`), `usb:VID:PID[:SERIAL]` (hexadecimal) or `virtual` (an in-memory radio, to try the
//! tool without a radio). BAUD defaults to 19200. The report goes to `FILE` (default
//! `rig_selftest-RIG_ID.txt`) and contains the raw bytes exchanged with the radio.
//!
//! It only reads. It has no key, no Morse, no setting command and no way to open the transmit gate,
//! and it makes no network connection and uploads nothing.
use std::collections::BTreeMap;
use std::error::Error;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use radioctl_core::rigfile;
use radioctl_core::usb::list_ports;
use radioctld::candidates::{load_summaries, render_listing};
use radioctld::selftest::{
    open, plan_checks, redact_target, render_report, run_checks, utc_stamp, ReportHeader,
};

/// The directory of rig descriptions: `$RADIOCTL_RIGS`, else `./rigs` when it exists, else the
/// checkout this program was built from.
fn rigs_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RADIOCTL_RIGS") {
        return PathBuf::from(dir);
    }
    let here = PathBuf::from("rigs");
    if here.is_dir() {
        return here;
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs")
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--list") {
        print!(
            "{}",
            render_listing(&list_ports(), &load_summaries(&rigs_dir())?)
        );
        return Ok(());
    }
    let mut unmanaged = false;
    let mut out_path = None;
    while let Some(first) = args.first().map(String::as_str) {
        match first {
            "--unmanaged" => {
                unmanaged = true;
                args.remove(0);
            }
            "--out" if args.len() > 1 => {
                out_path = Some(args.remove(1));
                args.remove(0);
            }
            _ => break,
        }
    }
    let [rig_id, target, rest @ ..] = args.as_slice() else {
        return Err("usage: rig_selftest --list | rig_selftest [--unmanaged] [--out FILE] RIG_ID TARGET [BAUD]".into());
    };
    let baud: u32 = rest.first().map_or(Ok(19_200), |b| b.parse())?;
    let text = std::fs::read_to_string(rigs_dir().join(format!("{rig_id}.toml")))?;
    let loaded = rigfile::parse(&text)?;
    let description = loaded.description.clone();

    let report_path = out_path.unwrap_or_else(|| format!("rig_selftest-{rig_id}.txt"));
    let wire_path = format!("{report_path}.wire.tmp");
    let wire = Arc::new(Mutex::new(File::create(&wire_path)?));
    let started = utc_stamp(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs());
    println!(
        "rig_selftest: {rig_id} on {} at {baud} baud (read only)",
        redact_target(target)
    );

    let rig = open(&description, target, baud, unmanaged, wire).await?;
    let results = run_checks(&rig, plan_checks(&description)).await;
    for r in &results {
        println!("{:?} {} ({} ms)", r.verdict, r.name, r.millis);
    }
    let header = ReportHeader {
        rig_id: description.id.clone(),
        rig_name: description.name.clone(),
        family: format!("{:?}", description.family),
        target: redact_target(target),
        baud,
        started,
        verified: loaded
            .verification_summary()
            .into_iter()
            .collect::<BTreeMap<_, _>>(),
    };
    let wire_text = std::fs::read_to_string(&wire_path)?;
    std::fs::write(&report_path, render_report(&header, &results, &wire_text))?;
    std::fs::remove_file(&wire_path)?;
    println!("report written to {report_path}; read it before sharing, and see docs/VOLUNTEER_TESTING.md");
    Ok(())
}
