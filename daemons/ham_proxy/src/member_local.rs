//! `/.ham/login` with the internet gone: the member certificate is checked here, with no round trip to hams.com.
//!
//! [`LocalVerifier`] holds a directory of member certificates (`SERIAL.pem`, public data) cached from earlier online logins and
//! checks, offline: the certificate chains to the pinned member CA, it is not on the signed deprecation list, its dates are right
//! within [`GRACE_SECONDS`] either side (a dead clock or an outage past expiry locks nobody out), and the signature over the
//! challenge verifies under the certificate's key. [`FallbackVerifier`] asks hams.com first (its answer is authoritative: a
//! refusal also deletes the cached certificate) and uses the local check only when hams.com cannot be reached.

use crate::login::{HttpAnswer, HttpVerifier, Verifier};
use openssl::asn1::Asn1Time;
use openssl::bn::BigNum;
use openssl::ecdsa::EcdsaSig;
use openssl::hash::{hash, MessageDigest};
use openssl::stack::Stack;
use openssl::x509::store::X509StoreBuilder;
use openssl::x509::verify::X509VerifyFlags;
use openssl::x509::{X509StoreContext, X509};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// How far outside the certificate's dates the local check still accepts (30 days, as the node's mTLS gate does).
pub const GRACE_SECONDS: i64 = 30 * 24 * 3600;
/// The Subject attribute carrying the member's callsign.
const CALLSIGN_OIDS: [&str; 2] = ["1.3.6.1.4.1.66926.1.1", "1.3.6.1.4.1.12348.1.1"];

/// Checks a login signature against a cached member certificate, offline.
pub struct LocalVerifier {
    /// Directory of `SERIAL.pem` member certificates (lower-case hex serial).
    pub dir: PathBuf,
    /// The pinned member CA certificates (PEM, one or more).
    pub ca_pem: PathBuf,
    /// The signed deprecation list (`cert:ISSUER_SHA256:SERIAL` lines), if the node has one.
    pub deprecation: Option<PathBuf>,
}

