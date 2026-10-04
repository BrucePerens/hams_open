# SPDX-License-Identifier: AGPL-3.0-or-later
"""Pure helpers for tenant_sites: host normalization and the public-path allow-list.

Nothing here touches Odoo, so the same code is unit-tested without a database.
"""

import ipaddress
import re

LABEL_RE = re.compile(r"^(?!-)[a-z0-9-]{1,63}(?<!-)$")
SAFE_METHODS = ("GET", "HEAD", "OPTIONS")
CLOUDFLARE_HEADERS = ("CF-Ray", "CF-Connecting-IP")

# Paths a public tenant website may answer through a route. Every other path is not a route on a
# tenant host: it falls through to the website's own page and redirect lookup, then to a 404.
TENANT_PATH_ALLOW = re.compile(
    r"^(?:/$"
    r"|/blog(?:/|$)"
    r"|/web/(?:assets|static)/"
    r"|/web/(?:image|content)(?:/|$)"
    r"|/website/(?:static|image)(?:/|$)"
    r"|/[a-z0-9_]+/static/"
    r"|/robots\.txt$"
    r"|/sitemap[^/]*\.xml$"
    r"|/favicon\.ico$)"
)
# Controllers whose routes may answer on a tenant host (Python module prefixes of the endpoint).
TENANT_MODULE_ALLOW = (
    "odoo.addons.website.",
    "odoo.addons.website_blog.",
    "odoo.addons.web.",
    "odoo.addons.http_routing.",
    "odoo.addons.edge_cache.",
)
# These two paths exist in both web and website; only the website controller may answer them.
WEBSITE_ONLY_PATHS = ("/", "/robots.txt")


# [@ANCHOR: tenant_sites:COMM_normalize_host]
# Verified by [@ANCHOR: tenant_sites:COMM_test_normalize_host]
def normalize_host(raw):
    """Lowercase ASCII (punycode) hostname without port or trailing dot, or "" if it is not a
    plausible DNS name. IP literals and anything with odd characters are refused."""
    if not raw or not isinstance(raw, str):
        return ""
    text = raw.strip().lower()
    if text.startswith("["):
        return ""
    if ":" in text:
        text, _sep, port = text.partition(":")
        if not port.isdigit():
            return ""
    text = text.rstrip(".")
    if not text or len(text) > 253:
        return ""
    try:
        text = text.encode("idna").decode("ascii")
    except UnicodeError:
        return ""
    labels = text.split(".")
    if len(labels) < 2 or all(label.isdigit() for label in labels):
        return ""
    if not all(LABEL_RE.match(label) for label in labels):
        return ""
    return text


# [@ANCHOR: tenant_sites:COMM_original_host]
# Verified by [@ANCHOR: tenant_sites:COMM_test_original_host]
def original_host(environ):
    """The Host header as the client sent it.

    Odoo's proxy_mode runs werkzeug's ProxyFix with x_host=1, which REPLACES HTTP_HOST by the
    client-supplied X-Forwarded-Host header. ProxyFix keeps the original in
    environ["werkzeug.proxy_fix.orig"]; this reads that, never the rewritten value."""
    orig = environ.get("werkzeug.proxy_fix.orig") or {}
    return orig.get("HTTP_HOST") or environ.get("HTTP_HOST") or ""


# [@ANCHOR: tenant_sites:COMM_original_peer]
# Verified by [@ANCHOR: tenant_sites:COMM_test_original_peer]
def original_peer(environ):
    """The address of the socket peer, before ProxyFix replaces it with X-Forwarded-For."""
    orig = environ.get("werkzeug.proxy_fix.orig") or {}
    return orig.get("REMOTE_ADDR") or environ.get("REMOTE_ADDR") or ""


# [@ANCHOR: tenant_sites:COMM_is_loopback_address]
# Verified by [@ANCHOR: tenant_sites:COMM_test_is_loopback_address]
def is_loopback_address(address):
    try:
        return ipaddress.ip_address((address or "").strip()).is_loopback
    except ValueError:
        return False


# [@ANCHOR: tenant_sites:COMM_through_cloudflare]
# Verified by [@ANCHOR: tenant_sites:COMM_test_through_cloudflare]
def through_cloudflare(headers):
    """True when the request carries Cloudflare's own headers. The edge sets CF-Ray (and
    CF-Connecting-IP) on every proxied request itself; a request from this machine's own daemons,
    the test harness or an operator's SSH tunnel carries neither. Used only to refuse a request
    through the tunnel whose Host is not a domain name; classification never depends on it."""
    return any(headers.get(name) for name in CLOUDFLARE_HEADERS)


# [@ANCHOR: tenant_sites:COMM_host_matches]
# Verified by [@ANCHOR: tenant_sites:COMM_test_host_matches]
def host_matches(host, patterns):
    """True when `host` equals a pattern or fits a leading wildcard (`*.example.com` matches
    `a.example.com` and `a.b.example.com`, not `example.com`)."""
    for pattern in patterns:
        if pattern.startswith("*."):
            if host.endswith(pattern[1:]) and len(host) > len(pattern) - 1:
                return True
        elif host == pattern:
            return True
    return False


# [@ANCHOR: tenant_sites:COMM_valid_host_pattern]
# Verified by [@ANCHOR: tenant_sites:COMM_test_valid_host_pattern]
def normalize_host_pattern(raw):
    """A host, or `*.` followed by a host, normalized; "" when it is neither."""
    text = (raw or "").strip().lower()
    if text.startswith("*."):
        base = normalize_host(text[2:])
        return "*." + base if base else ""
    return normalize_host(text)


# [@ANCHOR: tenant_sites:COMM_tenant_path_allowed]
# Verified by [@ANCHOR: tenant_sites:COMM_test_tenant_path_allowed]
def tenant_path_allowed(path):
    return bool(TENANT_PATH_ALLOW.match(path or ""))


# [@ANCHOR: tenant_sites:COMM_tenant_module_allowed]
# Verified by [@ANCHOR: tenant_sites:COMM_test_tenant_module_allowed]
def tenant_module_allowed(path, module_name):
    """Whether the controller module that matched `path` may answer on a tenant host."""
    name = module_name or ""
    if path in WEBSITE_ONLY_PATHS:
        return name.startswith("odoo.addons.website.")
    return name.startswith(TENANT_MODULE_ALLOW)
