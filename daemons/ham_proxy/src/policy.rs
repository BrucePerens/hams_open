//! Ham-side policy, ported from `ham_tls/ham_policy.py` so one whitelist file serves both engines: which sites
//! may be reached, and the Part 97 station-identification log (UTC time, callsign, destination, suite).

use std::io::Write;
use std::sync::Mutex;

// [@ANCHOR: ham_proxy_whitelist]
/// Permitted destinations: every `.ham` name plus the listed domains (`example.org` exact, `*.example.org`
/// subdomains). File format: one entry per line; blank lines and `#` comments ignored.
#[derive(Debug, Default)]
pub struct Whitelist {
    exact: Vec<String>,
    suffixes: Vec<String>,
}

impl Whitelist {
    // [@ANCHOR: ham_proxy_whitelist_parse]
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

    // [@ANCHOR: ham_proxy_whitelist_permits]
    /// Whether `host` may be reached from a ham link.
    pub fn permits(&self, host: &str) -> bool {
        let h = host.to_lowercase();
        let h = h.trim_end_matches('.');
        h.ends_with(".ham")
            || self.exact.iter().any(|e| e == h)
            || self.suffixes.iter().any(|s| h.ends_with(s.as_str()))
    }
}

// [@ANCHOR: ham_proxy_format_utc]
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

// [@ANCHOR: ham_proxy_log_line]
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

/// Whitelist plus the station-ID log sink for one proxy.
pub struct Policy {
    /// Permitted destinations.
    pub whitelist: Whitelist,
    callsign: String,
    log: Mutex<Box<dyn Write + Send>>,
}

impl Policy {
    // [@ANCHOR: ham_proxy_policy_new]
    /// Build a policy; `None` when the callsign is empty (a ham link needs one).
    pub fn new(whitelist: Whitelist, callsign: &str, log: Box<dyn Write + Send>) -> Option<Self> {
        (!callsign.trim().is_empty()).then(|| Policy {
            whitelist,
            callsign: callsign.to_string(),
            log: Mutex::new(log),
        })
    }

    // [@ANCHOR: ham_proxy_policy_record]
    /// Write the station-ID line for one flow.
    pub fn record(&self, secs: u64, host: &str, suite: &str) -> std::io::Result<()> {
        let line = log_line(secs, &self.callsign, host, suite).unwrap_or_default();
        let mut w = self
            .log
            .lock()
            .map_err(|_| std::io::Error::other("log lock poisoned"))?;
        w.write_all(line.as_bytes())?;
        w.flush()
    }
}