fn serial_ok(serial: &str) -> bool {
    !serial.is_empty() && serial.len() <= 40 && serial.bytes().all(|c| c.is_ascii_hexdigit())
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

fn trim_zeros(s: &str) -> String {
    s.trim_start_matches('0').to_ascii_lowercase()
}

impl LocalVerifier {
    fn cert_path(&self, serial: &str) -> PathBuf {
        self.dir
            .join(format!("{}.pem", serial.to_ascii_lowercase()))
    }

    fn cas(&self) -> Option<Vec<X509>> {
        X509::stack_from_pem(&std::fs::read(&self.ca_pem).ok()?).ok()
    }

    // [@ANCHOR: ham_proxy_local_chain_ok]
    /// Chain to the pinned CA (dates are checked separately, with the grace); the CA's fingerprint is the deprecation issuer.
    fn chain_ok(&self, cert: &X509, cas: &[X509]) -> bool {
        let Ok(mut b) = X509StoreBuilder::new() else {
            return false;
        };
        if b.set_flags(X509VerifyFlags::NO_CHECK_TIME).is_err() {
            return false;
        }
        for ca in cas {
            if b.add_cert(ca.clone()).is_err() {
                return false;
            }
        }
        let store = b.build();
        let (Ok(mut ctx), Ok(chain)) = (X509StoreContext::new(), Stack::<X509>::new()) else {
            return false;
        };
        ctx.init(&store, cert, &chain, |c| c.verify_cert())
            .unwrap_or(false)
    }

    // [@ANCHOR: ham_proxy_local_deprecated]
    fn deprecated(&self, cert: &X509, cas: &[X509]) -> bool {
        let Some(text) = self
            .deprecation
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
        else {
            return false;
        };
        let Some(serial) = cert
            .serial_number()
            .to_bn()
            .ok()
            .and_then(|b| b.to_hex_str().ok())
        else {
            return true;
        };
        let serial = trim_zeros(&serial);
        let issuers: Vec<String> = cas
            .iter()
            .filter_map(|c| c.digest(MessageDigest::sha256()).ok())
            .map(|d| d.iter().map(|b| format!("{b:02x}")).collect())
            .collect();
        text.lines().any(|l| {
            let mut f = l.trim().splitn(3, ':');
            f.next() == Some("cert")
                && f.next()
                    .is_some_and(|i| issuers.contains(&i.to_ascii_lowercase()))
                && f.next().is_some_and(|s| trim_zeros(s) == serial)
        })
    }

    // [@ANCHOR: ham_proxy_local_dates_ok]
    fn dates_ok(cert: &X509) -> bool {
        let t = now();
        let (Ok(early), Ok(late)) = (
            Asn1Time::from_unix(t + GRACE_SECONDS),
            Asn1Time::from_unix(t - GRACE_SECONDS),
        ) else {
            return false;
        };
        *cert.not_before() <= early && *cert.not_after() >= late
    }

    fn callsign(cert: &X509) -> Option<String> {
        cert.subject_name().entries().find_map(|e| {
            CALLSIGN_OIDS
                .contains(&e.object().to_string().as_str())
                .then(|| String::from_utf8(e.data().as_slice().to_vec()).ok())
                .flatten()
        })
    }

    // [@ANCHOR: ham_proxy_local_remember]
    /// Accept `pem` into the cache as certificate `serial` when it is that certificate and chains to the pinned CA.
    pub fn remember(&self, serial: &str, pem: &str) {
        if !serial_ok(serial) {
            return;
        }
        let (Ok(cert), Some(cas)) = (X509::from_pem(pem.as_bytes()), self.cas()) else {
            return;
        };
        let same = cert
            .serial_number()
            .to_bn()
            .ok()
            .and_then(|b| b.to_hex_str().ok())
            .is_some_and(|s| trim_zeros(&s) == trim_zeros(serial));
        if !same || !self.chain_ok(&cert, &cas) {
            return;
        }
        let tmp = self
            .dir
            .join(format!(".{}.new", serial.to_ascii_lowercase()));
        if std::fs::write(&tmp, pem).is_ok()
            && std::fs::rename(&tmp, self.cert_path(serial)).is_err()
        {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    // [@ANCHOR: ham_proxy_local_forget]
    /// hams.com refused this certificate: forget it so it cannot log in later while offline.
    pub fn forget(&self, serial: &str) {
        if serial_ok(serial) {
            let _ = std::fs::remove_file(self.cert_path(serial));
        }
    }
}

impl Verifier for LocalVerifier {
    // [@ANCHOR: ham_proxy_local_verify]
    fn verify(&self, serial: &str, nonce: &str, audience: &str, sig_b64: &str) -> Option<String> {
        if !serial_ok(serial) || !audience.starts_with("ham-login:") {
            return None;
        }
        let cert = X509::from_pem(&std::fs::read(self.cert_path(serial)).ok()?).ok()?;
        let cas = self.cas()?;
        if !self.chain_ok(&cert, &cas) || self.deprecated(&cert, &cas) || !Self::dates_ok(&cert) {
            return None;
        }
        let raw = openssl::base64::decode_block(sig_b64).ok()?;
        let sig = EcdsaSig::from_private_components(
            BigNum::from_slice(raw.get(..32)?).ok()?,
            BigNum::from_slice(raw.get(32..)?).ok()?,
        )
        .ok()?;
        let msg = format!("hams-member-cert-possession-v1\n{audience}\n{nonce}");
        let digest = hash(MessageDigest::sha256(), msg.as_bytes()).ok()?;
        let key = cert.public_key().ok()?.ec_key().ok()?;
        sig.verify(&digest, &key).ok()?.then_some(())?;
        Self::callsign(&cert)
    }
}

/// hams.com first, the local check when hams.com cannot be reached.
pub struct FallbackVerifier {
    /// The authoritative check.
    pub http: HttpVerifier,
    /// The offline check and certificate cache; `None` keeps the old behaviour (a refusal when hams.com is down).
    pub local: Option<LocalVerifier>,
}

impl Verifier for FallbackVerifier {
    // [@ANCHOR: ham_proxy_fallback_verify]
    fn verify(&self, serial: &str, nonce: &str, audience: &str, sig_b64: &str) -> Option<String> {
        match (self.http.ask(serial, nonce, audience, sig_b64), &self.local) {
            (
                HttpAnswer::Member {
                    callsign,
                    certificate_pem,
                },
                local,
            ) => {
                if let (Some(l), Some(pem)) = (local, certificate_pem) {
                    l.remember(serial, &pem);
                }
                Some(callsign)
            }
            (HttpAnswer::Refused, local) => {
                if let Some(l) = local {
                    l.forget(serial);
                }
                None
            }
            (HttpAnswer::Unreachable, Some(l)) => l.verify(serial, nonce, audience, sig_b64),
            (HttpAnswer::Unreachable, None) => None,
        }
    }
}

/// Where the cache lives, for the command line.
#[must_use]
pub fn local_from(dir: &Path, ca: &Path, deprecation: Option<&Path>) -> LocalVerifier {
    LocalVerifier {
        dir: dir.to_path_buf(),
        ca_pem: ca.to_path_buf(),
        deprecation: deprecation.map(Path::to_path_buf),
    }
}
