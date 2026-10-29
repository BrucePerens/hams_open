// SPDX-License-Identifier: AGPL-3.0-or-later
//! A raw look at one point: the audio of both codecs from before the set-up block is sent until a given time after the send ends, for
//! finding out when the unit starts emitting and where its level steps are (`examples/envelope.rs`). Nothing is judged or discarded here.

use std::time::Duration;

use crate::capture::SAMPLE_RATE;
use crate::setup::{setup_block, SetupParams};
use crate::sweep::Vna;

/// One point's raw audio and timing.
pub struct RawPoint {
    /// The frames of codec 1 and codec 2 from `before_ms` before the send began, `[left, right]` each.
    pub codec1: Vec<[i16; 2]>,
    pub codec2: Vec<[i16; 2]>,
    /// How long the block and the 0x0d read took to send, in milliseconds.
    pub send_ms: f64,
    /// Where the send ended in the frames: the frame index of the end of the send, from the clock.
    pub send_end_frame: usize,
}

impl Vna {
    /// Sends the set-up block for `freq_hz` as `measure` does and returns the audio from `before_ms` before the send until `after_ms` after it
    /// ends. Errors are not retried.
    pub fn trace_point(
        &self,
        freq_hz: f64,
        before_ms: f64,
        after_ms: f64,
    ) -> Result<RawPoint, String> {
        let c = &self.cfg;
        let (usb, cap11, cap21) = (&self.session.usb, &self.session.s11, &self.session.s21);
        let block = setup_block(&SetupParams {
            freq_hz,
            previous_hz: freq_hz,
            n_rf: 10,
            n_lo: 9,
            if_hz: c.if_hz,
            point_ms: c.point_ms,
            counter: 3,
            mode: 0,
            rf_level: c.rf_level,
        })?;
        usb.read(0x58, c.rf_level, c.rf_level, 64)?;
        let fr = |ms: f64| (ms / 1000.0 * SAMPLE_RATE as f64).round() as usize;
        let (m11, m21) = (cap11.mark(), cap21.mark());
        let started = std::time::Instant::now();
        usb.send_all(&block)?;
        usb.read(0x0d, 0x0303, 0, 64)?;
        let send_ms = started.elapsed().as_secs_f64() * 1000.0;
        let before = fr(before_ms);
        let (m11, m21) = (
            m11.saturating_sub(before as u64),
            m21.saturating_sub(before as u64),
        );
        let n = before + fr(send_ms) + fr(after_ms);
        let t = Duration::from_secs(3);
        Ok(RawPoint {
            codec1: cap11.frames_after(m11, 0, n, t)?,
            codec2: cap21.frames_after(m21, 0, n, t)?,
            send_ms,
            send_end_frame: before + fr(send_ms),
        })
    }
}

/// The first block of an amplitude envelope that reaches half of its largest value: where a tone starts. `None` for silence.
pub fn onset_block(env: &[f64]) -> Option<usize> {
    // [@ANCHOR: vnwa_open_emission_onset]
    let top = env.iter().cloned().fold(0.0, f64::max);
    if top <= 0.0 {
        return None;
    }
    env.iter().position(|v| *v >= 0.5 * top)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: vnwa_open_emission_onset]
    #[test]
    fn the_if_tone_starts_61_ms_and_the_18_khz_burst_1_ms_after_the_end_of_the_send_in_a_recorded_point(
    ) {
        // Recorded 2026-10-29 on the unit (10 MHz, IF 4 kHz, 60 ms per point, codec 1 left channel): amplitude of the tone per 1 ms block,
        // the first block 20 ms before the send began; the send ended at block 30. Only the stretch around the starts is kept.
        let tone: Vec<f64> = [
            vec![0.0; 30],
            vec![38.0],
            vec![1.0, 1.0, 0.0, 1.0],
            vec![0.0; 56],
            vec![783.0, 5708.0, 5706.0, 5707.0, 5707.0, 5708.0],
        ]
        .concat();
        let burst: Vec<f64> = [
            vec![0.0; 30],
            vec![496.0, 3733.0, 3736.0, 3735.0, 3735.0, 3735.0],
        ]
        .concat();
        let send_end = 30;
        // 783 is the partial block the tone starts in; half of the steady level is first reached in the next block.
        assert_eq!(onset_block(&tone), Some(92));
        assert_eq!(onset_block(&tone).unwrap() - send_end, 62);
        assert_eq!(onset_block(&burst).unwrap() - send_end, 1);
        assert_eq!(onset_block(&[0.0, 0.0]), None);
    }
}
