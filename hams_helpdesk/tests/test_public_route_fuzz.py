# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Every public page route, called with the garbage scanners send, never answers 500.

Bruce, 2026-10-19: "Let's make external APIs like /blog robust against bad data, because things will be probing our
site continually." A 500 is a server-exception ticket, so a public route that raises on bad input is a ticket per
probe. The routes are not listed here: they are read from the routing map of whatever modules are installed, so a
route added tomorrow is covered tomorrow. Run with the full module set installed for the whole site; with only
hams_helpdesk's dependencies it covers those.
"""

import inspect
import logging
import re
from urllib.parse import quote

import requests

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase

_logger = logging.getLogger(__name__)

# Routes that are not fuzzed, each with the reason. Prefix match on the route's own pattern.
SKIPPED_ROUTE_PREFIXES = {
    "/websocket": "a WebSocket upgrade, not a page",
    "/longpolling": "a held-open poll that never answers until an event arrives",
    "/bus/": "bus long poll, held open",
    "/web/database/": "database manager: drop, backup and restore; never called against a live database",
    "/web/session/logout": "ends the session the test runs on",
    "/web/session/destroy": "ends the session the test runs on",
    "/web/become": "switches the session user",
    "/report/": "renders PDFs by starting wkhtmltopdf",
    "/odoo/upgrade_modules": "module installer",
    "/web/binary/upload": "stores a file",
}

# What a scanner puts in the query string. A request carries several of these at once because the parameter names a
# route reads are not known here; each vector holds the names routes commonly read.
QUERY_VECTORS = (
    # text where a date belongs (the /blog?date_begin=ijyM probe that filed tickets 72 and 73)
    "date_begin=ijyM&date_end=pNOE&date=2026-13-45&start=0000-00-00&end=2026-02-30&from=%E2%80%A6&to=1",
    # 25-digit and negative numbers, numbers where text belongs, text where numbers belong
    "page=99999999999999999999&limit=-1&offset=-99999999999999999999&id=99999999999999999999&ids=1,abc&per_page=abc"
    "&order=name%3Bdrop%20table&sortby=%00&ppg=1e999&category=-1&tag=abc&tags=,,&partner_id=999999999999999999999",
    # NUL bytes
    "search=a%00b&q=%00&term=%00&name=%00&token=%00&type=%00&sort=%00&access_token=%00&callsign=%00&db=%00",
    # unicode, bidi override, emoji, lone surrogate-looking text, SQL fragments
    "search=%27%20OR%201%3D1--&q=%F0%9F%98%80%CE%A9%E2%80%AE&name=%22%3B%20DROP%20TABLE%20res_users%3B--"
    "&callsign=%27)%20UNION%20SELECT%201&slug=%ED%A0%80&email=%F0%9F%98%80@%E2%80%AE",
    # lists, nested keys, repeated keys, a domain
    "x[]=1&x[]=2&search=a&search=b&domain=[('id','=',1)]&filter[a][b]=1&tag=1&tag=abc&a.b=1&=1&%5B%5D=1",
    # redirects, languages, odd identifiers
    "redirect=//evil.com/..%2f..&lang=zz_ZZ&website_id=abc&state=a&code=b&next=http://x/../&url=%2e%2e%2f"
    "&model=res.users&res_model=%00&res_id=-1&field=id&mode=%00",
)

# Replacements for the <converter:name> parts of a route pattern, one request per token.
PATH_TOKENS = ("garbage", "99999999999999999999", "-1")

_PLACEHOLDER = re.compile(r"<(?:[^:>]+(?:\([^)]*\))?:)?[^>]+>")


def _concrete_path(pattern, token):
    return _PLACEHOLDER.sub(quote(token, safe=""), pattern)


@tagged("post_install", "-at_install")
class TestPublicRouteFuzz(HamsHttpCase):
    def _public_get_routes(self):
        """(pattern, handler-parameter-names) for every public page route that answers GET."""
        found = {}
        for rule in self.env["ir.http"].routing_map().iter_rules():
            routing = rule.endpoint.routing
            if routing.get("type") != "http" or routing.get("auth") not in ("public", "none"):
                continue
            if rule.methods is not None and "GET" not in rule.methods:
                continue
            if any(rule.rule.startswith(prefix) for prefix in SKIPPED_ROUTE_PREFIXES):
                continue
            names = ()
            handler = getattr(rule.endpoint, "original_endpoint", None)
            if handler is not None:
                try:
                    names = tuple(list(inspect.signature(handler).parameters)[1:])
                except (TypeError, ValueError):
                    names = ()
            found[rule.rule] = tuple(n for n in names if n not in ("kw", "kwargs", "post", "args"))
        return sorted(found.items())

    def _requests_for(self, pattern, names):
        for index, vector in enumerate(QUERY_VECTORS):
            yield _concrete_path(pattern, PATH_TOKENS[index % len(PATH_TOKENS)]) + "?" + vector
        # Every parameter the handler declares, set to text and then to a number no column holds.
        if names:
            for value in ("ijyM", "99999999999999999999", "%00"):
                query = "&".join(f"{name}={value}" for name in names)
                yield _concrete_path(pattern, PATH_TOKENS[0]) + "?" + query
        # Dot segments and an absolute URL where a path argument belongs.
        yield _concrete_path(pattern, "..%2f..%2fetc%2fpasswd")
        yield _concrete_path(pattern, "http://evil.example/..")

    def test_no_public_get_route_answers_500_to_bad_input(self):
        # Tests [@ANCHOR: hams_helpdesk:malformed_input_is_404]
        routes = self._public_get_routes()
        self.assertGreater(len(routes), 20, "the routing map holds almost no public routes")
        failures = {}
        sent = 0
        for pattern, names in routes:
            for url in self._requests_for(pattern, names):
                sent += 1
                try:
                    response = self.url_open(url, allow_redirects=False, timeout=15)
                except requests.RequestException as error:
                    failures.setdefault(pattern, []).append((url[:120], type(error).__name__))
                    continue
                if response.status_code >= 500:
                    failures.setdefault(pattern, []).append((url[:120], response.status_code))
        self._log_summary(len(routes), sent, failures)
        self.assertFalse(
            failures,
            "public routes that answered 500 or did not answer:\n"
            + "\n".join(f"{pattern}  e.g. {attempts[0]}  ({len(attempts)} requests)" for pattern, attempts in failures.items()),
        )

    def _log_summary(self, route_count, request_count, failures):
        _logger.info(
            "public route fuzz: %d routes, %d requests, %d routes answered 500: %s",
            route_count,
            request_count,
            len(failures),
            sorted(failures),
        )
