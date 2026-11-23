# SPDX-License-Identifier: AGPL-3.0-or-later
"""AREDN names as .ham zone data (docs/proposals/IPV6_MEMBER_NETWORK.md, 'AREDN interconnect', item 4).

A host `<node>.local.mesh` of a gatewayed mesh is published as `<node>.<mesh>.<callsign>.ham` with the AAAA of its
SIIT address (siit_map); an advertised service becomes a CNAME `<service>.<mesh>.<callsign>.ham` to its host. Records
come only from the registry's active domains and from the gateways handed in, so a withdrawn or suspended gateway (not
handed in) publishes nothing. With no translation block (the /40 is unset) the zone data is empty.
"""

import json
import re

import siit_map

LOCAL_MESH = ".local.mesh"
_LABEL = re.compile(r"^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$")


# [@ANCHOR: aredn_names:label]
def _label(text, what):
    """A DNS label from a node, mesh, service or callsign name: lowercase, '/' becomes '-', .local.mesh dropped; spaces become '-' in
    service names only."""
    value = (text or "").strip().lower()
    if value.endswith(LOCAL_MESH):
        value = value[: -len(LOCAL_MESH)]
    value = value.replace("/", "-")
    if what == "service":
        value = value.replace(" ", "-")
    if not _LABEL.match(value):
        raise ValueError("%s is not usable as a DNS label: %r" % (what, text))
    return value


# [@ANCHOR: aredn_names:ham_name]
def ham_name(host, mesh, callsign):
    """`<host>.<mesh>.<callsign>.ham` for an AREDN node name (with or without .local.mesh)."""
    return "%s.%s.%s.ham" % (_label(host, "host"), _label(mesh, "mesh"), _label(callsign, "callsign"))


# [@ANCHOR: aredn_names:local_mesh_name]
def local_mesh_name(name, mesh, callsign):
    """Inverse of ham_name: `<host>.local.mesh`. ValueError when the name is not exactly <host>.<mesh>.<callsign>.ham."""
    suffix = ".%s.%s.ham" % (_label(mesh, "mesh"), _label(callsign, "callsign"))
    value = name.strip().lower().rstrip(".")
    host = value[: -len(suffix)] if value.endswith(suffix) else ""
    if not _LABEL.match(host):
        raise ValueError("%r is not a host under %s" % (name, suffix[1:]))
    return host + LOCAL_MESH


# [@ANCHOR: aredn_names:zone_text]
def zone_text(export, gateways, ttl=300):
    """Records (owners relative to the ham. zone) for every gateway whose mesh is an active domain in the registry export
    (JSON text or dict, as siit_map.gateway_configs reads it). A gateway is {"mesh", "callsign", "hosts": {node: ipv4},
    "services": [(service name, node)]}. A service naming an unknown node is skipped. Empty text without a translation block."""
    data = json.loads(export) if isinstance(export, (str, bytes)) else export
    block = data.get("block")
    if block is None:
        return ""
    domains = {row["mesh"]: row["domain"] for row in data.get("domains", [])}
    out = []
    for gateway in gateways:
        domain = domains.get(gateway["mesh"])
        if domain is None:
            continue
        mesh, call = _label(gateway["mesh"], "mesh"), _label(gateway["callsign"], "callsign")
        known = set()
        for host, ipv4 in sorted(gateway.get("hosts", {}).items()):
            label = _label(host, "host")
            known.add(label)
            out.append("%s.%s.%s %d IN AAAA %s" % (label, mesh, call, ttl, siit_map.embed(block, domain, ipv4)))
        for service, host in gateway.get("services", ()):
            target = _label(host, "host")
            if target in known:
                out.append("%s.%s.%s %d IN CNAME %s.%s.%s.ham." % (_label(service, "service"), mesh, call, ttl, target, mesh, call))
    return "".join(line + "\n" for line in out)
