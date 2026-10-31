// SPDX-License-Identifier: AGPL-3.0-or-later
//! The wire protocol: a closed, versioned request/response schema carried in length-prefixed
//! frames over a Unix domain socket. Everything is big-endian, every length is bounded, and the
//! parser never panics: an unknown tag, a short buffer, trailing bytes or an over-long string is
//! `WireError`, not UB and not a crash. There is no free-form command: a request is one of four
//! tagged variants with typed fields, and the broker executes nothing but those variants.
//!
//! Frame: `u32 length` (1..=`MAX_FRAME`) then `length` bytes of body. A body starts with the
//! protocol version byte (`VERSION`) and a tag byte. Strings are `u16 length` + UTF-8 bytes, at
//! most `MAX_STRING` bytes. A passed file descriptor is not in the body: it rides in SCM_RIGHTS
//! ancillary data alongside an `Opened` response.

/// Protocol version carried in every body. A peer speaking another version gets
/// `Status::UnsupportedVersion` and nothing else is parsed.
pub const VERSION: u8 = 1;
/// Largest frame body accepted or produced, in bytes. A length prefix above this is rejected
/// before any allocation.
pub const MAX_FRAME: usize = 4096;
/// Largest string field, in bytes.
pub const MAX_STRING: usize = 255;
/// Largest device list in one response; longer lists are truncated by the broker.
pub const MAX_DEVICES: usize = 64;

/// A parse failure. The variant names what was wrong so a fuzzer finding reads directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireError {
    /// Fewer bytes than the schema needs at this point.
    Truncated,
    /// Bytes left over after a complete message.
    Trailing,
    /// A tag, status or enum byte outside the closed set.
    BadTag(u8),
    /// A string field longer than `MAX_STRING`, not UTF-8, or containing a NUL.
    BadString,
    /// A frame length of zero or above `MAX_FRAME`.
    BadLength(u32),
    /// The version byte is not `VERSION`.
    Version(u8),
    /// More list entries than `MAX_DEVICES`.
    TooMany,
}

/// Device classes the broker knows. The numbers are the wire encoding and are never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceClass {
    /// A serial tty (CAT/PTT port).
    Serial = 1,
    /// An ALSA sound node (`/dev/snd/*`).
    Audio = 2,
}

/// Groups the provisioning action may add the caller to. Closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    /// Serial ports.
    Dialout = 1,
    /// Sound devices.
    Audio = 2,
}

impl Group {
    // [@ANCHOR: relay_broker_group_name]
    /// The system group name.
    pub fn name(self) -> &'static str {
        match self {
            Group::Dialout => "dialout",
            Group::Audio => "audio",
        }
    }
}

/// What a client may ask for. Nothing else exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Enumerate devices of one class, or all classes.
    ListDevices {
        /// Restrict to one class.
        class: Option<DeviceClass>,
    },
    /// Open a device node and return its descriptor.
    OpenDevice {
        /// The class the caller believes the node is; the broker checks.
        class: DeviceClass,
        /// Open read-only instead of read-write.
        read_only: bool,
        /// Absolute path under the device root.
        path: String,
    },
    /// Add the caller to one group (dry run unless the config enables apply).
    GrantGroup {
        /// The group.
        group: Group,
    },
    /// Add the caller to every group the relay needs (dry run unless enabled).
    ProvisionGroups,
}

/// One enumerated device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceDescriptor {
    /// Class.
    pub class: DeviceClass,
    /// Device node path.
    pub path: String,
    /// USB vendor id, 0 when not USB.
    pub vid: u16,
    /// USB product id, 0 when not USB.
    pub pid: u16,
    /// Product string from sysfs, possibly empty.
    pub product: String,
}

/// Result code of a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Done; the payload follows.
    Ok = 0,
    /// Refused by policy.
    Denied = 1,
    /// Too many requests from this peer.
    RateLimited = 2,
    /// The request did not parse.
    Malformed = 3,
    /// Wrong protocol version.
    UnsupportedVersion = 4,
    /// The action was attempted and failed (OS error).
    Failed = 5,
}

