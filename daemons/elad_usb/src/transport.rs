// SPDX-License-Identifier: AGPL-3.0-or-later
//! The one seam to the device, and a fake for tests.

use crate::Error;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// What the driver needs from a USB connection to the receiver.
pub trait Transport: Send {
    /// Vendor IN request on endpoint 0.
    fn control_in(
        &mut self,
        request: u8,
        value: u16,
        index: u16,
        buf: &mut [u8],
    ) -> Result<usize, Error>;
    /// Vendor OUT request on endpoint 0 with an optional data stage.
    fn control_out(
        &mut self,
        request: u8,
        value: u16,
        index: u16,
        data: &[u8],
    ) -> Result<usize, Error>;
    /// One bulk IN read on the sample endpoint; [`Error::Timeout`] when nothing came.
    fn bulk_in(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize, Error>;
}

/// One recorded operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// A control IN.
    In {
        /// `bRequest`.
        request: u8,
        /// `wValue`.
        value: u16,
        /// `wIndex`.
        index: u16,
        /// Length asked for.
        len: usize,
    },
    /// A control OUT.
    Out {
        /// `bRequest`.
        request: u8,
        /// `wValue`.
        value: u16,
        /// `wIndex`.
        index: u16,
        /// Data stage.
        data: Vec<u8>,
    },
}

type ReplyTable = Arc<Mutex<HashMap<(u8, u16), Vec<u8>>>>;
type BulkQueue = Arc<Mutex<VecDeque<Result<Vec<u8>, Error>>>>;

/// A scripted unit: records every control request, answers control IN by (request, value) from a table, and serves queued bulk chunks.
#[derive(Clone, Default)]
pub struct FakeTransport {
    /// Everything sent, in order.
    pub log: Arc<Mutex<Vec<Op>>>,
    replies: ReplyTable,
    chunks: BulkQueue,
}

impl FakeTransport {
    /// An empty fake.
    pub fn new() -> FakeTransport {
        FakeTransport::default()
    }

    /// Answers control IN `(request, value)` with `bytes`.
    // [@ANCHOR: elad_fake_reply]
    pub fn reply(&self, request: u8, value: u16, bytes: Vec<u8>) {
        if let Ok(mut m) = self.replies.lock() {
            m.insert((request, value), bytes);
        }
    }

    /// Queues one bulk read result.
    // [@ANCHOR: elad_fake_queue_bulk]
    pub fn queue_bulk(&self, chunk: Result<Vec<u8>, Error>) {
        if let Ok(mut q) = self.chunks.lock() {
            q.push_back(chunk);
        }
    }

    /// The recorded operations.
    pub fn ops(&self) -> Vec<Op> {
        self.log.lock().map(|l| l.clone()).unwrap_or_default()
    }
}

impl Transport for FakeTransport {
    // [@ANCHOR: elad_fake_control_in]
    fn control_in(
        &mut self,
        request: u8,
        value: u16,
        index: u16,
        buf: &mut [u8],
    ) -> Result<usize, Error> {
        if let Ok(mut l) = self.log.lock() {
            l.push(Op::In {
                request,
                value,
                index,
                len: buf.len(),
            });
        }
        let reply = self
            .replies
            .lock()
            .ok()
            .and_then(|m| m.get(&(request, value)).cloned())
            .unwrap_or_default();
        let n = reply.len().min(buf.len());
        buf[..n].copy_from_slice(&reply[..n]);
        Ok(n)
    }

    // [@ANCHOR: elad_fake_control_out]
    fn control_out(
        &mut self,
        request: u8,
        value: u16,
        index: u16,
        data: &[u8],
    ) -> Result<usize, Error> {
        if let Ok(mut l) = self.log.lock() {
            l.push(Op::Out {
                request,
                value,
                index,
                data: data.to_vec(),
            });
        }
        Ok(data.len())
    }

    // [@ANCHOR: elad_fake_bulk_in]
    fn bulk_in(&mut self, buf: &mut [u8], _timeout: Duration) -> Result<usize, Error> {
        let next = self.chunks.lock().ok().and_then(|mut q| q.pop_front());
        match next {
            None => Err(Error::Timeout),
            Some(Err(e)) => Err(e),
            Some(Ok(c)) => {
                let n = c.len().min(buf.len());
                buf[..n].copy_from_slice(&c[..n]);
                Ok(n)
            }
        }
    }
}

/// A boxed transport is a transport, so a caller can pick the real or the fake at run time.
impl Transport for Box<dyn Transport> {
    // [@ANCHOR: elad_box_control_in]
    fn control_in(
        &mut self,
        request: u8,
        value: u16,
        index: u16,
        buf: &mut [u8],
    ) -> Result<usize, Error> {
        (**self).control_in(request, value, index, buf)
    }
    // [@ANCHOR: elad_box_control_out]
    fn control_out(
        &mut self,
        request: u8,
        value: u16,
        index: u16,
        data: &[u8],
    ) -> Result<usize, Error> {
        (**self).control_out(request, value, index, data)
    }
    // [@ANCHOR: elad_box_bulk_in]
    fn bulk_in(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize, Error> {
        (**self).bulk_in(buf, timeout)
    }
}
