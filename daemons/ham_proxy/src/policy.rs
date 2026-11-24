//! Ham-side policy: the whitelist and the station-ID line live in `hams_net_core::policy`; this adds the per-proxy
//! station-ID log sink.

use std::io::Write;
use std::sync::{Mutex, RwLock};

pub use hams_net_core::policy::{format_utc, log_line, Whitelist};

/// Whitelist plus the station-ID log sink for one proxy.
pub struct Policy {
    whitelist: RwLock<Whitelist>,
    callsign: String,
    log: Mutex<Box<dyn Write + Send>>,
}

impl Policy {
    // [@ANCHOR: ham_proxy_policy_new]
    /// Build a policy; `None` when the callsign is empty (a ham link needs one).
    pub fn new(whitelist: Whitelist, callsign: &str, log: Box<dyn Write + Send>) -> Option<Self> {
        (!callsign.trim().is_empty()).then(|| Policy {
            whitelist: RwLock::new(whitelist),
            callsign: callsign.to_string(),
            log: Mutex::new(log),
        })
    }

    // [@ANCHOR: ham_proxy_policy_permits]
    /// Whether the request for `path` on `host` is permitted by the current whitelist.
    pub fn permits(&self, host: &str, path: &str) -> bool {
        self.whitelist
            .read()
            .is_ok_and(|w| w.permits_path(host, path))
    }

    // [@ANCHOR: ham_proxy_policy_permits_host]
    /// Whether `host` is on the current whitelist at all (some path may be permitted); for a check made before the path is known.
    pub fn permits_host(&self, host: &str) -> bool {
        self.whitelist.read().is_ok_and(|w| w.permits(host))
    }

    // [@ANCHOR: ham_proxy_policy_replace]
    /// Swap in a new whitelist (a refreshed signed list); flows already running are not affected.
    pub fn replace(&self, whitelist: Whitelist) {
        if let Ok(mut w) = self.whitelist.write() {
            *w = whitelist;
        }
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