/// What the broker answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    /// Answer to `ListDevices`.
    Devices(Vec<DeviceDescriptor>),
    /// Answer to `OpenDevice`; the descriptor is in the ancillary data of the same frame.
    Opened,
    /// Answer to `GrantGroup`/`ProvisionGroups`.
    GroupReport {
        /// True when the change was made; false for a dry run.
        applied: bool,
        /// The account concerned.
        user: String,
        /// Groups that were (or would be) added; empty when nothing is missing.
        groups: Vec<String>,
    },
    /// Any non-`Ok` status with a short reason.
    Error {
        /// The status.
        status: Status,
        /// Human-readable reason, bounded.
        reason: String,
    },
}

/// A bounds-checked cursor over a byte slice.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    // [@ANCHOR: relay_broker_reader_take]
    /// The next `n` bytes, or `Truncated`.
    fn take(&mut self, n: usize) -> Result<&'a [u8], WireError> {
        let end = self.pos.checked_add(n).ok_or(WireError::Truncated)?;
        let slice = self.buf.get(self.pos..end).ok_or(WireError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }

    // [@ANCHOR: relay_broker_reader_u8]
    /// One byte.
    fn u8(&mut self) -> Result<u8, WireError> {
        self.take(1)?.first().copied().ok_or(WireError::Truncated)
    }

    // [@ANCHOR: relay_broker_reader_u16]
    /// A big-endian u16.
    fn u16(&mut self) -> Result<u16, WireError> {
        let b = self.take(2)?;
        match (b.first(), b.get(1)) {
            (Some(hi), Some(lo)) => Ok(u16::from_be_bytes([*hi, *lo])),
            _ => Err(WireError::Truncated),
        }
    }

    // [@ANCHOR: relay_broker_reader_string]
    /// A length-prefixed string of at most `MAX_STRING` bytes, UTF-8, no NUL.
    fn string(&mut self) -> Result<String, WireError> {
        let len = usize::from(self.u16()?);
        if len > MAX_STRING {
            return Err(WireError::BadString);
        }
        let bytes = self.take(len)?;
        if bytes.contains(&0) {
            return Err(WireError::BadString);
        }
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| WireError::BadString)
    }

    // [@ANCHOR: relay_broker_reader_finish]
    /// Succeeds only when every byte was consumed.
    fn finish(&self) -> Result<(), WireError> {
        if self.pos == self.buf.len() {
            Ok(())
        } else {
            Err(WireError::Trailing)
        }
    }
}

// [@ANCHOR: relay_broker_put_string]
/// Appends a length-prefixed string, truncating at a character boundary to `MAX_STRING` bytes
/// so an encoder can never produce a frame its own decoder rejects.
fn put_string(out: &mut Vec<u8>, s: &str) {
    let mut end = s.len().min(MAX_STRING);
    while !s.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let bytes = s.get(..end).unwrap_or_default().as_bytes();
    let clean: Vec<u8> = bytes.iter().copied().filter(|b| *b != 0).collect();
    out.extend_from_slice(&(clean.len() as u16).to_be_bytes());
    out.extend_from_slice(&clean);
}

// [@ANCHOR: relay_broker_device_class_from_tag]
/// The class for a wire tag.
fn device_class_from_tag(tag: u8) -> Result<DeviceClass, WireError> {
    match tag {
        1 => Ok(DeviceClass::Serial),
        2 => Ok(DeviceClass::Audio),
        other => Err(WireError::BadTag(other)),
    }
}

