# SPDX-License-Identifier: AGPL-3.0-or-later
"""Pure helpers for the parking module: host validation, redirect building, form tokens, pages.

Nothing here touches Odoo, so the same code is unit-tested without a database.
"""

import hashlib
import hmac
import html
import ipaddress
import re
import time
from urllib.parse import quote, urlsplit

LABEL_RE = re.compile(r"^(?!-)[a-z0-9-]{1,63}(?<!-)$")
INQUIRY_PATH = "/__parking/inquiry"
TOKEN_MIN_AGE = 3
TOKEN_MAX_AGE = 86400
PATH_SAFE = "/:@!$&'()*+,;=-._~"
QUERY_SAFE = "=&%+/:@!$'()*,;-._~?"


# [@ANCHOR: parking:COMM_normalize_host]
# Verified by [@ANCHOR: parking:COMM_test_normalize_host]
def normalize_host(raw):
    """Lowercase ASCII (punycode) hostname without port or trailing dot, or "" if it is not a
    plausible DNS name. IP literals and anything with odd characters are refused."""
    if not raw or not isinstance(raw, str):
        return ""
    text = raw.strip().lower()
    if text.startswith("["):  # IPv6 literal: never a parked domain
        return ""
    if ":" in text:
        text, _, port = text.partition(":")
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


# [@ANCHOR: parking:COMM_original_host]
# Verified by [@ANCHOR: parking:COMM_test_original_host]
def original_host(environ):
    """The Host header as the client sent it.

    Odoo's proxy_mode runs werkzeug's ProxyFix with x_host=1, which REPLACES HTTP_HOST by the
    client-supplied X-Forwarded-Host header. A visitor of parked-a.example could send
    `X-Forwarded-Host: parked-b.example` and make Odoo answer as parked-b.example while Cloudflare
    caches the answer under parked-a.example. ProxyFix keeps the original in
    environ["werkzeug.proxy_fix.orig"]; this reads that, never the rewritten value."""
    orig = environ.get("werkzeug.proxy_fix.orig") or {}
    return orig.get("HTTP_HOST") or environ.get("HTTP_HOST") or ""


# [@ANCHOR: parking:COMM_validate_redirect_url]
# Verified by [@ANCHOR: parking:COMM_test_validate_redirect_url]
def validate_redirect_url(url, own_host=""):
    """Returns "" if `url` is an acceptable fixed redirect target, else the reason it is not."""
    if not url or len(url) > 2000:
        return "empty or too long"
    if any(ord(ch) < 33 or ord(ch) == 127 for ch in url):
        return "contains spaces or control characters"
    parts = urlsplit(url)
    if parts.scheme not in ("http", "https"):
        return "scheme must be http or https"
    if not parts.netloc or "@" in parts.netloc or "\\" in url:
        return "needs a plain host (no credentials, no backslash)"
    host = normalize_host(parts.hostname or "")
    if not host:
        return "host is not a valid domain name"
    if own_host and host == own_host:
        return "redirects to itself"
    try:
        parts.port
    except ValueError:
        return "invalid port"
    return ""


# [@ANCHOR: parking:COMM_redirect_location]
# Verified by [@ANCHOR: parking:COMM_test_redirect_location]
def redirect_location(target, path="/", query="", preserve_path=False):
    """The Location header for a redirect. The scheme and host come only from the stored target,
    never from the request, so a crafted path cannot move the redirect to another host. With
    preserve_path the request path and query are appended after the target's own path."""
    if not preserve_path:
        return target
    parts = urlsplit(target)
    base_path = parts.path.rstrip("/")
    request_path = "/" + (path or "/").lstrip("/")
    request_path = request_path.replace("\\", "/")
    location = f"{parts.scheme}://{parts.netloc}{base_path}{quote(request_path, safe=PATH_SAFE)}"
    if parts.query:
        location += "?" + parts.query
    if query:
        location += ("&" if parts.query else "?") + quote(query, safe=QUERY_SAFE)
    return location


# [@ANCHOR: parking:COMM_form_token]
# Verified by [@ANCHOR: parking:COMM_test_form_token]
def form_token(secret, host, now=None):
    """Stateless anti-CSRF and anti-bot token for the for-sale form: a timestamp and an HMAC over
    host and timestamp. The page may sit in Cloudflare's cache for up to a day and still post."""
    stamp = int(now if now is not None else time.time())
    mac = hmac.new(secret.encode(), f"{host}|{stamp}".encode(), hashlib.sha256).hexdigest()[:32]
    return f"{stamp}.{mac}"


# [@ANCHOR: parking:COMM_verify_form_token]
# Verified by [@ANCHOR: parking:COMM_test_verify_form_token]
def verify_form_token(secret, host, token, now=None):
    """True when the token was made for this host, is at least TOKEN_MIN_AGE seconds old (a bot
    that posts instantly fails) and at most TOKEN_MAX_AGE seconds old."""
    try:
        stamp_text, _, mac = (token or "").partition(".")
        stamp = int(stamp_text)
    except ValueError:
        return False
    current = int(now if now is not None else time.time())
    age = current - stamp
    if age < TOKEN_MIN_AGE or age > TOKEN_MAX_AGE:
        return False
    expected = hmac.new(secret.encode(), f"{host}|{stamp}".encode(), hashlib.sha256).hexdigest()[:32]
    return hmac.compare_digest(expected, mac)


