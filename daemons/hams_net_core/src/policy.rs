//! Ham-side policy shared by the proxy and the relay, ported from `ham_tls/ham_policy.py` so one whitelist file serves
//! every engine: which sites may be reached, and the Part 97 station-identification log line (UTC time, callsign,
//! destination, suite).

// [@ANCHOR: hams_net_core_policy_whitelist]
/// Permitted destinations: every `.ham` name plus the listed domains (`example.org` exact, `*.example.org`
/// subdomains). File format: one entry per line; blank lines and `#` comments ignored.
#[derive(Debug, Default)]
pub struct Whitelist {
    exact: Vec<String>,
    suffixes: Vec<String>,
    /// Hosts from a signed list: (host, allowed path prefixes; empty means the whole site).
    sites: Vec<(String, Vec<String>)>,
}

// [@ANCHOR: hams_net_core_policy_climbs]
/// Whether `path` could leave the prefix it starts with once the site decodes it: a `..` (or `.`) segment, also written
/// with percent escapes, a backslash or `;` parameters, or a NUL.
fn climbs(path: &str) -> bool {
    let b = path.as_bytes();
    let mut dec = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (
            b[i],
            b.get(i + 1).and_then(|&c| hex(c)),
            b.get(i + 2).and_then(|&c| hex(c)),
        ) {
            (b'%', Some(h), Some(l)) => {
                dec.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, _, _) => {
                dec.push(c);
                i += 1;
            }
        }
    }
    dec.contains(&0)
        || dec
            .split(|&c| c == b'/' || c == b'\\')
            .any(|seg| matches!(seg.split(|&c| c == b';').next(), Some(b"..") | Some(b".")))
}

impl Whitelist {
    // [@ANCHOR: hams_net_core_policy_whitelist_parse]
    /// Parse the whitelist file text.
    pub fn parse(text: &str) -> Self {
        let mut w = Whitelist::default();
        for line in text.lines() {
            let e = line.split('#').next().unwrap_or("").trim().to_lowercase();
            if let Some(rest) = e.strip_prefix("*.") {
                w.suffixes.push(format!(".{rest}"));
            } else if !e.is_empty() {
                w.exact.push(e);
            }
        }
        w
    }

    // [@ANCHOR: hams_net_core_policy_whitelist_add_site]
    /// Add an exact `host` with its allowed path prefixes (none: every path), as a signed list names it.
    pub fn add_site(&mut self, host: &str, paths: Vec<String>) {
        self.sites.push((host.to_lowercase(), paths));
    }

    // [@ANCHOR: hams_net_core_policy_whitelist_permits_path]
    /// Whether the request for `path` on `host` may be passed: the host must be permitted, and a signed-list entry that names
    /// paths limits the request to those prefixes (on a `/` boundary, query ignored). Entries without paths allow every path.
    pub fn permits_path(&self, host: &str, path: &str) -> bool {
        let h = host.to_lowercase();
        let h = h.trim_end_matches('.');
        let path = path.split(['?', '#']).next().unwrap_or("");
        let safe = !climbs(path);
        let mut listed = false;
        for (_, paths) in self.sites.iter().filter(|(s, _)| s == h) {
            listed = true;
            let root_ok = |p: &String| {
                let root = p.trim_end_matches('/');
                path == root || path.strip_prefix(root).is_some_and(|r| r.starts_with('/'))
            };
            if paths.is_empty() || (safe && paths.iter().any(root_ok)) {
                return true;
            }
        }
        !listed && self.permits(host)
    }

    // [@ANCHOR: hams_net_core_policy_whitelist_permits]
    /// Whether `host` may be reached from a ham link.
    pub fn permits(&self, host: &str) -> bool {
        let h = host.to_lowercase();
        let h = h.trim_end_matches('.');
        h.ends_with(".ham")
            || self.exact.iter().any(|e| e == h)
            || self.suffixes.iter().any(|s| h.ends_with(s.as_str()))
            || self.sites.iter().any(|(e, _)| e == h)
    }
}

// [@ANCHOR: hams_net_core_policy_format_utc]
/// `YYYY-MM-DDTHH:MM:SSZ` for a unix time.
pub fn format_utc(secs: u64) -> String {
    let (days, rem) = (secs / 86400, secs % 86400);
    // Civil date from day count (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

// [@ANCHOR: hams_net_core_policy_log_line]
/// One station-identification line; a callsign is required.
pub fn log_line(secs: u64, callsign: &str, host: &str, suite: &str) -> Option<String> {
    let cs = callsign.trim();
    if cs.is_empty() {
        return None;
    }
    Some(format!(
        "{} {} -> {host} {suite}\n",
        format_utc(secs),
        cs.to_uppercase()
    ))
}
