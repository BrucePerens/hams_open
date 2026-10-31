// SPDX-License-Identifier: AGPL-3.0-or-later
//! The receive I/Q stream interface: blocks of complex samples with sequence numbers and drop counters.
//!
//! This is what the relay consumes. Anything that yields [`IqBlock`]s implements [`IqStream`], so
//! the relay does not depend on the HPSDR wire format.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

/// One complex baseband sample, scaled so that full scale is +/-1.0.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Complex {
    /// In-phase part.
    pub re: f32,
    /// Quadrature part.
    pub im: f32,
}

/// A contiguous run of samples from one receiver (DDC).
#[derive(Debug, Clone, PartialEq)]
pub struct IqBlock {
    /// Receiver index (0 for the first DDC).
    pub stream: u8,
    /// Sequence number of the datagram this block came from (wraps at 2^32).
    pub seq: u32,
    /// Datagrams lost on this stream immediately before this one (from the sequence gap).
    pub lost_before: u32,
    /// Sample rate in hertz.
    pub sample_rate_hz: u32,
    /// Centre frequency of the receiver in hertz when the block was produced.
    pub centre_hz: u32,
    /// The samples.
    pub samples: Vec<Complex>,
}

/// Counters shared between the producer task and the consumer.
#[derive(Debug, Default)]
pub struct StreamCounters {
    blocks: AtomicU64,
    samples: AtomicU64,
    lost_packets: AtomicU64,
    dropped_blocks: AtomicU64,
    malformed: AtomicU64,
}

/// A snapshot of [`StreamCounters`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StreamStats {
    /// Blocks delivered to the consumer.
    pub blocks: u64,
    /// Samples delivered to the consumer.
    pub samples: u64,
    /// Datagrams the radio sent that never arrived (sequence gaps).
    pub lost_packets: u64,
    /// Blocks discarded because the consumer was too slow (the channel was full).
    pub dropped_blocks: u64,
    /// Datagrams rejected by the parser.
    pub malformed: u64,
}

impl StreamCounters {
    // [@ANCHOR: hpsdr_iq_counters_delivered]
    /// Record one delivered block of `n` samples.
    pub fn delivered(&self, n: usize) {
        self.blocks.fetch_add(1, Ordering::Relaxed);
        self.samples.fetch_add(n as u64, Ordering::Relaxed);
    }

    // [@ANCHOR: hpsdr_iq_counters_lost]
    /// Record `n` datagrams lost to a sequence gap.
    pub fn lost(&self, n: u32) {
        self.lost_packets.fetch_add(u64::from(n), Ordering::Relaxed);
    }

    // [@ANCHOR: hpsdr_iq_counters_dropped]
    /// Record one block dropped because the consumer lagged.
    pub fn dropped(&self) {
        self.dropped_blocks.fetch_add(1, Ordering::Relaxed);
    }

    // [@ANCHOR: hpsdr_iq_counters_malformed]
    /// Record one rejected datagram.
    pub fn malformed(&self) {
        self.malformed.fetch_add(1, Ordering::Relaxed);
    }

    // [@ANCHOR: hpsdr_iq_counters_snapshot]
    /// Read all counters.
    pub fn snapshot(&self) -> StreamStats {
        StreamStats {
            blocks: self.blocks.load(Ordering::Relaxed),
            samples: self.samples.load(Ordering::Relaxed),
            lost_packets: self.lost_packets.load(Ordering::Relaxed),
            dropped_blocks: self.dropped_blocks.load(Ordering::Relaxed),
            malformed: self.malformed.load(Ordering::Relaxed),
        }
    }
}

/// Tracks a 32-bit sequence number and reports gaps.
#[derive(Debug, Clone, Copy, Default)]
pub struct SeqTracker {
    next: Option<u32>,
}

impl SeqTracker {
    // [@ANCHOR: hpsdr_iq_seq_observe]
    /// Account for one datagram; returns how many datagrams were skipped before it. A sequence
    /// that goes backwards by less than half the range counts as a restart (0 lost).
    pub fn observe(&mut self, seq: u32) -> u32 {
        let lost = match self.next {
            None => 0,
            Some(n) => {
                let gap = seq.wrapping_sub(n);
                if gap < 0x8000_0000 {
                    gap
                } else {
                    0
                }
            }
        };
        self.next = Some(seq.wrapping_add(1));
        lost
    }
}

