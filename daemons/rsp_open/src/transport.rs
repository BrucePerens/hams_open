// SPDX-License-Identifier: AGPL-3.0-or-later
//! The narrow USB interface the driver needs, plus a recording mock for tests.

use crate::Error;
use std::sync::{Arc, Mutex};

/// Callback receiving each completed isochronous packet's bytes (run on the USB event thread).
pub type PacketSink = Box<dyn FnMut(&[u8]) + Send + 'static>;

/// What the driver needs from a USB connection to the receiver.
pub trait Transport: Send {
    /// Vendor OUT request on EP0 without data stage (bmRequestType 0x40).
    fn vendor_out(&mut self, request: u8, value: u16, index: u16) -> Result<(), Error>;
    /// Vendor IN request on EP0 (bmRequestType 0xc0).
    fn vendor_in(
        &mut self,
        request: u8,
        value: u16,
        index: u16,
        buf: &mut [u8],
    ) -> Result<usize, Error>;
    /// SET_INTERFACE on interface 0.
    fn set_alt(&mut self, alt: u8) -> Result<(), Error>;
    /// Submit isochronous IN transfers on EP 0x81 and deliver packets to `sink` until stopped.
    fn start_iso(&mut self, sink: PacketSink) -> Result<(), Error>;
    /// Cancel isochronous transfers and wait for them to finish.
    fn stop_iso(&mut self) -> Result<(), Error>;
    /// Release interface 0 (and close).
    fn release(&mut self) -> Result<(), Error>;
}

/// One recorded operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Out {
        req: u8,
        value: u16,
        index: u16,
    },
    In {
        req: u8,
        value: u16,
        index: u16,
        len: usize,
    },
    Alt(u8),
    IsoStart,
    IsoStop,
    Release,
}

/// Mock transport: records every operation; `vendor_in` 0x48 answers 0x04 as the device does.
/// Packets queued with [`MockTransport::queue_packet`] are delivered when streaming starts.
#[derive(Clone, Default)]
pub struct MockTransport {
    pub log: Arc<Mutex<Vec<Op>>>,
    pub packets: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl MockTransport {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn ops(&self) -> Vec<Op> {
        self.log.lock().unwrap().clone()
    }
    pub fn clear(&self) {
        self.log.lock().unwrap().clear();
    }
    pub fn queue_packet(&self, p: Vec<u8>) {
        self.packets.lock().unwrap().push(p);
    }
}

impl Transport for MockTransport {
    fn vendor_out(&mut self, req: u8, value: u16, index: u16) -> Result<(), Error> {
        self.log.lock().unwrap().push(Op::Out { req, value, index });
        Ok(())
    }
    fn vendor_in(
        &mut self,
        req: u8,
        value: u16,
        index: u16,
        buf: &mut [u8],
    ) -> Result<usize, Error> {
        self.log.lock().unwrap().push(Op::In {
            req,
            value,
            index,
            len: buf.len(),
        });
        if req == 0x48 && !buf.is_empty() {
            buf[0] = 0x04;
            return Ok(1);
        }
        Ok(0)
    }
    fn set_alt(&mut self, alt: u8) -> Result<(), Error> {
        self.log.lock().unwrap().push(Op::Alt(alt));
        Ok(())
    }
    fn start_iso(&mut self, mut sink: PacketSink) -> Result<(), Error> {
        self.log.lock().unwrap().push(Op::IsoStart);
        for p in self.packets.lock().unwrap().drain(..) {
            sink(&p);
        }
        Ok(())
    }
    fn stop_iso(&mut self) -> Result<(), Error> {
        self.log.lock().unwrap().push(Op::IsoStop);
        Ok(())
    }
    fn release(&mut self) -> Result<(), Error> {
        self.log.lock().unwrap().push(Op::Release);
        Ok(())
    }
}
