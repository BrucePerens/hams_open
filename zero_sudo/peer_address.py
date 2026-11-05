# SPDX-License-Identifier: AGPL-3.0-or-later
"""Parsing of a transport peer address (``REMOTE_ADDR``) for the tunnel-peer test.

A listener bound to a dual-stack ``[::]`` socket reports an IPv4 peer in its IPv4-mapped IPv6 form
(``::ffff:127.0.0.1``). Comparing the text of such an address with "127.0.0.1" or "::1" fails, so the
helpers here turn the mapped form back into the IPv4 address before any test is made.
"""

import ipaddress


# [@ANCHOR: zero_sudo:parse_peer_address]
def parse_peer_address(address):
    """The :class:`ipaddress.IPv4Address` or :class:`ipaddress.IPv6Address` for ``address``, with an
    IPv4-mapped IPv6 address (``::ffff:a.b.c.d``) returned as the IPv4 address ``a.b.c.d``; ``None``
    when ``address`` is not a literal IP address (missing, empty, a host name, garbage). Pure."""
    try:
        parsed = ipaddress.ip_address(str(address or "").strip())
    except ValueError:
        return None
    if parsed.version == 6 and parsed.ipv4_mapped is not None:
        return parsed.ipv4_mapped
    return parsed


# [@ANCHOR: zero_sudo:is_loopback_peer]
def is_loopback_peer(address):
    """True when ``address`` is a loopback address (127.0.0.0/8 or ::1), including the mapped form
    ``::ffff:127.0.0.1``. This is the "the request came through the local tunnel" test; anything
    that is not a literal IP address is not loopback. Pure."""
    parsed = parse_peer_address(address)
    return parsed is not None and parsed.is_loopback