/// A source of receive I/Q blocks. Implemented by the HPSDR session; the relay depends on this
/// trait only.
pub trait IqStream {
    /// The next block, or `None` once the stream has ended.
    fn next_block(&mut self) -> impl Future<Output = Option<IqBlock>> + Send;
    /// Counters for this stream.
    fn stats(&self) -> StreamStats;
}

/// An [`IqStream`] fed by a bounded channel from the receive task.
#[derive(Debug)]
pub struct ChannelStream {
    rx: mpsc::Receiver<IqBlock>,
    counters: Arc<StreamCounters>,
}

impl ChannelStream {
    // [@ANCHOR: hpsdr_iq_channel_new]
    /// A stream and the sender that feeds it; `depth` is the number of blocks buffered before drops.
    pub fn new(depth: usize, counters: Arc<StreamCounters>) -> (mpsc::Sender<IqBlock>, Self) {
        let (tx, rx) = mpsc::channel(depth.max(1));
        (tx, Self { rx, counters })
    }
}

impl IqStream for ChannelStream {
    // [@ANCHOR: hpsdr_iq_channel_next_block]
    /// Wait for the next block.
    fn next_block(&mut self) -> impl Future<Output = Option<IqBlock>> + Send {
        self.rx.recv()
    }

    // [@ANCHOR: hpsdr_iq_channel_stats]
    /// Counters for this stream.
    fn stats(&self) -> StreamStats {
        self.counters.snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_accumulate() {
        // Tests [@ANCHOR: hpsdr_iq_counters_delivered]
        // Tests [@ANCHOR: hpsdr_iq_counters_lost]
        // Tests [@ANCHOR: hpsdr_iq_counters_dropped]
        // Tests [@ANCHOR: hpsdr_iq_counters_malformed]
        // Tests [@ANCHOR: hpsdr_iq_counters_snapshot]
        let c = StreamCounters::default();
        c.delivered(10);
        c.delivered(5);
        c.lost(3);
        c.dropped();
        c.malformed();
        assert_eq!(
            c.snapshot(),
            StreamStats {
                blocks: 2,
                samples: 15,
                lost_packets: 3,
                dropped_blocks: 1,
                malformed: 1
            }
        );
    }

    #[test]
    fn seq_gaps_and_wrap() {
        // Tests [@ANCHOR: hpsdr_iq_seq_observe]
        let mut t = SeqTracker::default();
        assert_eq!(t.observe(7), 0);
        assert_eq!(t.observe(8), 0);
        assert_eq!(t.observe(11), 2);
        assert_eq!(t.observe(0), 0, "backwards is a restart");
        let mut w = SeqTracker::default();
        assert_eq!(w.observe(u32::MAX), 0);
        assert_eq!(w.observe(1), 1, "wraps through zero");
    }

    #[tokio::test]
    async fn channel_stream_delivers_in_order() {
        // Tests [@ANCHOR: hpsdr_iq_channel_new]
        // Tests [@ANCHOR: hpsdr_iq_channel_next_block]
        // Tests [@ANCHOR: hpsdr_iq_channel_stats]
        let counters = Arc::new(StreamCounters::default());
        let (tx, mut s) = ChannelStream::new(2, counters.clone());
        for seq in 0..2 {
            let b = IqBlock {
                stream: 0,
                seq,
                lost_before: 0,
                sample_rate_hz: 48_000,
                centre_hz: 7_000_000,
                samples: vec![Complex::default(); 4],
            };
            assert!(tx.try_send(b).is_ok());
            counters.delivered(4);
        }
        assert!(
            tx.try_send(IqBlock {
                stream: 0,
                seq: 9,
                lost_before: 0,
                sample_rate_hz: 48_000,
                centre_hz: 0,
                samples: vec![],
            })
            .is_err(),
            "a full channel refuses rather than blocks"
        );
        assert_eq!(s.next_block().await.map(|b| b.seq), Some(0));
        assert_eq!(s.next_block().await.map(|b| b.seq), Some(1));
        assert_eq!(s.stats().samples, 8);
        drop(tx);
        assert_eq!(s.next_block().await, None);
    }
}