impl Request {
    // [@ANCHOR: relay_broker_request_encode]
    /// The body bytes (no frame header) for this request.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![VERSION];
        match self {
            Request::ListDevices { class } => {
                out.push(1);
                out.push(class.map_or(0, |c| c as u8));
            }
            Request::OpenDevice {
                class,
                read_only,
                path,
            } => {
                out.push(2);
                out.push(*class as u8);
                out.push(u8::from(*read_only));
                put_string(&mut out, path);
            }
            Request::GrantGroup { group } => {
                out.push(3);
                out.push(*group as u8);
            }
            Request::ProvisionGroups => out.push(4),
        }
        out
    }

    // [@ANCHOR: relay_broker_request_decode]
    /// Parses a body. Every failure is a `WireError`; no input panics.
    pub fn decode(body: &[u8]) -> Result<Request, WireError> {
        let mut r = Reader { buf: body, pos: 0 };
        let version = r.u8()?;
        if version != VERSION {
            return Err(WireError::Version(version));
        }
        let req = match r.u8()? {
            1 => {
                let class = match r.u8()? {
                    0 => None,
                    tag => Some(device_class_from_tag(tag)?),
                };
                Request::ListDevices { class }
            }
            2 => {
                let class = device_class_from_tag(r.u8()?)?;
                let read_only = match r.u8()? {
                    0 => false,
                    1 => true,
                    other => return Err(WireError::BadTag(other)),
                };
                let path = r.string()?;
                Request::OpenDevice {
                    class,
                    read_only,
                    path,
                }
            }
            3 => {
                let group = match r.u8()? {
                    1 => Group::Dialout,
                    2 => Group::Audio,
                    other => return Err(WireError::BadTag(other)),
                };
                Request::GrantGroup { group }
            }
            4 => Request::ProvisionGroups,
            other => return Err(WireError::BadTag(other)),
        };
        r.finish()?;
        Ok(req)
    }
}

impl Response {
    // [@ANCHOR: relay_broker_response_error]
    /// Shorthand for an error response.
    pub fn error(status: Status, reason: impl Into<String>) -> Response {
        Response::Error {
            status,
            reason: reason.into(),
        }
    }

    // [@ANCHOR: relay_broker_response_encode]
    /// The body bytes (no frame header). Device lists are cut at `MAX_DEVICES`.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![VERSION];
        match self {
            Response::Devices(devs) => {
                out.push(Status::Ok as u8);
                out.push(1);
                let n = devs.len().min(MAX_DEVICES);
                out.extend_from_slice(&(n as u16).to_be_bytes());
                for d in devs.iter().take(n) {
                    out.push(d.class as u8);
                    put_string(&mut out, &d.path);
                    out.extend_from_slice(&d.vid.to_be_bytes());
                    out.extend_from_slice(&d.pid.to_be_bytes());
                    put_string(&mut out, &d.product);
                }
            }
            Response::Opened => {
                out.push(Status::Ok as u8);
                out.push(2);
            }
            Response::GroupReport {
                applied,
                user,
                groups,
            } => {
                out.push(Status::Ok as u8);
                out.push(3);
                out.push(u8::from(*applied));
                put_string(&mut out, user);
                let n = groups.len().min(8);
                out.push(n as u8);
                for g in groups.iter().take(n) {
                    put_string(&mut out, g);
                }
            }
            Response::Error { status, reason } => {
                out.push(*status as u8);
                put_string(&mut out, reason);
            }
        }
        out
    }

    // [@ANCHOR: relay_broker_response_decode]
    /// Parses a response body. Used by the client and by the fuzz/property tests.
    pub fn decode(body: &[u8]) -> Result<Response, WireError> {
        let mut r = Reader { buf: body, pos: 0 };
        let version = r.u8()?;
        if version != VERSION {
            return Err(WireError::Version(version));
        }
        let status = match r.u8()? {
            0 => Status::Ok,
            1 => Status::Denied,
            2 => Status::RateLimited,
            3 => Status::Malformed,
            4 => Status::UnsupportedVersion,
            5 => Status::Failed,
            other => return Err(WireError::BadTag(other)),
        };
        let resp = if status != Status::Ok {
            Response::Error {
                status,
                reason: r.string()?,
            }
        } else {
            match r.u8()? {
                1 => {
                    let n = usize::from(r.u16()?);
                    if n > MAX_DEVICES {
                        return Err(WireError::TooMany);
                    }
                    let mut devs = Vec::with_capacity(n);
                    for _ in 0..n {
                        let class = device_class_from_tag(r.u8()?)?;
                        let path = r.string()?;
                        let vid = r.u16()?;
                        let pid = r.u16()?;
                        let product = r.string()?;
                        devs.push(DeviceDescriptor {
                            class,
                            path,
                            vid,
                            pid,
                            product,
                        });
                    }
                    Response::Devices(devs)
                }
                2 => Response::Opened,
                3 => {
                    let applied = match r.u8()? {
                        0 => false,
                        1 => true,
                        other => return Err(WireError::BadTag(other)),
                    };
                    let user = r.string()?;
                    let n = usize::from(r.u8()?);
                    if n > 8 {
                        return Err(WireError::TooMany);
                    }
                    let mut groups = Vec::with_capacity(n);
                    for _ in 0..n {
                        groups.push(r.string()?);
                    }
                    Response::GroupReport {
                        applied,
                        user,
                        groups,
                    }
                }
                other => return Err(WireError::BadTag(other)),
            }
        };
        r.finish()?;
        Ok(resp)
    }
}

