//! What may go on an RF interface (47 CFR 97.113(a)(4): no encryption). Mirrors the forward chain that
//! `ham_tls/ham_boundary.py::render_ruleset` renders, rule for rule and in the same order.

/// Ports and encrypted-service lists; same defaults as `ham_boundary.DEFAULTS`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// WireGuard overlay listen port (always counted as encrypted UDP).
    pub wg_port: u16,
    /// Integrity-only TLS port (ham_tls).
    pub ham_tls_port: u16,
    /// Readable TCP services allowed toward RF.
    pub plain_tcp_ports: Vec<u16>,
    /// Readable UDP services allowed toward RF.
    pub plain_udp_ports: Vec<u16>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            wg_port: 51820,
            ham_tls_port: 8443,
            plain_tcp_ports: vec![80],
            plain_udp_ports: vec![53],
        }
    }
}

/// WireGuard, AREDN tunnel WireGuard, IKE, IPsec NAT-T, OpenVPN.
const ENCRYPTED_UDP: [u16; 5] = [51820, 5525, 500, 4500, 1194];
/// SSH, HTTPS, DoT, mail TLS, MQTT TLS, OpenVPN.
const ENCRYPTED_TCP: [u16; 8] = [22, 443, 853, 993, 995, 465, 8883, 1194];

/// A flow as seen at the forwarding point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proto {
    /// TCP to a destination port.
    Tcp(u16),
    /// UDP to a destination port.
    Udp(u16),
    /// ICMP echo request.
    IcmpEcho,
    /// ICMPv6 echo request.
    Icmpv6Echo,
    /// IPsec ESP (payload encryption).
    Esp,
    /// Any other protocol or ICMP type.
    Other,
}

/// Outcome for one packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// May go on the interface.
    Forward,
    /// Refused because it is (or looks like) encrypted.
    RefusedEncrypted,
    /// Refused because it is not on the allow list.
    RefusedOther,
}

impl Config {
    // [@ANCHOR: hams_net_core_boundary_check]
    /// Refuse a config that would allow an encrypted service port toward RF.
    pub fn check(&self) -> Result<(), String> {
        let tcp_ok = self.plain_tcp_ports.iter().chain(Some(&self.ham_tls_port));
        let mut udp_bad = ENCRYPTED_UDP.to_vec();
        udp_bad.push(self.wg_port);
        if tcp_ok.into_iter().any(|p| ENCRYPTED_TCP.contains(p))
            || self.plain_udp_ports.iter().any(|p| udp_bad.contains(p))
        {
            return Err("an encrypted service port cannot be allowed toward the mesh".into());
        }
        Ok(())
    }

    // [@ANCHOR: hams_net_core_boundary_decide]
    /// Verdict for a packet leaving through an interface; `to_rf` marks an RF interface, `new` a first
    /// packet of a flow (otherwise it belongs to an established flow). Not-RF is always forwarded.
    pub fn decide(&self, to_rf: bool, proto: Proto, new: bool) -> Verdict {
        if !to_rf {
            return Verdict::Forward;
        }
        match proto {
            Proto::Esp => return Verdict::RefusedEncrypted,
            Proto::Udp(p) if p == self.wg_port || ENCRYPTED_UDP.contains(&p) => {
                return Verdict::RefusedEncrypted
            }
            Proto::Tcp(p) if ENCRYPTED_TCP.contains(&p) => return Verdict::RefusedEncrypted,
            _ => {}
        }
        let allowed = match proto {
            Proto::Tcp(p) => new && (self.plain_tcp_ports.contains(&p) || p == self.ham_tls_port),
            Proto::Udp(p) => new && self.plain_udp_ports.contains(&p),
            Proto::IcmpEcho | Proto::Icmpv6Echo => true,
            _ => false,
        };
        if allowed || !new {
            Verdict::Forward
        } else {
            Verdict::RefusedOther
        }
    }
}
