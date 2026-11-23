# SPDX-License-Identifier: AGPL-3.0-or-later
"""Gateway-side resolver config for AREDN (docs/proposals/IPV6_MEMBER_NETWORK.md, 'AREDN interconnect', item 4).

An AREDN node's dnsmasq keeps answering its own mesh names; the gateway adds one drop-in that forwards only the private
.ham root to the hams.com Net resolvers. Nothing here touches the mesh's own domain. No resolvers (the /40 is unset)
gives an empty file, so the node's resolver is unchanged.
"""

import ipaddress


# [@ANCHOR: aredn_resolver:dnsmasq_conf]
def dnsmasq_conf(resolvers):
    """dnsmasq drop-in text forwarding .ham to the given IPv6 resolver addresses; ValueError for anything that is not an
    IPv6 address (no injection of other directives). Empty text for no resolvers."""
    lines = []
    for resolver in resolvers:
        try:
            address = ipaddress.IPv6Address(str(resolver).strip())
        except ValueError:
            raise ValueError("resolver must be an IPv6 address: %r" % (resolver,)) from None
        lines.append("server=/ham/%s" % address)
    if not lines:
        return ""
    return "# hams.com Net: forward the private .ham root; AREDN's own names stay with the node\n" + "\n".join(lines) + "\n"
