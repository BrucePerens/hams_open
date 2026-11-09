# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Odoo's external RPC endpoints answer only a caller that is on the server itself, unless an operator opens them.

Odoo 19 ships four generic ways to call any public model method from outside the browser: the deprecated
`/xmlrpc/<service>`, `/xmlrpc/2/<service>` and `/jsonrpc` (credentials in the body, `auth="none"` routes, scheduled
for removal in Odoo 22), and `/json/2/<model>/<method>` (bearer API key). Every one of them reaches whatever the
authenticated account may call, so each is a standing target for a password-guessing or API-key-stealing attacker.

This has to hold for every installation of this open-source module, so it is code and not a firewall or Cloudflare
rule (an operator may add those too, see the README):

* `hams_rpc_guard.legacy_access` governs `/xmlrpc/...` and `/jsonrpc`. Default `loopback`.
* `hams_rpc_guard.json2_access` governs `/json/2/...`. Default `any`, because JSON-2 is the supported way for a
  daemon on another machine to call Odoo with a bearer key (production's FCC sync runs off-box); an operator whose
  daemons all run on the Odoo host sets it to `loopback`. An unrecognised value is read as `loopback`.
* `hams_rpc_guard.trusted_proxies`: comma-separated loopback addresses or networks of a reverse proxy on this
  machine that may forward a request whose true client is also on this machine. Default empty.
* Values of the two policies: `loopback` (only a request whose true client is on this machine), `any` (the stock
  behaviour), `none` (never).

A refused request gets a plain 404, as if the route did not exist, and is counted; the log gets one line the first
time and at every power of ten after that (family and count only: no address, login, model, method or body).

"The true client is on this machine" means the socket peer is a loopback address (or there is no peer address, which
is what a Unix socket gives) AND the request carries none of the headers a reverse proxy or Cloudflare adds. The nginx
and cloudflared that front a public Odoo run on the same machine, so their peer is loopback too: the forwarding
headers are what tell a visitor's request from a daemon's. When the peer is a trusted proxy, the request is still
local only if every client address the proxy supplied (`X-Forwarded-For`, `Forwarded`, `CF-Connecting-IP`,
`X-Real-IP`, `True-Client-IP`) is a loopback address and at least one was supplied. A proxy that adds no forwarding
header at all cannot be told apart from a daemon, which is why Odoo's own deployment guide requires `proxy_mode` and
the `X-Forwarded-*` headers.
"""
import collections
import ipaddress
import logging
import re
import threading

_logger = logging.getLogger(__name__)

LEGACY_PARAM = "hams_rpc_guard.legacy_access"
TRUSTED_PROXIES_PARAM = "hams_rpc_guard.trusted_proxies"
JSON2_PARAM = "hams_rpc_guard.json2_access"
POLICY_LOOPBACK = "loopback"
POLICY_ANY = "any"
POLICY_NONE = "none"
POLICIES = (POLICY_LOOPBACK, POLICY_ANY, POLICY_NONE)
DEFAULT_LEGACY_POLICY = POLICY_LOOPBACK
DEFAULT_JSON2_POLICY = POLICY_ANY

FAMILY_LEGACY = "legacy-rpc"
FAMILY_JSON2 = "json2"
PARAM_OF_FAMILY = {FAMILY_LEGACY: LEGACY_PARAM, FAMILY_JSON2: JSON2_PARAM}
DEFAULT_OF_FAMILY = {FAMILY_LEGACY: DEFAULT_LEGACY_POLICY, FAMILY_JSON2: DEFAULT_JSON2_POLICY}

# Headers a reverse proxy, a tunnel or Cloudflare adds. A request straight from a daemon on the box has none.
FORWARDING_HEADERS = (
    "X-Forwarded-For",
    "X-Forwarded-Host",
    "X-Forwarded-Proto",
    "X-Forwarded-Server",
    "X-Real-IP",
    "Forwarded",
    "Via",
    "CF-Connecting-IP",
    "CF-Ray",
    "True-Client-IP",
)

_denied = collections.Counter()
_denied_lock = threading.Lock()


# [@ANCHOR: hams_rpc_guard:endpoint_family]
def endpoint_family(path):
    """`legacy-rpc`, `json2` or None for the request path.

    The real routes are exactly `/jsonrpc`, anything under `/xmlrpc/` (with the separator: `/xmlrpc.php`, the
    WordPress probe scanners send, is not one) and anything under `/json/2`."""
    if not isinstance(path, str):
        return None
    if path == "/jsonrpc" or path.startswith("/xmlrpc/"):
        return FAMILY_LEGACY
    if path == "/json/2" or path.startswith("/json/2/"):
        return FAMILY_JSON2
    return None


ADDRESS_HEADERS = ("X-Forwarded-For", "Forwarded", "CF-Connecting-IP", "X-Real-IP", "True-Client-IP")
_FORWARDED_FOR = re.compile(r'for="?\[?([^;,\]"]+)', re.IGNORECASE)


# [@ANCHOR: hams_rpc_guard:parse_address]
def _parse_address(text):
    """The `ipaddress` object of `text` (an IPv4-mapped IPv6 address becomes IPv4), or None when it is not an address."""
    text = str(text).strip().strip('"')
    if text.startswith("[") and "]" in text:
        text = text[1:text.index("]")]
    elif text.count(":") == 1:  # host:port
        text = text.split(":", 1)[0]
    try:
        address = ipaddress.ip_address(text.split("%", 1)[0])
    except ValueError:
        return None
    if address.version == 6 and address.ipv4_mapped is not None:
        address = address.ipv4_mapped
    return address


# [@ANCHOR: hams_rpc_guard:parse_trusted_proxies]
def parse_trusted_proxies(value):
    """The list of `ipaddress` networks in the comma-separated parameter; entries that are not addresses are dropped."""
    networks = []
    for item in str(value or "").split(","):
        item = item.strip()
        if item:
            try:
                networks.append(ipaddress.ip_network(item, strict=False))
            except ValueError:
                _logger.warning("Ignoring %r in %s: not an address or network.", item, TRUSTED_PROXIES_PARAM)
    return networks


# [@ANCHOR: hams_rpc_guard:client_addresses]
def _client_addresses(headers):
    """Every client address the forwarding headers carry (None for one that is not an address), in no order."""
    found = []
    for name in ADDRESS_HEADERS:
        value = headers.get(name)
        if value is None:
            continue
        if name == "Forwarded":
            tokens = _FORWARDED_FOR.findall(value) or [value]
        else:
            tokens = value.split(",")
        found.extend(_parse_address(token) for token in tokens)
    return found


# [@ANCHOR: hams_rpc_guard:origin_is_local]
def origin_is_local(environ, headers, trusted_proxies=()):
    """True when the true client of the request is on this machine.

    `environ` is the WSGI environment, `headers` the request headers (case-insensitive), `trusted_proxies` a list of
    networks (see `parse_trusted_proxies`). Odoo's `proxy_mode` rewrites `REMOTE_ADDR` from `X-Forwarded-For`; the
    socket peer it replaced is kept by werkzeug's ProxyFix in `werkzeug.proxy_fix.orig`, and that is judged here."""
    original = environ.get("werkzeug.proxy_fix.orig") or {}
    peer = original.get("REMOTE_ADDR", environ.get("REMOTE_ADDR"))
    peer_address = None
    if peer not in (None, ""):
        peer_address = _parse_address(peer)
        if peer_address is None or not peer_address.is_loopback:
            return False
    if not any(headers.get(name) is not None for name in FORWARDING_HEADERS):
        return True
    if peer_address is None or not any(peer_address in network for network in trusted_proxies if network.version == peer_address.version):
        return False
    clients = _client_addresses(headers)
    return bool(clients) and all(client is not None and client.is_loopback for client in clients)


# [@ANCHOR: hams_rpc_guard:policy_allows]
def policy_allows(policy, local):
    """Whether a request of origin `local` (a bool) may use an endpoint under `policy`."""
    if policy == POLICY_ANY:
        return True
    if policy == POLICY_NONE:
        return False
    return bool(local)


# [@ANCHOR: hams_rpc_guard:normalise_policy]
def normalise_policy(value, default):
    """`value` when it is a known policy, `default` when it is unset, `loopback` (fail closed) when it is junk."""
    if value in (None, False, ""):
        return default
    value = str(value).strip().lower()
    return value if value in POLICIES else POLICY_LOOPBACK


# [@ANCHOR: hams_rpc_guard:note_denial]
def note_denial(family, policy):
    """Count a refusal and say so in the log at the first one and at every power of ten. Returns the count."""
    with _denied_lock:
        _denied[family] += 1
        count = _denied[family]
    if _is_power_of_ten(count):
        _logger.warning(
            "Refused %s request(s) to the %s endpoints (%s=%s); the count is per worker since it started. "
            "An operator who needs them sets that system parameter to 'any' or 'loopback'.",
            count, family, PARAM_OF_FAMILY[family], policy,
        )
    return count


# [@ANCHOR: hams_rpc_guard:denial_counts]
def denial_counts():
    """A copy of the per-family refusal counts of this worker."""
    with _denied_lock:
        return dict(_denied)


# [@ANCHOR: hams_rpc_guard:is_power_of_ten]
def _is_power_of_ten(count):
    while count >= 10 and count % 10 == 0:
        count //= 10
    return count == 1
