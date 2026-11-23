# SPDX-License-Identifier: AGPL-3.0-or-later
"""Part 97 boundary rules for a hams-node gateway (docs/proposals/IPV6_MEMBER_NETWORK.md, 'AREDN interconnect').
Whatever is forwarded toward the AREDN side goes onto RF, where encryption is forbidden. WireGuard (the internet
overlay) terminates at the gateway; this module renders an nftables ruleset that forwards toward the mesh only
the listed plaintext or integrity-only (ham_tls) services, drops everything else, counts the refusals, and logs
each new flow for station identification."""

DEFAULTS = {
    "mesh_if": "br-dtdlink",  # interface(s) toward AREDN (RF side)
    "wg_if": "wg-hams",  # WireGuard interface of the overlay; terminates here
    "wg_port": 51820,  # overlay WireGuard listen port (UDP), accepted from the internet side only
    "ham_tls_port": 8443,  # integrity-only TLS (ham_tls); suite enforced by the proxy, never offered encrypted
    "plain_tcp_ports": [80],  # readable protocols allowed toward the mesh
    "plain_udp_ports": [53],
    "nflog_group": 97,  # station-identification consumer reads this group (ham_policy.log_connection)
}
# Encrypted protocols that are dropped by name in addition to the default drop, so the counter says why.
ENCRYPTED_UDP = (51820, 5525, 500, 4500, 1194)  # WireGuard, AREDN tunnel WireGuard, IKE, IPsec NAT-T, OpenVPN
ENCRYPTED_TCP = (22, 443, 853, 993, 995, 465, 8883, 1194)  # SSH, HTTPS, DoT, mail TLS, MQTT TLS, OpenVPN
ENCRYPTED_PROTOCOLS = ("esp",)  # IPsec payload encryption (AH is integrity-only and not listed)




# [@ANCHOR: ham_boundary:render_ruleset]
def render_ruleset(config=None):
    """Return the nftables ruleset text for `config` (keys of DEFAULTS; missing keys take the default)."""
    cfg = dict(DEFAULTS)
    cfg.update(config or {})
    def _ports(ports):
        return "{ %s }" % ", ".join(str(int(p)) for p in sorted(set(ports)))

    mesh, wg = cfg["mesh_if"], cfg["wg_if"]
    udp_bad = set(ENCRYPTED_UDP) | {int(cfg["wg_port"])}
    tcp_bad = set(ENCRYPTED_TCP)
    tcp_ok = set(cfg["plain_tcp_ports"]) | {cfg["ham_tls_port"]}
    if tcp_ok & tcp_bad or set(cfg["plain_udp_ports"]) & udp_bad:
        raise ValueError("an encrypted service port cannot be allowed toward the mesh")
    return """table inet hams_boundary {
	counter refused_encrypted { }
	counter refused_other { }
	counter forwarded_to_mesh { }

	chain input {
		type filter hook input priority 0; policy accept;
		# the WireGuard overlay terminates here, from the internet side only
		iifname "%(mesh)s" udp dport %(wg_port)d drop
	}

	chain forward {
		type filter hook forward priority -10; policy accept;
		oifname != "%(mesh)s" accept
		# encrypted traffic never leaves toward RF
		oifname "%(mesh)s" meta l4proto %(protos)s counter name refused_encrypted drop
		oifname "%(mesh)s" udp dport %(udp_bad)s counter name refused_encrypted drop
		oifname "%(mesh)s" tcp dport %(tcp_bad)s counter name refused_encrypted drop
		# only listed readable or integrity-only services are forwarded; each new flow is logged for station ID
		oifname "%(mesh)s" tcp dport %(tcp_ok)s ct state new log group %(group)d prefix "hams-station-id " counter name forwarded_to_mesh accept
		oifname "%(mesh)s" udp dport %(udp_ok)s ct state new log group %(group)d prefix "hams-station-id " counter name forwarded_to_mesh accept
		oifname "%(mesh)s" ct state established,related counter name forwarded_to_mesh accept
		oifname "%(mesh)s" icmp type echo-request accept
		oifname "%(mesh)s" icmpv6 type echo-request accept
		oifname "%(mesh)s" counter name refused_other drop
	}
}
""" % {
        "mesh": mesh, "wg": wg, "wg_port": int(cfg["wg_port"]), "group": int(cfg["nflog_group"]),
        "protos": "{ %s }" % ", ".join(ENCRYPTED_PROTOCOLS), "udp_bad": _ports(udp_bad), "tcp_bad": _ports(tcp_bad),
        "tcp_ok": _ports(tcp_ok), "udp_ok": _ports(cfg["plain_udp_ports"]),
    }
