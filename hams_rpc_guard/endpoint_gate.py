# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Odoo's external RPC endpoints answer only callers on an allowed network (by default: this machine).

Odoo 19 ships four generic ways to call any public model method from outside the browser: the deprecated
`/xmlrpc/<service>`, `/xmlrpc/2/<service>` and `/jsonrpc` (credentials in the body, `auth="none"` routes, scheduled
for removal in Odoo 22), and `/json/2/<model>/<method>` (bearer API key). Every one of them reaches whatever the
authenticated account may call, so each is a standing target for a password-guessing or API-key-stealing attacker.

This has to hold for every installation of this open-source module, so it is code and not a firewall or Cloudflare
rule (an operator may add those too, see the README). The settings (system parameters; README "Settings"):

* `hams_rpc_guard.allowed_networks`: comma-separated IPv4/IPv6 networks whose callers may use the endpoints.
  Default `127.0.0.0/8,::1/128` (this machine only). `none` allows no network (the Unix socket only, see
  `unix_listener.py`). A value that does not parse is a configuration error: refused when it is written
  (`models/ir_config_parameter.py`), and if it is in the database anyway, logged as an error and read as
  "no network" (fail closed), never as open.
* `hams_rpc_guard.legacy_access`: `networks` (default: `/xmlrpc...` and `/jsonrpc` follow `allowed_networks`) or
  `none` (closed to everyone). Junk is read as `none`.
* `hams_rpc_guard.trusted_proxies`: networks of the reverse proxies whose client-address header is believed.
* `hams_rpc_guard.client_address_header`: the one header (`CF-Connecting-IP`, `X-Real-IP` or `True-Client-IP`) a
  trusted proxy sets (overwriting what the client sent) to the true client address.
* `hams_rpc_guard.rpc_transport`: `tcp` (default), `unix` (the endpoints are refused on TCP and served only on the
  Unix socket) or `both`.

Who the true client is (`resolve_client`):

1. The socket peer (werkzeug's ProxyFix record `werkzeug.proxy_fix.orig` when Odoo's `proxy_mode` has rewritten
   `REMOTE_ADDR`, because the rewritten value comes from a header the client controls).
2. No forwarding header at all: the peer is the client (a daemon calling Odoo directly).
3. A forwarding header and the peer is in `trusted_proxies`: the client is the single address in
   `client_address_header`. Missing, several, or not an address: unknown, refused.
4. A forwarding header from any other peer: unknown, refused. A visitor can add `X-Forwarded-For: 127.0.0.1`
   but cannot become a trusted proxy, so forging headers gains nothing.

