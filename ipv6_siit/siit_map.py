# SPDX-License-Identifier: AGPL-3.0-or-later
"""Stateless SIIT address mapping for the hams.com Net IPv4 translation block
(docs/proposals/IPV6_MEMBER_NETWORK.md, 'IPv4 translation block' and 'AREDN interconnect').

Each IPv4 address space (domain; one per linked AREDN mesh, so overlapping 10/8 meshes never collide) gets
a /96 inside the translation /48 in RFC 6052 format: <translation /48>:<16-bit domain>:0:0:<IPv4>.
The /48 is a parameter (the /40 is not issued yet) and domain IDs come from the registry; nothing here
reads local configuration. 64:ff9b::/96 (NAT64 to the Internet) is never used.
"""

import ipaddress
import json

WELL_KNOWN_NAT64 = ipaddress.ip_network("64:ff9b::/96")


def _block(block):
    network = ipaddress.ip_network(block)
    if network.version != 6 or network.prefixlen != 48:
        raise ValueError("translation block must be an IPv6 /48: %s" % block)
    if network.overlaps(WELL_KNOWN_NAT64):
        raise ValueError("64:ff9b::/96 is never used")
    return network


def _domain(domain):
    if not isinstance(domain, int) or not 0 <= domain <= 0xFFFF:
        raise ValueError("domain ID must be 0..65535: %r" % (domain,))
    return domain


# [@ANCHOR: siit_map:domain_prefix]
def domain_prefix(block, domain):
    """The /96 of `domain` inside the translation /48 `block`."""
    network = _block(block)
    base = int(network.network_address) | (_domain(domain) << 64)
    return ipaddress.IPv6Network((base, 96))


# [@ANCHOR: siit_map:embed]
def embed(block, domain, ipv4):
    """IPv6 address of IPv4 host `ipv4` in `domain` (RFC 6052 /96 format)."""
    prefix = domain_prefix(block, domain)
    address = ipaddress.ip_address(ipv4)
    if address.version != 4:
        raise ValueError("not an IPv4 address: %s" % ipv4)
    return ipaddress.IPv6Address(int(prefix.network_address) | int(address))


# [@ANCHOR: siit_map:extract]
def extract(block, address):
    """Inverse of embed: (domain, IPv4 string). Raises ValueError outside the block or with a nonzero
    bits 80..95 (the RFC 6052 /96 format keeps them zero)."""
    network = _block(block)
    value = int(ipaddress.IPv6Address(address))
    if ipaddress.IPv6Address(value) not in network:
        raise ValueError("%s is outside %s" % (address, block))
    if (value >> 32) & 0xFFFFFFFF:
        raise ValueError("%s is not in the /96 format (bits 64..95 must be zero)" % address)
    return (value >> 64) & 0xFFFF, str(ipaddress.IPv4Address(value & 0xFFFFFFFF))


# [@ANCHOR: siit_map:next_free_domain]
def next_free_domain(used):
    """Lowest unused domain ID from 1 (0 stays reserved as an unassigned marker). The registry is the
    authority; this only proposes the number to record there."""
    taken = set(used)
    for candidate in range(1, 0x10000):
        if candidate not in taken:
            return candidate
    raise ValueError("all 65535 domain IDs are in use")


# [@ANCHOR: siit_map:jool_siit_config]
def jool_siit_config(block, domain, instance):
    """Jool SIIT instance file (jool file handling: `jool file handle`) translating exactly the /96 of
    `domain`; the gateway loads it with the domain ID and block it fetched from the registry."""
    return json.dumps(
        {"instance": instance, "framework": "netfilter", "global": {"pool6": str(domain_prefix(block, domain))}},
        indent=2,
    ) + "\n"
