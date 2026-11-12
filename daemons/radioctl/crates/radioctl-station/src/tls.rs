// SPDX-License-Identifier: AGPL-3.0-or-later
//! TLS to a Home Assistant that serves only HTTPS. The owner gives the trust: either their own
//! certificate authority (a PEM file; the address is checked against the certificate's IP
//! entries) or the SHA-256 fingerprint of the server's certificate (a "pin": exactly that
//! certificate is accepted, whatever its name or expiry). Verification is never switched off, and
//! there is no option to trust the system's public roots: Home Assistant is on the owner's own
//! network, not the internet. The same crates (`tokio-rustls` with the ring provider) as the
//! `radioctld` report upload, so no new dependency family.

use std::net::IpAddr;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio_rustls::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use tokio_rustls::rustls::crypto::{ring, verify_tls12_signature, verify_tls13_signature};
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio_rustls::rustls::{
    self, ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
};
use tokio_rustls::TlsConnector;

use crate::error::{StationError, StationErrorKind};

// [@ANCHOR: radioctl_station_tls_ha_tls]
/// How to trust a Home Assistant over TLS. Exactly one of the two fields is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HaTls {
    /// The PEM text of the owner's certificate authority (or of the server's own self-signed
    /// certificate).
    pub ca_pem: Option<String>,
    /// The SHA-256 fingerprint of the server certificate, 64 hex digits (colons and spaces are
    /// ignored).
    pub pin_sha256: Option<String>,
}

fn config_err(m: impl Into<String>) -> StationError {
    StationError::new(StationErrorKind::Config, m)
}

// [@ANCHOR: radioctl_station_tls_parse_pin]
/// Reads a fingerprint: 64 hex digits, any case, with optional colons or spaces.
pub fn parse_pin(text: &str) -> Result<[u8; 32], StationError> {
    let digits: Vec<u8> = text
        .bytes()
        .filter(|b| *b != b':' && !b.is_ascii_whitespace())
        .collect();
    if digits.len() != 64 || !digits.iter().all(u8::is_ascii_hexdigit) {
        return Err(config_err(
            "the certificate fingerprint must be 64 hex digits (SHA-256)",
        ));
    }
    let mut out = [0u8; 32];
    for (slot, pair) in out.iter_mut().zip(digits.chunks(2)) {
        let text = std::str::from_utf8(pair).unwrap_or("00");
        *slot = u8::from_str_radix(text, 16).unwrap_or(0);
    }
    Ok(out)
}

// [@ANCHOR: radioctl_station_tls_pin_verifier]
/// Accepts exactly the pinned certificate; the handshake signature is still checked against its
/// key, so the peer must hold the private key.
#[derive(Debug)]
struct PinVerifier {
    pin: [u8; 32],
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl ServerCertVerifier for PinVerifier {
    // [@ANCHOR: radioctl_station_tls_verify_server_cert]
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let seen: [u8; 32] = Sha256::digest(end_entity.as_ref()).into();
        if seen == self.pin {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "the server certificate does not match the pinned fingerprint".into(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

// [@ANCHOR: radioctl_station_tls_connector]
/// The connector for `tls`: the owner's authority or the pinned certificate, nothing else.
pub fn connector(tls: &HaTls) -> Result<TlsConnector, StationError> {
    let provider = Arc::new(ring::default_provider());
    let builder = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|e| config_err(format!("TLS setup: {e}")))?;
    let config = match (&tls.ca_pem, &tls.pin_sha256) {
        (Some(pem), None) => {
            let mut roots = RootCertStore::empty();
            let mut count = 0;
            for cert in pem_certs(pem)? {
                roots
                    .add(cert)
                    .map_err(|e| config_err(format!("the CA certificate is not usable: {e}")))?;
                count += 1;
            }
            if count == 0 {
                return Err(config_err("the CA file has no certificate in it"));
            }
            builder.with_root_certificates(roots).with_no_client_auth()
        }
        (None, Some(pin)) => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinVerifier {
                pin: parse_pin(pin)?,
                provider,
            }))
            .with_no_client_auth(),
        _ => {
            return Err(config_err(
                "TLS needs exactly one of a CA certificate and a certificate fingerprint",
            ))
        }
    };
    Ok(TlsConnector::from(Arc::new(config)))
}

/// The certificates in PEM text.
fn pem_certs(pem: &str) -> Result<Vec<CertificateDer<'static>>, StationError> {
    use tokio_rustls::rustls::pki_types::pem::PemObject;
    CertificateDer::pem_slice_iter(pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| config_err(format!("the CA file is not PEM certificates: {e}")))
}

// [@ANCHOR: radioctl_station_tls_server_name]
/// The name a certificate must carry: the address itself, an IP entry.
pub fn server_name(ip: IpAddr) -> ServerName<'static> {
    ServerName::IpAddress(ip.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: radioctl_station_tls_parse_pin]
    #[test]
    fn a_fingerprint_is_64_hex_digits_in_any_notation() {
        let plain = "ab".repeat(32);
        assert_eq!(parse_pin(&plain).expect("plain"), [0xab; 32]);
        let colons = vec!["AB"; 32].join(":");
        assert_eq!(parse_pin(&colons).expect("colons"), [0xab; 32]);
        assert!(parse_pin("abcd").is_err());
        assert!(parse_pin(&"zz".repeat(32)).is_err());
    }

    // Tests [@ANCHOR: radioctl_station_tls_connector]
    // Tests [@ANCHOR: radioctl_station_tls_ha_tls]
    #[test]
    fn the_connector_needs_exactly_one_way_to_trust() {
        let none = HaTls {
            ca_pem: None,
            pin_sha256: None,
        };
        assert!(connector(&none).is_err());
        let both = HaTls {
            ca_pem: Some("x".into()),
            pin_sha256: Some("ab".repeat(32)),
        };
        assert!(connector(&both).is_err());
        let empty_ca = HaTls {
            ca_pem: Some("not a certificate".into()),
            pin_sha256: None,
        };
        assert!(connector(&empty_ca).is_err());
        let pin = HaTls {
            ca_pem: None,
            pin_sha256: Some("ab".repeat(32)),
        };
        assert!(connector(&pin).is_ok());
    }

    // Tests [@ANCHOR: radioctl_station_tls_verify_server_cert]
    // Tests [@ANCHOR: radioctl_station_tls_pin_verifier]
    #[test]
    fn only_the_pinned_certificate_is_accepted() {
        let cert = CertificateDer::from(b"a fake certificate, only its hash matters".to_vec());
        let verifier = PinVerifier {
            pin: Sha256::digest(cert.as_ref()).into(),
            provider: Arc::new(ring::default_provider()),
        };
        let name = server_name("192.168.1.20".parse().expect("ip"));
        let now = UnixTime::since_unix_epoch(std::time::Duration::from_secs(0));
        assert!(verifier
            .verify_server_cert(&cert, &[], &name, &[], now)
            .is_ok());
        let other = CertificateDer::from(b"some other certificate".to_vec());
        assert!(verifier
            .verify_server_cert(&other, &[], &name, &[], now)
            .is_err());
    }

    // Tests [@ANCHOR: radioctl_station_tls_server_name]
    #[test]
    fn the_name_checked_is_the_ip_address() {
        let name = server_name("192.168.1.20".parse().expect("ip"));
        assert!(matches!(name, ServerName::IpAddress(_)));
    }
}