A refused request gets a plain 404, as if the route did not exist. Requests are counted per endpoint family,
network class (`loopback`, `private`, `link-local`, `public`, `unknown`, `unix`) and outcome; the log gets one line
the first time and at every power of ten after that, with those names and the count only: never an address, login,
model, method or body.
"""
import collections
import ipaddress
import logging
import re
import threading

_logger = logging.getLogger(__name__)

# [@ANCHOR: hams_rpc_guard:gate_parameters]
ALLOWED_NETWORKS_PARAM = "hams_rpc_guard.allowed_networks"
LEGACY_PARAM = "hams_rpc_guard.legacy_access"
TRUSTED_PROXIES_PARAM = "hams_rpc_guard.trusted_proxies"
CLIENT_ADDRESS_HEADER_PARAM = "hams_rpc_guard.client_address_header"
TRANSPORT_PARAM = "hams_rpc_guard.rpc_transport"
UNIX_PEERS_PARAM = "hams_rpc_guard.unix_peers"

DEFAULT_ALLOWED_NETWORKS = "127.0.0.0/8,::1/128"
NO_NETWORK = "none"
LEGACY_NETWORKS = "networks"
LEGACY_NONE = "none"
TRANSPORT_TCP = "tcp"
TRANSPORT_UNIX = "unix"
TRANSPORT_BOTH = "both"
TRANSPORTS = (TRANSPORT_TCP, TRANSPORT_UNIX, TRANSPORT_BOTH)
TRANSPORT_CLOSED = "closed"  # what a junk transport value is read as: no transport serves the endpoints
# Single-address headers a proxy can set to its own peer. X-Forwarded-For is not one: it is a list the client starts.
CLIENT_ADDRESS_HEADERS = ("CF-Connecting-IP", "X-Real-IP", "True-Client-IP")

FAMILY_LEGACY = "legacy-rpc"
FAMILY_JSON2 = "json2"

CLASS_LOOPBACK = "loopback"
CLASS_PRIVATE = "private"
CLASS_LINK_LOCAL = "link-local"
CLASS_PUBLIC = "public"
CLASS_UNKNOWN = "unknown"
CLASS_UNIX = "unix"

# The WSGI environ key the Unix-socket listener sets to the verified peer credentials (see unix_listener.py). An HTTP
# client cannot set it: werkzeug puts every request header under an `HTTP_` key.
UNIX_PEER_ENVIRON_KEY = "hams_rpc_guard.unix_peer"
# Set by the gate on a Unix-socket request it admitted: the frozenset of Odoo logins the peer may act as (empty: any).
UNIX_LOGINS_ENVIRON_KEY = "hams_rpc_guard.unix_logins"

# Headers a reverse proxy, a tunnel or Cloudflare adds. A request straight from a daemon has none.
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

_counts = collections.Counter()
_counts_lock = threading.Lock()


class ConfigError(ValueError):
    """A gate setting that does not parse. Raised when it is written; logged and failed closed when it is read."""


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


# [@ANCHOR: hams_rpc_guard:parse_networks]
def parse_networks(value, param=ALLOWED_NETWORKS_PARAM, default=""):
    """The tuple of `ipaddress` networks in a comma-separated setting. Raises `ConfigError` naming `param` on any entry
    that is not an address or a network (a host address is a /32 or /128). Unset or blank takes `default`; the word
    `none` is the empty tuple."""
    text = str(value).strip() if value not in (None, False) else ""
    if not text:
        text = default
    if text.lower() == NO_NETWORK:
        return ()
    networks = []
    for item in text.split(","):
        item = item.strip()
        if not item:
            continue
        try:
            networks.append(ipaddress.ip_network(item, strict=False))
        except ValueError:
            raise ConfigError("%s: %r is not an IPv4 or IPv6 address or network (CIDR)." % (param, item)) from None
    return tuple(networks)


# [@ANCHOR: hams_rpc_guard:parse_trusted_proxies]
def parse_trusted_proxies(value):
    """`parse_networks` for `hams_rpc_guard.trusted_proxies` (default: none). Raises `ConfigError` on junk."""
    return parse_networks(value, TRUSTED_PROXIES_PARAM, "")


# [@ANCHOR: hams_rpc_guard:parse_client_address_header]
def parse_client_address_header(value):
    """The canonical spelling of the trusted client-address header, or "" when unset. Raises `ConfigError` for any
    other header (`X-Forwarded-For` included: it is a list whose first entries the client writes)."""
    text = str(value).strip() if value not in (None, False) else ""
    if not text:
        return ""
    for name in CLIENT_ADDRESS_HEADERS:
        if name.lower() == text.lower():
            return name
    raise ConfigError("%s: %r is not one of %s." % (CLIENT_ADDRESS_HEADER_PARAM, text, ", ".join(CLIENT_ADDRESS_HEADERS)))


# [@ANCHOR: hams_rpc_guard:parse_legacy_access]
def parse_legacy_access(value):
    """`networks` (unset, or the word) or `none`. Raises `ConfigError` on anything else."""
    text = str(value).strip().lower() if value not in (None, False) else ""
    if text in ("", LEGACY_NETWORKS):
        return LEGACY_NETWORKS
    if text == LEGACY_NONE:
        return LEGACY_NONE
    raise ConfigError("%s: %r is not 'networks' or 'none'." % (LEGACY_PARAM, value))


# [@ANCHOR: hams_rpc_guard:parse_transport]
def parse_transport(value):
    """`tcp` (unset), `unix` or `both`. Raises `ConfigError` on anything else."""
    text = str(value).strip().lower() if value not in (None, False) else ""
    if not text:
        return TRANSPORT_TCP
    if text in TRANSPORTS:
        return text
    raise ConfigError("%s: %r is not one of %s." % (TRANSPORT_PARAM, value, ", ".join(TRANSPORTS)))


# [@ANCHOR: hams_rpc_guard:peer]
class Peer:
    """The verified credentials of the process at the other end of a Unix-socket connection.

    `pid`, `uid`, `gid` (int, from `SO_PEERCRED`, set by the kernel at `connect()`), `user` (the uid's name, or "" when
    it has none), `group` (the gid's name, or "")."""

    __slots__ = ("pid", "uid", "gid", "user", "group")

    def __init__(self, pid, uid, gid, user="", group=""):
        self.pid, self.uid, self.gid, self.user, self.group = pid, uid, gid, user, group

    def as_dict(self):
        return {name: getattr(self, name) for name in self.__slots__}


# [@ANCHOR: hams_rpc_guard:peer_rule]
class PeerRule:
    """One entry of `hams_rpc_guard.unix_peers`: `kind` (`user`, `uid`, `group` or `gid`), `value` (name or int) and
    `logins` (frozenset of the Odoo logins the peer may authenticate as; empty means any)."""

    __slots__ = ("kind", "value", "logins")

    def __init__(self, kind, value, logins=frozenset()):
        self.kind, self.value, self.logins = kind, value, frozenset(logins)

    def matches(self, peer):
        return {"user": peer.user, "uid": peer.uid, "group": peer.group, "gid": peer.gid}[self.kind] == self.value


# [@ANCHOR: hams_rpc_guard:peer_rules]
class PeerRules:
    """The parsed `hams_rpc_guard.unix_peers`: an ordered tuple of `PeerRule`. `match(peer)` returns the first rule
    that matches, or None (the peer is refused)."""

    def __init__(self, rules=()):
        self.rules = tuple(rules)

    def match(self, peer):
        return next((rule for rule in self.rules if rule.matches(peer)), None)


_NAME = re.compile(r"^[A-Za-z_][A-Za-z0-9_.-]*\$?$")
_LOGIN = re.compile(r"^[^\s,|=]+$")


# [@ANCHOR: hams_rpc_guard:parse_peer_rules]
def parse_peer_rules(value):
    """`PeerRules` from the setting text. Entries are comma-separated `WHO[=LOGIN|LOGIN...]`, where WHO is a user name,
    a numeric uid, `@group` or `@<numeric gid>` (the peer's primary group: `SO_PEERCRED` carries no supplementary
    groups). Unset or blank: no rule, every peer refused. Raises `ConfigError` on a malformed entry."""
    text = str(value).strip() if value not in (None, False) else ""
    rules = []
    for item in text.split(","):
        item = item.strip()
        if not item:
            continue
        who, _sep, logins_text = item.partition("=")
        who = who.strip()
        logins = [login.strip() for login in logins_text.split("|")] if _sep else []
        if _sep and (not logins or not all(_LOGIN.match(login) for login in logins)):
            raise ConfigError("%s: %r has an empty or malformed login list." % (UNIX_PEERS_PARAM, item))
        is_group = who.startswith("@")
        name = who[1:] if is_group else who
        if name.isdigit():
            rules.append(PeerRule("gid" if is_group else "uid", int(name), logins))
        elif _NAME.match(name):
            rules.append(PeerRule("group" if is_group else "user", name, logins))
        else:
            raise ConfigError("%s: %r is not a user name, uid, @group or @gid." % (UNIX_PEERS_PARAM, item))
    return PeerRules(rules)


# [@ANCHOR: hams_rpc_guard:gate_settings]
class GateSettings:
    """The parsed gate settings of one database, as `read_settings` builds them.

    `allowed_networks` (tuple of networks), `trusted_proxies` (tuple of networks), `client_address_header` (str, ""
    for none), `legacy` (`networks`/`none`), `transport` (`tcp`/`unix`/`both`/`closed`), `unix_peers` (a
    `PeerRules`, or None when the setting could not be read: no Unix peer is admitted)."""

    def __init__(self, allowed_networks, trusted_proxies=(), client_address_header="", legacy=LEGACY_NETWORKS,
                 transport=TRANSPORT_TCP, unix_peers=None):
        self.allowed_networks = tuple(allowed_networks)
        self.trusted_proxies = tuple(trusted_proxies)
        self.client_address_header = client_address_header
        self.legacy = legacy
        self.transport = transport
        self.unix_peers = unix_peers


# [@ANCHOR: hams_rpc_guard:read_settings]
def read_settings(get_param):
    """A `GateSettings` from `get_param(key, default)` (a system-parameter reader). Every setting that does not parse
    is logged at ERROR (the parameter name and the reason; the value only as the operator wrote it) and replaced by its
    closed value: no network, no trusted proxy, legacy `none`, transport `closed`, no Unix peer. Never raises for a
    bad value; a reader that raises propagates (the caller fails closed)."""
    def parsed(parser, key, closed):
        try:
            return parser(get_param(key, None))
        except ConfigError as error:
            _logger.error("RPC guard configuration error, failing closed: %s", error)
            return closed

    return GateSettings(
        allowed_networks=parsed(lambda value: parse_networks(value, ALLOWED_NETWORKS_PARAM, DEFAULT_ALLOWED_NETWORKS),
                                ALLOWED_NETWORKS_PARAM, ()),
        trusted_proxies=parsed(parse_trusted_proxies, TRUSTED_PROXIES_PARAM, ()),
        client_address_header=parsed(parse_client_address_header, CLIENT_ADDRESS_HEADER_PARAM, ""),
        legacy=parsed(parse_legacy_access, LEGACY_PARAM, LEGACY_NONE),
        transport=parsed(parse_transport, TRANSPORT_PARAM, TRANSPORT_CLOSED),
        unix_peers=parsed(parse_peer_rules, UNIX_PEERS_PARAM, None),
    )


# [@ANCHOR: hams_rpc_guard:socket_peer]
def socket_peer(environ):
    """The socket peer address of the request (ProxyFix's saved original when `proxy_mode` rewrote it), or None when
    there is none or it is not an IP address."""
    original = environ.get("werkzeug.proxy_fix.orig") or {}
    peer = original.get("REMOTE_ADDR", environ.get("REMOTE_ADDR"))
    if peer in (None, ""):
        return None
    return _parse_address(peer)


# [@ANCHOR: hams_rpc_guard:resolve_client]
def resolve_client(environ, headers, trusted_proxies=(), client_address_header=""):
    """The true client address of a TCP request (an `ipaddress` object), or None when it cannot be known.

    `environ` is the WSGI environment, `headers` the request headers (case-insensitive), `trusted_proxies` a tuple of
    networks, `client_address_header` the canonical header name or "". Pure; the rules are in the module docstring."""
    peer = socket_peer(environ)
    if peer is None:
        return None
    if not any(headers.get(name) is not None for name in FORWARDING_HEADERS):
        return peer
    if not client_address_header or not _in_any(peer, trusted_proxies):
        return None
    values = headers.getlist(client_address_header)
    if len(values) != 1 or "," in values[0]:
        return None
    return _parse_address(values[0])


# [@ANCHOR: hams_rpc_guard:peer_is_trusted]
def peer_is_trusted(environ, trusted_proxies):
    """True when the socket peer of the request is a loopback address inside one of `trusted_proxies`: the one
    condition under which the verified client-certificate subject header (`hams_rpc_guard.client_cert_header`) may be
    believed. Stricter than `resolve_client`, which also trusts a non-loopback proxy for the client address."""
    peer = socket_peer(environ)
    return peer is not None and peer.is_loopback and _in_any(peer, trusted_proxies)


def _in_any(address, networks):
    return any(address in network for network in networks if network.version == address.version)


# [@ANCHOR: hams_rpc_guard:network_class]
def network_class(address):
    """The counter class of a client address: `loopback`, `link-local`, `private`, `public`, or `unknown` for None."""
    if address is None:
        return CLASS_UNKNOWN
    if address.is_loopback:
        return CLASS_LOOPBACK
    if address.is_link_local:
        return CLASS_LINK_LOCAL
    if address.is_private:
        return CLASS_PRIVATE
    return CLASS_PUBLIC


# [@ANCHOR: hams_rpc_guard:judge_tcp]
def judge_tcp(family, settings, client):
    """Whether a TCP request of endpoint `family` from `client` (an address or None) may proceed under `settings`.
    Returns `(allowed, reason)`; `reason` is one of `allowed`, `transport`, `legacy-closed`, `network`."""
    if settings.transport not in (TRANSPORT_TCP, TRANSPORT_BOTH):
        return False, "transport"
    if family == FAMILY_LEGACY and settings.legacy != LEGACY_NETWORKS:
        return False, "legacy-closed"
    if client is None or not _in_any(client, settings.allowed_networks):
        return False, "network"
    return True, "allowed"


# [@ANCHOR: hams_rpc_guard:judge_unix]
def judge_unix(family, settings, peer):
    """Whether a request that came in on the Unix socket with verified credentials `peer` (a `Peer`) may
    proceed. Returns `(allowed, reason, logins)`: `logins` is the frozenset of Odoo logins this peer may act as
    (empty = any), checked again once the caller has authenticated (`patch.check_unix_login`)."""
    if settings.transport not in (TRANSPORT_UNIX, TRANSPORT_BOTH):
        return False, "transport", frozenset()
    if family == FAMILY_LEGACY and settings.legacy != LEGACY_NETWORKS:
        return False, "legacy-closed", frozenset()
    if settings.unix_peers is None:
        return False, "peer", frozenset()
    rule = settings.unix_peers.match(peer)
    if rule is None:
        return False, "peer", frozenset()
    return True, "allowed", rule.logins


# [@ANCHOR: hams_rpc_guard:note_request]
def note_request(family, client_class, allowed, reason, served=None):
    """Count one judged request and, for a refusal, log at the first and every power of ten. Returns the count.

    `served` (default: `allowed`) is False when the request is refused; in report mode a would-be refusal is served
    and counted as `would-refuse:<reason>`. Logged: the family, the network class, the reason and the count. Never an
    address, header, login or body."""
    served = allowed if served is None else served
    outcome = "allowed" if allowed else ("refused:" if not served else "would-refuse:") + reason
    key = (family, client_class, outcome)
    with _counts_lock:
        _counts[key] += 1
        count = _counts[key]
    if not allowed and is_power_of_ten(count):
        _logger.warning(
            "%s %s request(s) to the %s endpoints from a %s client (reason: %s); the count is per worker since it "
            "started. See hams_rpc_guard.allowed_networks and the README of hams_rpc_guard.",
            "Served (report-only mode), would have refused," if served else "Refused", count, family, client_class,
            reason,
        )
    return count


# [@ANCHOR: hams_rpc_guard:request_counts]
def request_counts():
    """A copy of this worker's counters: `{(family, network class, "allowed" | "refused:<reason>" |
    "would-refuse:<reason>"): count}`."""
    with _counts_lock:
        return dict(_counts)


# [@ANCHOR: hams_rpc_guard:is_power_of_ten]
def is_power_of_ten(count):
    while count >= 10 and count % 10 == 0:
        count //= 10
    return count == 1
