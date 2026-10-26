// SPDX-License-Identifier: AGPL-3.0-or-later
//! Capture from one of the VNWA's two USB codecs (PCM2900C: stereo, 16 bit, 48 kHz is the best they offer). The stream
//! runs continuously into a ring buffer with a running frame counter, so a caller can mark a moment (the instant it
//! sent a set-up block), skip the frames before the unit has settled and take the frames after.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub const SAMPLE_RATE: u32 = 48_000;
/// How many frames the ring keeps (two seconds).
const KEEP_FRAMES: usize = 2 * SAMPLE_RATE as usize;

struct Ring {
    frames: std::collections::VecDeque<[i16; 2]>,
    /// Frames received since the stream started (not reduced when old frames are dropped).
    total: u64,
}

pub struct Capture {
    ring: Arc<(Mutex<Ring>, Condvar)>,
    _stream: cpal::Stream,
}

/// The input devices' ids (for the ALSA host, for example `alsa:hw:CARD=CODEC,DEV=0`), in the host's order.
pub fn input_names() -> Result<Vec<String>, String> {
    let host = cpal::default_host();
    let devices = host.input_devices().map_err(|e| format!("audio: {e}"))?;
    Ok(devices
        .filter_map(|d| d.id().ok())
        .map(|id| id.to_string())
        .collect())
}

impl Capture {
    /// Opens the input device whose id contains `needle` (case-insensitive, for example `hw:CARD=CODEC,`) at 48 kHz, stereo, 16 bit.
    pub fn open(needle: &str) -> Result<Capture, String> {
        let host = cpal::default_host();
        let wanted = needle.to_lowercase();
        let device = host
            .input_devices()
            .map_err(|e| format!("audio: {e}"))?
            .find(|d| {
                d.id()
                    .map(|id| id.to_string().to_lowercase().contains(&wanted))
                    .unwrap_or(false)
            })
            .ok_or_else(|| format!("no audio input device with \"{needle}\" in its id"))?;
        let config = cpal::StreamConfig {
            channels: 2,
            sample_rate: SAMPLE_RATE,
            buffer_size: cpal::BufferSize::Default,
        };
        let ring = Arc::new((
            Mutex::new(Ring {
                frames: std::collections::VecDeque::new(),
                total: 0,
            }),
            Condvar::new(),
        ));
        let writer = ring.clone();
        let stream = device
            .build_input_stream(
                config,
                move |data: &[i16], _| {
                    let (lock, wake) = &*writer;
                    let mut ring = lock.lock().unwrap();
                    for f in data.as_chunks::<2>().0 {
                        ring.frames.push_back([f[0], f[1]]);
                        ring.total += 1;
                    }
                    while ring.frames.len() > KEEP_FRAMES {
                        ring.frames.pop_front();
                    }
                    wake.notify_all();
                },
                |e| eprintln!("audio stream error: {e}"),
                None,
            )
            .map_err(|e| format!("audio: {e}"))?;
        stream.play().map_err(|e| format!("audio: {e}"))?;
        Ok(Capture {
            ring,
            _stream: stream,
        })
    }

    /// The running frame count now: the "mark" for `frames_after`.
    pub fn mark(&self) -> u64 {
        self.ring.0.lock().unwrap().total
    }

    /// `n` frames starting `skip` frames after `mark`, waiting (up to `timeout`) until they have arrived.
    pub fn frames_after(
        &self,
        mark: u64,
        skip: usize,
        n: usize,
        timeout: Duration,
    ) -> Result<Vec<[i16; 2]>, String> {
        let (lock, wake) = &*self.ring;
        let first = mark + skip as u64;
        let last = first + n as u64;
        let deadline = Instant::now() + timeout;
        let mut ring = lock.lock().unwrap();
        while ring.total < last {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!(
                    "audio: {} of {} frames arrived in time",
                    ring.total.saturating_sub(first),
                    n
                ));
            }
            ring = wake.wait_timeout(ring, left).unwrap().0;
        }
        let oldest = ring.total - ring.frames.len() as u64;
        if first < oldest {
            return Err("audio: the frames asked for were already dropped from the ring".into());
        }
        let start = (first - oldest) as usize;
        Ok(ring.frames.iter().skip(start).take(n).copied().collect())
    }
}