# [@ANCHOR: parking:COMM_original_peer]
# Verified by [@ANCHOR: parking:COMM_test_original_peer]
def original_peer(environ):
    """The address of the socket peer, before ProxyFix replaces it with X-Forwarded-For."""
    orig = environ.get("werkzeug.proxy_fix.orig") or {}
    return orig.get("REMOTE_ADDR") or environ.get("REMOTE_ADDR") or ""


# [@ANCHOR: parking:COMM_is_loopback_address]
# Verified by [@ANCHOR: parking:COMM_test_is_loopback_address]
def is_loopback_address(address):
    try:
        return ipaddress.ip_address((address or "").strip()).is_loopback
    except ValueError:
        return False


# [@ANCHOR: parking:COMM_client_ip]
# Verified by [@ANCHOR: parking:COMM_test_client_ip]
def client_ip(environ, headers):
    """The visitor's address. CF-Connecting-IP is trusted only when CF-Ray is also present: every
    request that came through Cloudflare carries both, and the tenant's only listener is reachable
    from cloudflared and from loopback."""
    if headers.get("CF-Ray") and headers.get("CF-Connecting-IP"):
        return headers["CF-Connecting-IP"].strip()[:64]
    return (environ.get("REMOTE_ADDR") or "")[:64]


# [@ANCHOR: parking:COMM_hash_ip]
# Verified by [@ANCHOR: parking:COMM_test_hash_ip]
def hash_ip(secret, ip):
    return hmac.new(secret.encode(), ip.encode(), hashlib.sha256).hexdigest()[:32]


STYLE = (
    "body{font:16px/1.5 system-ui,sans-serif;max-width:34rem;margin:12vh auto;padding:0 1rem;"
    "color:#222}h1{font-size:1.4rem;word-break:break-all}input,textarea{width:100%;box-sizing:border-box;"
    "padding:.4rem;margin:.2rem 0 .8rem}button{padding:.5rem 1.2rem}.hp{display:none}"
    "@media(prefers-color-scheme:dark){body{background:#111;color:#ddd}}"
)


# [@ANCHOR: parking:COMM_page]
# Verified by [@ANCHOR: parking:COMM_test_page]
def _page(title, body, noindex, lang="en"):
    robots = '<meta name="robots" content="noindex,nofollow">' if noindex else ""
    return (
        f'<!doctype html><html lang="{html.escape(lang, quote=True)}"><head><meta charset="utf-8">'
        '<meta name="viewport" content="width=device-width,initial-scale=1">'
        f"{robots}<title>{html.escape(title)}</title><style>{STYLE}</style></head>"
        f"<body>{body}</body></html>"
    )


# [@ANCHOR: parking:COMM_render_pages]
# Verified by [@ANCHOR: parking:COMM_test_render_pages]
def render_parked(host, title="", message="", noindex=True):
    heading = html.escape(title or host)
    text = html.escape(message or "This domain is registered and not currently in use.")
    paragraphs = "".join(f"<p>{line}</p>" for line in text.splitlines() if line.strip())
    return _page(title or host, f"<h1>{heading}</h1>{paragraphs}", noindex)


# [@ANCHOR: parking:COMM_render_for_sale]
# Verified by [@ANCHOR: parking:COMM_test_render_for_sale]
def render_for_sale(host, token, title="", message="", price_text="", sent=False, noindex=True):
    heading = html.escape(title or f"{host} is for sale")
    text = html.escape(message or "Interested in this domain? Send an inquiry below.")
    price = f"<p><strong>{html.escape(price_text)}</strong></p>" if price_text else ""
    if sent:
        form = "<p>Thank you. Your inquiry was sent.</p>"
    else:
        form = (
            f'<form method="post" action="{INQUIRY_PATH}">'
            f'<input type="hidden" name="token" value="{html.escape(token, quote=True)}">'
            '<label>Your name<input name="name" maxlength="120" autocomplete="name"></label>'
            '<label>Email<input name="email" type="email" maxlength="200" required autocomplete="email"></label>'
            '<label>Offer or message<textarea name="message" rows="4" maxlength="4000" required></textarea></label>'
            '<div class="hp" aria-hidden="true"><label>Leave empty<input name="website" tabindex="-1" autocomplete="off"></label></div>'
            "<button>Send</button></form>"
        )
    return _page(title or host, f"<h1>{heading}</h1><p>{text}</p>{price}{form}", noindex)


# [@ANCHOR: parking:COMM_render_gone]
# Verified by [@ANCHOR: parking:COMM_test_render_gone]
def render_gone(host):
    return _page(host, "<h1>410 Gone</h1><p>This address no longer exists.</p>", True)


# [@ANCHOR: parking:COMM_robots_txt]
# Verified by [@ANCHOR: parking:COMM_test_robots_txt]
def robots_txt(noindex):
    return "User-agent: *\nDisallow: /\n" if noindex else "User-agent: *\nAllow: /\n"
