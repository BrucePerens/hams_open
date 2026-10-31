// SPDX-License-Identifier: AGPL-3.0-or-later
//! A virtual radio on a pseudo-terminal, for rehearsing a bench day without hardware: the radio of
//! a rig description answers on the master side of a pty and the slave side is linked at a path
//! that `radioctld` (`kind = "serial"`), Hamlib's `rigctl -m 3073 -r PATH` or any CAT program opens
//! as if it were the radio's serial port. Nothing here opens a real serial device.
//!
//! Usage: `virtual_radio_pty RIG_ID LINK [WIRE_LOG]` (`RIG_ID` names a file in `rigs/`, for example
//! `icom-ic-7300`; `LINK` is the symlink to create; `WIRE_LOG` is an optional file that receives
//! every byte in both directions as hexadecimal lines).
//!
//! Commands on standard input stand in for the radio's front panel and its cable:
//!
//! - `freq HZ`, `mode NAME`, `ptt 0|1`, `signal N`, `split 0|1`, `rit HZ`: knobs and buttons.
//! - `state`: print the radio's state.
//! - `unplug` and `plug`: the USB cable pulled and pushed back. `unplug` removes the pty (a keyed
//!   radio stays keyed, as a real one does); `plug` makes a new pty at the same `LINK`.
//! - `quit` or the end of standard input.
use std::error::Error;
use std::fs::File;
use std::io::{BufRead, Read, Write};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use radioctl_core::description::RigDescription;
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Mode};
use radioctl_protocols::virtual_rig::{PanelChange, VirtualRig};
use serialport::{SerialPort, TTYPort};

type SharedRig = Arc<Mutex<VirtualRig>>;
type SharedLog = Arc<Mutex<Option<File>>>;

/// One pty with a thread that answers whoever opens the slave.
struct Cable {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    link: PathBuf,
    writer: Arc<Mutex<TTYPort>>,
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn note(log: &SharedLog, direction: &str, bytes: &[u8]) {
    if let Ok(mut guard) = log.lock() {
        if let Some(file) = guard.as_mut() {
            let _ = writeln!(file, "{direction} {}", hex(bytes));
        }
    }
}

impl Cable {
    fn plug(rig: &SharedRig, link: &Path, log: &SharedLog) -> Result<Cable, Box<dyn Error>> {
        let (mut master, slave) = TTYPort::pair()?;
        let slave_path = slave.name().ok_or("the pty slave has no name")?;
        drop(slave);
        let _ = std::fs::remove_file(link);
        symlink(&slave_path, link)?;
        let writer = Arc::new(Mutex::new(master.try_clone_native()?));
        let stop = Arc::new(AtomicBool::new(false));
        let (rig_t, stop_t, log_t) = (Arc::clone(rig), Arc::clone(&stop), Arc::clone(log));
        let writer_t = Arc::clone(&writer);
        let thread = thread::spawn(move || {
            let _ = master.set_timeout(Duration::from_millis(20));
            let started = Instant::now();
            let mut buffer = [0u8; 512];
            while !stop_t.load(Ordering::Acquire) {
                let Ok(n) = master.read(&mut buffer) else {
                    continue;
                };
                let bytes = buffer.get(..n).unwrap_or(&[]);
                note(&log_t, "<-", bytes);
                let reply = match rig_t.lock() {
                    Ok(mut rig) => rig.feed(bytes, started.elapsed()),
                    Err(_) => return,
                };
                if !reply.is_empty() {
                    note(&log_t, "->", &reply);
                    if let Ok(mut port) = writer_t.lock() {
                        let _ = port.write_all(&reply);
                    }
                }
            }
        });
        println!("plugged: {} -> {slave_path}", link.display());
        Ok(Cable {
            stop,
            thread: Some(thread),
            link: link.to_path_buf(),
            writer,
        })
    }

    /// Bytes the radio sends by itself (transceive reports).
    fn send(&self, log: &SharedLog, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        note(log, "->", bytes);
        if let Ok(mut port) = self.writer.lock() {
            let _ = port.write_all(bytes);
        }
    }

    fn unplug(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.link);
        println!("unplugged: {} removed", self.link.display());
    }
}

impl Drop for Cable {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.unplug();
        }
    }
}

fn panel(rig: &SharedRig, change: PanelChange) -> Vec<u8> {
    rig.lock()
        .map(|mut rig| rig.front_panel(change))
        .unwrap_or_default()
}

/// The directory of rig descriptions: `$RADIOCTL_RIGS`, else `./rigs` when it exists (the binaries
/// copied to a bench machine run from the directory that holds `rigs/`), else the checkout this
/// program was built from.
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

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [rig_id, link, rest @ ..] = args.as_slice() else {
        return Err("usage: virtual_radio_pty RIG_ID LINK [WIRE_LOG]".into());
    };
    let rigs = rigs_dir();
    let text = std::fs::read_to_string(rigs.join(format!("{rig_id}.toml")))?;
    let description: RigDescription = rigfile::parse(&text)?.description;
    let rig: SharedRig = Arc::new(Mutex::new(VirtualRig::from_description(&description)?));
    let log: SharedLog = Arc::new(Mutex::new(match rest.first() {
        Some(path) => Some(File::create(path)?),
        None => None,
    }));
    let link = PathBuf::from(link);
    let mut cable = Some(Cable::plug(&rig, &link, &log)?);

    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let words: Vec<&str> = line.split_whitespace().collect();
        let sent = match words.as_slice() {
            ["freq", hz] => Some(panel(&rig, PanelChange::Frequency(Hz(hz.parse()?)))),
            ["mode", name] => match Mode::from_name(name) {
                Some(mode) => Some(panel(&rig, PanelChange::Mode(mode))),
                None => {
                    println!("unknown mode {name}");
                    None
                }
            },
            ["ptt", on] => Some(panel(&rig, PanelChange::Ptt(*on == "1"))),
            ["signal", level] => Some(panel(&rig, PanelChange::Signal(level.parse()?))),
            ["split", on] => Some(panel(&rig, PanelChange::Split(*on == "1"))),
            ["rit", hz] => Some(panel(&rig, PanelChange::RitOffset(hz.parse()?))),
            ["state"] => {
                if let Ok(rig) = rig.lock() {
                    println!("{:?}", rig.state());
                }
                None
            }
            ["unplug"] => {
                cable = None;
                None
            }
            ["plug"] => {
                if cable.is_none() {
                    cable = Some(Cable::plug(&rig, &link, &log)?);
                }
                None
            }
            ["quit"] => break,
            [] => None,
            _ => {
                println!("commands: freq HZ | mode NAME | ptt 0|1 | signal N | split 0|1 | rit HZ | state | unplug | plug | quit");
                None
            }
        };
        if let (Some(bytes), Some(cable)) = (sent, cable.as_ref()) {
            cable.send(&log, &bytes);
        }
        std::io::stdout().flush()?;
    }
    Ok(())
}
