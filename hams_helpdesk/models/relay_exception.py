# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The stateful parts of the relay exception and crash reports: the per-relay rate limit and the hand-off
to the shared exception pipeline.

The report is validated by relay_exception_schema.py. The caller that holds the authenticated link
(``ham_relay_bridge``'s ``exception_report`` relay call, in hams_com) passes the relay's id from the login and the
accounts it found for that relay's owner and the acting operator; nothing in the report itself names an account.
A report that passes is recorded with ``report_event``, the same fingerprint row, once-a-minute gate,
own-cursor writer and new-ticket cap as every other source, with source "relay".
"""

import hashlib
import logging

import redis

from odoo import api, models

from odoo.addons.distributed_redis_cache.redis_pool import get_redis_connection

from . import relay_exception_schema as schema
from .exception_fingerprint import report_event

_logger = logging.getLogger("hams_helpdesk.exception_reporter")

# Per relay: 6 reports a minute and 30 an hour. A relay coalesces repeats itself and sends a crash once per
# start, so a healthy relay stays far below this; a relay in a panic loop is held to it.
RATE_LIMITS = ((60, 6), (3600, 30))

# The answers of ``_submit``.
RECORDED = "recorded"
REJECTED = "rejected"
LIMITED = "limited"


class RelayException(models.AbstractModel):
    _name = "hams_helpdesk.relay.exception"
    _description = "Relay exception and crash reports"

    # [@ANCHOR: hams_helpdesk:relay_client_key]
    @api.model
    def _client_key(self, relay_id):
        """The Redis key part for one relay: a hash of the database name and the relay id. The counters expire on
        their own."""
        digest = hashlib.sha256(f"{self.env.cr.dbname}|{relay_id}".encode()).hexdigest()[:24]
        return f"hams_helpdesk:relay_exception:{digest}"

    # [@ANCHOR: hams_helpdesk:relay_rate_limited]
    @api.model
    def _rate_limited(self, relay_id):
        """True when this relay is over a limit, or when the limiter cannot count (Redis is down): a report that
        cannot be metered is dropped, not accepted without limit. Each call counts one use."""
        base = self._client_key(relay_id)
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
            _logger.info("Relay exception limiter unavailable, dropping the report: %r", error)
            return True

    # [@ANCHOR: hams_helpdesk:relay_record]
    @api.model
    def _record(self, report, owner_uid=None, operator_uid=None):
        """Hands a validated ``schema.Report`` to the shared pipeline. Returns True when it wrote (False when the
        once-a-minute gate or a cap held it back). ``owner_uid`` and ``operator_uid`` are res.users ids or None; both
        are LINKED to the ticket when they are real members' accounts (the pipeline checks), so the site team
        can write to them. With neither, the report is anonymous."""
        owner = owner_uid if isinstance(owner_uid, int) and not isinstance(owner_uid, bool) else None
        operator = operator_uid if isinstance(operator_uid, int) and not isinstance(operator_uid, bool) else None
        primary = owner or operator
        extra = (operator,) if owner and operator and operator != owner else ()
        return report_event(
            self.env.registry,
            schema.SOURCE,
            report.type_name,
            report.module,
            report.frame,
            report.route,
            report.trace,
            report.name,
            uid=primary,
            reporter="relay exception report",
            extra_uids=extra,
            weight=report.count,
        )

    # [@ANCHOR: hams_helpdesk:relay_submit]
    @api.model
    def _submit(self, params, relay_id, owner_uid=None, operator_uid=None):
        """The whole path for one report: rate limit, validation, rebuild, record. ``params`` is the report as a dict
        (the relay-call parameters). Returns ``RECORDED`` (written, or held back by the gate or a cap: a sender
        cannot tell which), ``LIMITED`` (over the per-relay limit) or ``REJECTED`` (not a valid report). Never
        raises: a failure to record is logged and answered ``REJECTED``."""
        try:
            if self._rate_limited(str(relay_id)):
                return LIMITED
            report = schema.build_report(params, relay_id=relay_id)
        except schema.ReportRejected as rejected:
            _logger.debug("Relay exception report refused: %s", rejected.reason)
            return REJECTED
        except Exception as error:  # audit-ignore-catch-all: Tested by [@ANCHOR: hams_helpdesk:relay_submit]
            _logger.error("Could not check a relay exception report: %r", error, exc_info=True)
            return REJECTED
        try:
            self._record(report, owner_uid, operator_uid)
        except Exception as error:  # audit-ignore-catch-all: Tested by [@ANCHOR: hams_helpdesk:relay_submit]
            # The reporter must never become an error report of its own.
            _logger.error("Could not record a relay exception report: %r", error, exc_info=True)
            return REJECTED
        return RECORDED
