# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The stateful parts of the browser JavaScript error reports: the per-visitor rate limit, the check
that a report names a current asset bundle, and the hand-off to the shared exception pipeline.

The request handling is in controllers/browser_exception_api.py and the validation of the report in
browser_exception_schema.py. A report that passes both is recorded with ``report_event``, the same
fingerprint row, once-a-minute gate, own-cursor writer and new-ticket cap as every other source,
with source "browser".
"""

import hashlib
import logging
import threading
import time

import redis

from odoo import api, models

from odoo.addons.distributed_redis_cache.redis_pool import get_redis_connection

from . import browser_exception_schema as schema
from .exception_fingerprint import report_event

_logger = logging.getLogger("hams_helpdesk.exception_reporter")

# Per visitor (the address Cloudflare reports): 10 reports a minute and 60 an hour. A page sends at most
# five per load, and only the first of a repeated error, so a real browser stays far below this.
RATE_LIMITS = ((60, 10), (3600, 60))
# The current version of each asset bundle is a hash of its files, found by walking them; it only
# changes when a module is installed or upgraded, so a worker keeps it for a minute.
VERSION_CACHE_SECONDS = 60

_VERSIONS_LOCK = threading.Lock()
# database name -> (monotonic time it was read, {bundle name: current js version})
_VERSIONS = {}


class BrowserException(models.AbstractModel):
    _name = "hams_helpdesk.browser.exception"
    _description = "Browser JavaScript error reports"

    # [@ANCHOR: hams_helpdesk:browser_client_key]
    @api.model
    def _client_key(self, client_address):
        """The Redis key part for one visitor: a hash of the database name and the address, never the
        address itself. The counters expire on their own."""
        digest = hashlib.sha256(f"{self.env.cr.dbname}|{client_address}".encode()).hexdigest()[:24]
        return f"hams_helpdesk:browser_exception:{digest}"

    # [@ANCHOR: hams_helpdesk:browser_rate_limited]
    @api.model
    def _rate_limited(self, client_address):
        """True when this visitor is over a limit, or when the limiter cannot count (Redis is down): a
        report that cannot be metered is dropped, not accepted without limit. Each call counts one use."""
        base = self._client_key(client_address)
        try:
            client = get_redis_connection(self.env)
            over = False
            for window, allowed in RATE_LIMITS:
                key = f"{base}:{window}"
                used = client.incr(key)
                if used == 1 or client.ttl(key) < 0:
                    client.expire(key, window)
                over = over or used > allowed
            return over
        except redis.RedisError as error:
            _logger.info("Browser exception limiter unavailable, dropping the report: %r", error)
            return True

    # [@ANCHOR: hams_helpdesk:browser_current_versions]
    @api.model
    def _current_versions(self):
        """{bundle name: current JavaScript version} for the bundles a page loads. This is the
        build-scoped value that stands in for a per-response nonce: the pages are cached, so a nonce
        would be shared by every visitor and prove nothing, while the version changes with each release."""
        dbname = self.env.cr.dbname
        now = time.monotonic()
        with _VERSIONS_LOCK:
            cached = _VERSIONS.get(dbname)
            if cached and now - cached[0] < VERSION_CACHE_SECONDS:
                return cached[1]
        qweb = self.env["ir.qweb"]
        versions = {}
        for name in schema.KNOWN_BUNDLES:
            bundle = qweb._get_asset_bundle(name, css=False, js=True)
            versions[name] = bundle.get_version("js")
        with _VERSIONS_LOCK:
            _VERSIONS[dbname] = (now, versions)
        return versions

    # [@ANCHOR: hams_helpdesk:browser_release_is_current]
    @api.model
    def _release_is_current(self, bundle, version):
        """True when ``version`` is the current version of the known bundle ``bundle``. A report from a
        page loaded before the last release is stale and is refused, as is an unknown bundle."""
        current = self._current_versions().get(bundle)
        return bool(current) and version == current

    # [@ANCHOR: hams_helpdesk:browser_record]
    @api.model
    def _record(self, report):
        """Hands a validated ``schema.Report`` to the shared pipeline. Returns True when it wrote (False
        when the once-a-minute gate or a cap held it back). No user id is passed: a browser report is
        anonymous by design."""
        return report_event(
            self.env.registry,
            "browser",
            report.type_name,
            report.module,
            report.frame,
            report.route,
            report.trace,
            report.name,
            uid=None,
            reporter="browser JavaScript error report",
        )
