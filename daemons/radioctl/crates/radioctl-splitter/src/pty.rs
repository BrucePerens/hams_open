// SPDX-License-Identifier: AGPL-3.0-or-later
//! A virtual serial port: a pseudo-terminal whose slave side outside software opens as if it were a
//! rig's serial port, with the splitter behind its master side.
//!
//! Two OS threads move bytes between the master and the async side. A program closing the slave
//! shows up as a hang-up; the splitter then gives up that client's PTT (unkeying the rig if it
//! held it) and keeps the endpoint for the next program that opens it. Dropping the
//! [`PtyEndpoint`] stops the threads and removes the symlink. Unix only; Windows needs a virtual
//! COM driver or the TCP endpoint.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use radioctl_core::error::{ErrorKind, RigError};
use serialport::{SerialPort, TTYPort};
use tokio::runtime::Handle;
use tokio::sync::mpsc;

use crate::session::{ClientHandle, ClientInput, Splitter};

const SLICE: Duration = Duration::from_millis(50);
const CHANNEL: usize = 64;

/// A pty endpoint. Hand [`PtyEndpoint::path`] to the outside program as its serial port.
pub struct PtyEndpoint {
    slave: PathBuf,
    link: Option<PathBuf>,
    stop: Arc<AtomicBool>,
}

impl PtyEndpoint {
    // [@ANCHOR: radioctl_splitter_pty_path]
    /// The path programs should open: the symlink if one was asked for, else the slave device.
    pub fn path(&self) -> &Path {
        self.link.as_deref().unwrap_or(&self.slave)
    }

    // [@ANCHOR: radioctl_splitter_pty_slave_path]
    /// The slave device itself (`/dev/pts/N`).
    pub fn slave_path(&self) -> &Path {
        &self.slave
    }
}

impl Drop for PtyEndpoint {
    // [@ANCHOR: radioctl_splitter_pty_drop]
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(link) = &self.link {
            if std::fs::read_link(link).is_ok_and(|target| target == self.slave) {
                let _ = std::fs::remove_file(link);
            }
        }
    }
}

fn pty_error(detail: impl Into<String>) -> RigError {
    RigError::new(ErrorKind::LinkDown, detail)
}

// [@ANCHOR: radioctl_splitter_pty_attach]
/// Creates a pty, attaches it to `splitter` as a persistent client, and optionally links
/// `link` to its slave path (an existing file at `link` is replaced). Each program that should be
/// a separate client needs its own endpoint.
pub fn attach_pty(
    splitter: &Splitter,
    runtime: &Handle,
    link: Option<PathBuf>,
) -> Result<(PtyEndpoint, ClientHandle), RigError> {
    let (master, slave) = TTYPort::pair().map_err(|e| pty_error(format!("pty: {e}")))?;
    let slave_path = PathBuf::from(
        slave
            .name()
            .ok_or_else(|| pty_error("the pty slave has no name"))?,
    );
    drop(slave);
    if let Some(link) = &link {
        let _ = std::fs::remove_file(link);
        std::os::unix::fs::symlink(&slave_path, link)
            .map_err(|e| pty_error(format!("link {}: {e}", link.display())))?;
    }
    let writer_port = master
        .try_clone_native()
        .map_err(|e| pty_error(format!("pty clone: {e}")))?;
    let stop = Arc::new(AtomicBool::new(false));
    let (input_tx, input_rx) = mpsc::channel::<ClientInput>(CHANNEL);
    let (output_tx, output_rx) = mpsc::channel::<Vec<u8>>(CHANNEL);
    let reader_stop = Arc::clone(&stop);
    thread::Builder::new()
        .name("radioctl-pty-rx".into())
        .spawn(move || read_master(master, input_tx, reader_stop))
        .map_err(|e| pty_error(format!("pty thread: {e}")))?;
    thread::Builder::new()
        .name("radioctl-pty-tx".into())
        .spawn(move || write_master(writer_port, output_rx))
        .map_err(|e| pty_error(format!("pty thread: {e}")))?;
    let client = splitter.attach_channels(runtime, input_rx, output_tx, true);
    Ok((
        PtyEndpoint {
            slave: slave_path,
            link,
            stop,
        },
        client,
    ))
}

// [@ANCHOR: radioctl_splitter_pty_read_master]
/// Master to async side. A read error is the program closing the slave: report one disconnect and
/// wait for the next open.
fn read_master(mut master: TTYPort, input: mpsc::Sender<ClientInput>, stop: Arc<AtomicBool>) {
    if master.set_timeout(SLICE).is_err() {
        return;
    }
    let mut buffer = [0u8; 256];
    let mut hung_up = false;
    while !stop.load(Ordering::Acquire) {
        match master.read(&mut buffer) {
            Ok(n) if n > 0 => {
                hung_up = false;
                let chunk = buffer.get(..n).unwrap_or(&[]).to_vec();
                if input.blocking_send(ClientInput::Bytes(chunk)).is_err() {
                    return;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
                ) => {}
            _ => {
                if !hung_up {
                    hung_up = true;
                    if input.blocking_send(ClientInput::Disconnected).is_err() {
                        return;
                    }
                }
                thread::sleep(SLICE);
            }
        }
    }
}

// [@ANCHOR: radioctl_splitter_pty_write_master]
/// Async side to master. Bytes written while no program has the slave open are dropped.
fn write_master(mut master: TTYPort, mut output: mpsc::Receiver<Vec<u8>>) {
    if master.set_timeout(SLICE).is_err() {
        return;
    }
    while let Some(bytes) = output.blocking_recv() {
        let mut rest = bytes.as_slice();
        let mut stalls = 0;
        while !rest.is_empty() {
            match master.write(rest) {
                Ok(n) if n > 0 => rest = rest.get(n..).unwrap_or(&[]),
                Err(e) if e.kind() == io::ErrorKind::TimedOut && stalls < 40 => stalls += 1,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                _ => break,
            }
        }
    }
}
