//! Request bodies (forms, POST): read from the ham or browser with a size cap and sent on with a plain `Content-Length`.

use crate::{indicator, ProxyError};
use std::io::{Read, Write};

/// Most request body bytes accepted (a ham link is slow; this is a form, not an upload service).
pub const MAX_BODY: usize = 1024 * 1024;

// [@ANCHOR: ham_proxy_read_head]
/// Read one request head and return it with whatever followed it in the same reads (the start of the body).
pub(crate) fn read_request<R: Read>(r: &mut R) -> Result<(Vec<u8>, Vec<u8>), ProxyError> {
    let mut all = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        if let Some(i) = all.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = all.split_off(i + 4);
            return Ok((all, rest));
        }
        let n = r.read(&mut buf)?;
        if n == 0 || all.len() + n > crate::MAX_HEAD {
            return Err(ProxyError::BadRequest);
        }
        all.extend_from_slice(&buf[..n]);
    }
}

fn headers<'a>(head: &'a str, name: &'a str) -> impl Iterator<Item = &'a str> {
    head.split("\r\n").skip(1).filter_map(move |l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

// [@ANCHOR: ham_proxy_body_read]
/// The request's body, framed by `Content-Length` or `Transfer-Encoding: chunked` (never both, never more than [`MAX_BODY`]);
/// `start` is what [`read_request`] already read past the head. `None` when the request has no body framing. Answers
/// `Expect: 100-continue` first.
pub(crate) fn read_body<S: Read + Write>(
    s: &mut S,
    head: &str,
    start: Vec<u8>,
) -> Result<Option<Vec<u8>>, ProxyError> {
    let lens: Vec<&str> = headers(head, "content-length").collect();
    let enc: Vec<&str> = headers(head, "transfer-encoding").collect();
    if lens.len() + enc.len() == 0 {
        return Ok(None);
    }
    if lens.len() + enc.len() > 1 {
        return Err(ProxyError::BadRequest);
    }
    let chunked = enc
        .first()
        .is_some_and(|e| e.eq_ignore_ascii_case("chunked"));
    if !enc.is_empty() && !chunked {
        return Err(ProxyError::BadRequest);
    }
    let want = match lens.first() {
        Some(l) if !l.is_empty() && l.bytes().all(|c| c.is_ascii_digit()) => {
            Some(l.parse::<usize>().map_err(|_| ProxyError::BadRequest)?)
        }
        Some(_) => return Err(ProxyError::BadRequest),
        None => None,
    };
    if want.is_some_and(|n| n > MAX_BODY) {
        return Err(ProxyError::BadRequest);
    }
    let mut buf = start;
    let mut tmp = [0u8; 4096];
    let mut asked = false;
    loop {
        if let Some(n) = want {
            if buf.len() >= n {
                buf.truncate(n);
                return Ok(Some(buf));
            }
        } else if let Some(b) = indicator::dechunk(&buf) {
            return if b.len() > MAX_BODY {
                Err(ProxyError::BadRequest)
            } else {
                Ok(Some(b))
            };
        }
        if buf.len() > MAX_BODY + crate::MAX_HEAD {
            return Err(ProxyError::BadRequest);
        }
        if !asked && headers(head, "expect").any(|e| e.eq_ignore_ascii_case("100-continue")) {
            s.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
            asked = true;
        }
        let n = s.read(&mut tmp)?;
        if n == 0 {
            return Err(ProxyError::BadRequest);
        }
        buf.extend_from_slice(&tmp[..n]);
    }
}

// [@ANCHOR: ham_proxy_body_with_body]
/// `head` (ending in a blank line) with its body framing replaced: `Content-Length`, `Transfer-Encoding` and `Expect` removed and, if
/// the request had a body (`body` is `Some`), one correct `Content-Length` added; the body follows.
pub(crate) fn with_body(head: &[u8], body: Option<&[u8]>) -> Vec<u8> {
    let text = String::from_utf8_lossy(head);
    let mut lines: Vec<&str> = text
        .trim_end_matches("\r\n")
        .split("\r\n")
        .filter(|l| {
            let k = l
                .split(':')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            !["content-length", "transfer-encoding", "expect"].contains(&k.as_str())
        })
        .collect();
    let len = format!("Content-Length: {}", body.map_or(0, <[u8]>::len));
    if body.is_some() {
        lines.push(&len);
    }
    let mut out = lines.join("\r\n").into_bytes();
    out.extend_from_slice(b"\r\n\r\n");
    out.extend_from_slice(body.unwrap_or(&[]));
    out
}

// [@ANCHOR: ham_proxy_body_matches_mac]
/// Whether `body` is the one a session request's MAC covered: its `X-Ham-Body-Sha256` (lower-case hex), or empty if the header is absent.
pub(crate) fn matches_mac(head: &str, body: Option<&[u8]>) -> bool {
    let body = body.unwrap_or(&[]);
    match headers(head, "x-ham-body-sha256").next() {
        Some(h) => {
            openssl::hash::hash(openssl::hash::MessageDigest::sha256(), body).is_ok_and(|d| {
                d.iter().map(|b| format!("{b:02x}")).collect::<String>() == h.to_ascii_lowercase()
            })
        }
        None => body.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A stream that reads from `r` and records what is written.
    struct Duplex(Cursor<Vec<u8>>, Vec<u8>);
    impl Read for Duplex {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(b)
        }
    }
    impl Write for Duplex {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.1.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    fn duplex(more: &[u8]) -> Duplex {
        Duplex(Cursor::new(more.to_vec()), Vec::new())
    }
    const H: &str = "POST /f HTTP/1.1\r\nHost: a\r\n";

    #[test]
    fn head_ends_at_the_blank_line_and_the_rest_is_kept() {
        // Tests [@ANCHOR: ham_proxy_read_head]
        let raw = b"POST /f HTTP/1.1\r\nHost: a\r\nContent-Length: 3\r\n\r\nabc";
        let (head, rest) = read_request(&mut &raw[..]).unwrap();
        assert!(head.ends_with(b"\r\n\r\n") && rest == b"abc");
        assert!(read_request(&mut &b"GET / HTTP/1.1\r\nHost"[..]).is_err());
    }

    #[test]
    fn content_length_and_chunked_bodies_are_read_and_the_cap_holds() {
        // Tests [@ANCHOR: ham_proxy_body_read]
        let head = format!("{H}Content-Length: 5\r\n\r\n");
        // part of the body came with the head, the rest follows
        let got = read_body(&mut duplex(b"cde"), &head, b"ab".to_vec()).unwrap();
        assert_eq!(got.unwrap(), b"abcde");
        let head = format!("{H}Transfer-Encoding: chunked\r\n\r\n");
        let got = read_body(&mut duplex(b"lo\r\n0\r\n\r\n"), &head, b"5\r\nhel".to_vec()).unwrap();
        assert!(
            got.is_none() || got.as_deref() == Some(&b"hello"[..]),
            "{got:?}"
        );
        assert!(read_body(&mut duplex(b""), &format!("{H}\r\n"), vec![])
            .unwrap()
            .is_none());
        for bad in [
            "Content-Length: 5\r\nTransfer-Encoding: chunked",
            "Content-Length: 5\r\nContent-Length: 5",
            "Content-Length: -1",
            "Content-Length: 0x5",
            "Transfer-Encoding: gzip, chunked",
            &format!("Content-Length: {}", MAX_BODY + 1),
            "Content-Length: 99999999999999999999999",
        ] {
            let head = format!("{H}{bad}\r\n\r\n");
            assert!(
                read_body(&mut duplex(b"abcdef"), &head, vec![]).is_err(),
                "{bad}"
            );
        }
        // a client that stops early, and a chunked body past the cap
        let head = format!("{H}Content-Length: 9\r\n\r\n");
        assert!(read_body(&mut duplex(b"abc"), &head, vec![]).is_err());
        let head = format!("{H}Transfer-Encoding: chunked\r\n\r\n");
        let big = format!(
            "{:x}\r\n{}\r\n0\r\n\r\n",
            MAX_BODY + 1,
            "a".repeat(MAX_BODY + 1)
        );
        assert!(read_body(&mut duplex(big.as_bytes()), &head, vec![]).is_err());
        // Expect: 100-continue is answered before the body is read
        let head = format!("{H}Content-Length: 2\r\nExpect: 100-continue\r\n\r\n");
        let mut d = duplex(b"hi");
        assert_eq!(read_body(&mut d, &head, vec![]).unwrap().unwrap(), b"hi");
        assert_eq!(d.1, b"HTTP/1.1 100 Continue\r\n\r\n");
    }

    #[test]
    fn framing_headers_are_rebuilt_around_the_body() {
        // Tests [@ANCHOR: ham_proxy_body_with_body]
        let head = format!("{H}Transfer-Encoding: chunked\r\nExpect: 100-continue\r\ncontent-length : 7\r\nAccept: */*\r\n\r\n");
        let out = String::from_utf8(with_body(head.as_bytes(), Some(b"hello"))).unwrap();
        assert_eq!(
            out,
            format!("{H}Accept: */*\r\nContent-Length: 5\r\n\r\nhello")
        );
        let none = String::from_utf8(with_body(
            format!("{H}Accept: */*\r\n\r\n").as_bytes(),
            None,
        ))
        .unwrap();
        assert_eq!(none, format!("{H}Accept: */*\r\n\r\n"));
    }

    #[test]
    fn a_session_body_must_match_its_signed_hash() {
        // Tests [@ANCHOR: ham_proxy_body_matches_mac]
        let sha = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"; // "hello"
        let head = format!("{H}X-Ham-Body-Sha256: {}\r\n\r\n", sha.to_uppercase());
        assert!(matches_mac(&head, Some(b"hello")));
        assert!(!matches_mac(&head, Some(b"hellO")) && !matches_mac(&head, None));
        // no hash header: the MAC covered an empty body
        assert!(matches_mac(&format!("{H}\r\n"), None));
        assert!(!matches_mac(&format!("{H}\r\n"), Some(b"x")));
    }
}