// [@ANCHOR: relay_broker_frame_encode]
/// Prefixes a body with its length. Bodies above `MAX_FRAME` are an encoder bug and are cut to
/// `MAX_FRAME` so the peer sees a malformed frame rather than an unbounded one.
pub fn frame_encode(body: &[u8]) -> Vec<u8> {
    let body = body.get(..body.len().min(MAX_FRAME)).unwrap_or_default();
    let mut out = Vec::with_capacity(body.len().saturating_add(4));
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
    out
}

// [@ANCHOR: relay_broker_frame_body_len]
/// The body length announced by a 4-byte header, if it is within bounds.
pub fn frame_body_len(header: [u8; 4]) -> Result<usize, WireError> {
    let len = u32::from_be_bytes(header);
    if len == 0 || len as usize > MAX_FRAME {
        return Err(WireError::BadLength(len));
    }
    Ok(len as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<Request> {
        vec![
            Request::ListDevices { class: None },
            Request::ListDevices {
                class: Some(DeviceClass::Audio),
            },
            Request::OpenDevice {
                class: DeviceClass::Serial,
                read_only: true,
                path: "/dev/ttyUSB0".into(),
            },
            Request::GrantGroup {
                group: Group::Audio,
            },
            Request::ProvisionGroups,
        ]
    }

    // Tests [@ANCHOR: relay_broker_request_encode]
    // Tests [@ANCHOR: relay_broker_request_decode]
    #[test]
    fn every_request_round_trips_and_trailing_bytes_are_rejected() {
        for req in samples() {
            let body = req.encode();
            assert_eq!(Request::decode(&body), Ok(req.clone()));
            let mut long = body.clone();
            long.push(0);
            assert_eq!(Request::decode(&long), Err(WireError::Trailing));
            let short = &body[..body.len() - 1];
            assert!(Request::decode(short).is_err());
        }
        assert_eq!(Request::decode(&[]), Err(WireError::Truncated));
        assert_eq!(Request::decode(&[9, 1, 0]), Err(WireError::Version(9)));
        assert_eq!(Request::decode(&[1, 77]), Err(WireError::BadTag(77)));
        assert_eq!(
            Request::decode(&[1, 2, 1, 2, 0, 0]),
            Err(WireError::BadTag(2))
        );
    }

    // Tests [@ANCHOR: relay_broker_response_encode]
    // Tests [@ANCHOR: relay_broker_response_decode]
    // Tests [@ANCHOR: relay_broker_response_error]
    #[test]
    fn every_response_round_trips() {
        let samples = vec![
            Response::Devices(vec![DeviceDescriptor {
                class: DeviceClass::Serial,
                path: "/dev/ttyACM0".into(),
                vid: 0x0403,
                pid: 0x6001,
                product: "FT232R".into(),
            }]),
            Response::Opened,
            Response::GroupReport {
                applied: false,
                user: "ham".into(),
                groups: vec!["dialout".into()],
            },
            Response::error(Status::Denied, "disk"),
        ];
        for resp in samples {
            let body = resp.encode();
            assert_eq!(Response::decode(&body), Ok(resp.clone()));
            let mut long = body.clone();
            long.push(0);
            assert_eq!(Response::decode(&long), Err(WireError::Trailing));
        }
        assert_eq!(
            Response::decode(&[1, 0, 1, 0xff, 0xff]),
            Err(WireError::TooMany)
        );
    }

    // Tests [@ANCHOR: relay_broker_reader_take]
    // Tests [@ANCHOR: relay_broker_reader_u8]
    // Tests [@ANCHOR: relay_broker_reader_u16]
    // Tests [@ANCHOR: relay_broker_reader_string]
    // Tests [@ANCHOR: relay_broker_reader_finish]
    // Tests [@ANCHOR: relay_broker_put_string]
    #[test]
    fn strings_are_bounded_utf8_and_nul_free() {
        let mut r = Reader {
            buf: &[0, 3, b'a', 0, b'c'],
            pos: 0,
        };
        assert_eq!(r.string(), Err(WireError::BadString));
        let mut r = Reader {
            buf: &[0, 2, 0xff, 0xfe],
            pos: 0,
        };
        assert_eq!(r.string(), Err(WireError::BadString));
        let mut r = Reader {
            buf: &[1, 0],
            pos: 0,
        };
        assert_eq!(r.string(), Err(WireError::BadString));
        let mut r = Reader { buf: &[0], pos: 0 };
        assert_eq!(r.u16(), Err(WireError::Truncated));
        assert_eq!(
            r.finish(),
            Err(WireError::Trailing),
            "a failed take consumes nothing"
        );
        assert_eq!(r.u8(), Ok(0));
        assert_eq!(r.finish(), Ok(()));
        let mut r = Reader {
            buf: &[1, 2, 3],
            pos: 0,
        };
        assert_eq!(r.take(2), Ok(&[1u8, 2][..]));
        assert_eq!(r.u8(), Ok(3));
        assert_eq!(r.take(usize::MAX), Err(WireError::Truncated));

        let mut out = Vec::new();
        put_string(&mut out, &"é".repeat(200));
        let mut r = Reader { buf: &out, pos: 0 };
        let s = r.string().unwrap();
        assert!(s.len() <= MAX_STRING && s.chars().all(|c| c == 'é'));
        let mut out = Vec::new();
        put_string(&mut out, "a\0b");
        assert_eq!(out, vec![0, 2, b'a', b'b']);
    }

    // Tests [@ANCHOR: relay_broker_frame_encode]
    // Tests [@ANCHOR: relay_broker_frame_body_len]
    // Tests [@ANCHOR: relay_broker_device_class_from_tag]
    // Tests [@ANCHOR: relay_broker_group_name]
    #[test]
    fn frames_are_bounded_and_tags_closed() {
        let f = frame_encode(&[1, 2, 3]);
        assert_eq!(f, vec![0, 0, 0, 3, 1, 2, 3]);
        assert_eq!(frame_body_len([0, 0, 0, 3]), Ok(3));
        assert_eq!(frame_body_len([0, 0, 0, 0]), Err(WireError::BadLength(0)));
        assert_eq!(
            frame_body_len([0, 0, 0x10, 1]),
            Err(WireError::BadLength(0x1001))
        );
        assert_eq!(frame_encode(&vec![0; MAX_FRAME + 10]).len(), MAX_FRAME + 4);
        assert_eq!(device_class_from_tag(1), Ok(DeviceClass::Serial));
        assert_eq!(device_class_from_tag(3), Err(WireError::BadTag(3)));
        assert_eq!(Group::Dialout.name(), "dialout");
        assert_eq!(Group::Audio.name(), "audio");
    }
}
