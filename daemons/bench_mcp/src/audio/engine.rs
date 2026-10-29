// SPDX-License-Identifier: AGPL-3.0-or-later
//! Streams through PipeWire: `pw-record` into memory and `pw-play` of a WAV file, each aimed at one node by name. The callers
//! have already resolved the node through `devices::resolve`, which never returns a reserved device.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Without these, a target that has disappeared makes PipeWire connect the stream to the default device instead, which could be
/// a reserved one. With them the stream fails with "defined target not found".
// [@ANCHOR: audio_engine_no_fallback]
const NO_FALLBACK: &str = "node.dont-fallback=true node.dont-reconnect=true";

pub struct Recorder {
    child: Child,
    buf: Arc<Mutex<Vec<f32>>>,
    err: Arc<Mutex<String>>,
    readers: Vec<JoinHandle<()>>,
}

impl Recorder {
    /// Starts recording one channel of `node` as 32-bit float at `rate`. With `monitor`, `node` is an output (a sink) and what is
    /// recorded is what it plays.
    pub fn start(node: &str, rate: u32, monitor: bool) -> Result<Recorder, String> {
        // [@ANCHOR: bench_mcp_audio_engine_recorder_start]
        let mut child = Command::new("pw-record")
            .args([
                "-P",
                if monitor {
                    "stream.capture.sink=true node.dont-fallback=true node.dont-reconnect=true"
                } else {
                    "node.dont-fallback=true node.dont-reconnect=true"
                },
            ])
            .args([
                "--raw",
                "--target",
                node,
                "--rate",
                &rate.to_string(),
                "--channels",
                "1",
                "--format",
                "f32",
                "--latency",
                "40ms",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "pw-record is not installed (not found on PATH)".to_string()
                } else {
                    format!("pw-record: {e}")
                }
            })?;
        let buf = Arc::new(Mutex::new(Vec::<f32>::new()));
        let err = Arc::new(Mutex::new(String::new()));
        let mut out = child.stdout.take().ok_or("pw-record has no stdout")?;
        let mut errp = child.stderr.take().ok_or("pw-record has no stderr")?;
        let b = buf.clone();
        let r1 = std::thread::spawn(move || {
            let mut chunk = [0u8; 16384];
            let mut carry: Vec<u8> = Vec::new();
            while let Ok(n) = out.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                carry.extend_from_slice(&chunk[..n]);
                let whole = carry.len() / 4 * 4;
                let mut g = b.lock().unwrap_or_else(|e| e.into_inner());
                g.extend(
                    carry[..whole]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|c| f32::from_le_bytes(*c)),
                );
                carry.drain(..whole);
            }
        });
        let e = err.clone();
        let r2 = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = errp.read_to_string(&mut s);
            *e.lock().unwrap_or_else(|x| x.into_inner()) = s;
        });
        Ok(Recorder {
            child,
            buf,
            err,
            readers: vec![r1, r2],
        })
    }

    pub fn len(&self) -> usize {
        self.buf.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Waits until `n` samples have arrived. The error says what pw-record reported when nothing arrives at all.
    pub fn wait_samples(&mut self, n: usize, patience: Duration) -> Result<(), String> {
        // [@ANCHOR: bench_mcp_audio_engine_recorder_wait_samples]
        let t = Instant::now();
        loop {
            if self.len() >= n {
                return Ok(());
            }
            if let Ok(Some(status)) = self.child.try_wait() {
                std::thread::sleep(Duration::from_millis(50));
                return Err(format!(
                    "pw-record ended early ({status}): {}",
                    self.err.lock().unwrap_or_else(|e| e.into_inner()).trim()
                ));
            }
            if t.elapsed() > patience {
                return Err(format!(
                    "pw-record delivered {} of {n} samples in {:.1} s",
                    self.len(),
                    t.elapsed().as_secs_f64()
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A copy of the samples from index `from`.
    pub fn since(&self, from: usize) -> Vec<f32> {
        let g = self.buf.lock().unwrap_or_else(|e| e.into_inner());
        g.get(from..).map(|s| s.to_vec()).unwrap_or_default()
    }

    /// Stops pw-record and returns everything recorded.
    pub fn stop(mut self) -> Vec<f32> {
        self.shutdown();
        self.since(0)
    }

    fn shutdown(&mut self) {
        // [@ANCHOR: bench_mcp_audio_engine_recorder_shutdown]
        let _ = self.child.kill();
        let _ = self.child.wait();
        for h in self.readers.drain(..) {
            let _ = h.join();
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// A `pw-play` process playing one file.
pub struct Playing {
    child: Child,
    pub started: Instant,
}

impl Playing {
    /// Starts playing `path` on `node` with the stream volume `stream_volume` (1.0 leaves the file's own level).
    pub fn start(node: &str, path: &Path, stream_volume: f64) -> Result<Playing, String> {
        // [@ANCHOR: bench_mcp_audio_engine_playing_start]
        let child = Command::new("pw-play")
            .arg("-P")
            .arg(NO_FALLBACK)
            .arg("--target")
            .arg(node)
            .arg("--volume")
            .arg(format!("{stream_volume:.3}"))
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "pw-play is not installed (not found on PATH)".to_string()
                } else {
                    format!("pw-play: {e}")
                }
            })?;
        Ok(Playing {
            child,
            started: Instant::now(),
        })
    }

    /// Waits for the end of playback; after `limit` the process is killed and that is an error.
    pub fn wait(mut self, limit: Duration) -> Result<Duration, String> {
        // [@ANCHOR: bench_mcp_audio_engine_playing_wait]
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    let took = self.started.elapsed();
                    if status.success() {
                        return Ok(took);
                    }
                    let mut msg = String::new();
                    if let Some(mut e) = self.child.stderr.take() {
                        let _ = e.read_to_string(&mut msg);
                    }
                    return Err(format!("pw-play failed ({status}): {}", msg.trim()));
                }
                Ok(None) => {}
                Err(e) => return Err(format!("pw-play: {e}")),
            }
            if self.started.elapsed() > limit {
                let _ = self.child.kill();
                let _ = self.child.wait();
                return Err(format!(
                    "pw-play did not finish within {:.1} s and was stopped",
                    limit.as_secs_f64()
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Playing {
    fn drop(&mut self) {
        // [@ANCHOR: bench_mcp_audio_engine_playing_drop]
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: audio_engine_no_fallback]
    /// Needs PipeWire: `cargo test engine -- --ignored`. A target that does not exist must fail, not record the default source.
    // Tests [@ANCHOR: bench_mcp_audio_engine_playing_start]
    // Tests [@ANCHOR: bench_mcp_audio_engine_recorder_start]
    // Tests [@ANCHOR: bench_mcp_audio_engine_recorder_wait_samples]
    #[test]
    #[ignore]
    fn a_missing_target_fails_and_never_falls_back_to_the_default_device() {
        let mut rec = Recorder::start("no-such-node-for-the-test", 48000, false).unwrap();
        let e = rec.wait_samples(4800, Duration::from_secs(3)).unwrap_err();
        assert!(
            e.contains("target not found") || e.contains("ended early"),
            "{e}"
        );
        assert_eq!(rec.len(), 0);
    }

    // Tests [@ANCHOR: bench_mcp_audio_engine_playing_drop]
    #[test]
    fn dropping_a_playback_stops_the_process_that_is_still_running() {
        let child = Command::new("sleep")
            .arg("60")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        let playing = Playing {
            child,
            started: Instant::now(),
        };
        assert!(Path::new(&format!("/proc/{pid}")).exists());
        drop(playing);
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "the process is stopped and reaped"
        );
    }
}
