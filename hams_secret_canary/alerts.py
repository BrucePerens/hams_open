# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Alert backends. A backend is a function `(env, alarm)`; every registered backend runs for each alarm. The alarm names
the label and where it was seen and never carries the matched value."""
import logging
from dataclasses import dataclass

from odoo import fields

_logger = logging.getLogger("hams_secret_canary.alarm")
BACKENDS = {}


# [@ANCHOR: hams_secret_canary_alerts_alarm]
@dataclass(frozen=True)
class Alarm:
    label: str
    kind: str
    source: str  # "response" or "log"
    where: str  # request path or logger name, already redacted


# [@ANCHOR: hams_secret_canary:register_backend]
def register_backend(name):
    """Decorator: registers `fn(env, alarm)` as the alert backend `name` (replacing one of that name)."""
    def decorator(fn):
        BACKENDS[name] = fn
        return fn
    return decorator


# [@ANCHOR: hams_secret_canary:raise_alarm]
def raise_alarm(env, alarm):
    """Runs every registered backend for `alarm`, in registration order; an exception propagates (fail fast)."""
    for backend in list(BACKENDS.values()):
        backend(env, alarm)


@register_backend("logging")
# [@ANCHOR: hams_secret_canary:logging_backend]
def logging_backend(env, alarm):
    """Writes one CRITICAL line naming the label, kind, source and place. `env` may be None (the log watcher has none)."""
    _logger.critical("SECRET CANARY TRIPPED: %s %s seen in %s (%s)", alarm.kind, alarm.label, alarm.source, alarm.where)


@register_backend("record")
# [@ANCHOR: hams_secret_canary:record_backend]
def record_backend(env, alarm):
    """Writes a `zero_sudo.security.log` row (reason `secret_canary`; login = label, user agent = place). Skipped when
    `env` is None."""
    if env is None:
        return
    facility = env["zero_sudo.security.utils"]._get_service_env("zero_sudo.odoo_facility_service_internal")
    facility["zero_sudo.security.log"].create({
        "reason": "secret_canary",
        "login": alarm.label,
        "user_agent": "%s: %s" % (alarm.source, alarm.where),
    })


@register_backend("mail")
# [@ANCHOR: hams_secret_canary:mail_backend]
def mail_backend(env, alarm):
    """Queues one email to the administrator's address (`base.user_admin`), sent by Odoo's normal mail queue. Skipped
    when `env` is None; logs a warning when the administrator has no address."""
    if env is None:
        return
    mail_env = env["zero_sudo.security.utils"]._get_service_env("zero_sudo.mail_service_internal")
    address = mail_env.ref("base.user_admin").partner_id.email
    if not address:
        _logger.warning("Secret canary alarm for %s: the administrator has no email address, no mail queued.", alarm.label)
        return
    mail_env["mail.mail"].create({
        "subject": "Secret canary tripped: %s" % alarm.label,
        "body_html": "<p>A %s (%s) was seen in %s: %s.</p><p>This detects a leak after it happened.</p>"
                     % (alarm.kind, alarm.label, alarm.source, alarm.where),
        "email_to": address,
        "auto_delete": False,
    })


@register_backend("pager")
# [@ANCHOR: hams_secret_canary:pager_backend]
def pager_backend(env, alarm):
    """Opens (or adds an occurrence to) a critical `pager.incident` whose source is `Secret canary: <label>`, when the
    `pager_duty` addon is installed. The description is metadata only: kind, label, place and UTC time, never the value.
    `report_incident` does not read the `pagerduty-maintenance` flag, so a hit during maintenance still pages. Skipped
    when `env` is None (log hits) or `pager_duty` is not installed. A failure is logged and never reaches the response
    (the other backends have already run, and this one runs last)."""
    if env is None or "pager.incident" not in env:
        return
    try:
        with env.cr.savepoint():
            pager_uid = env["zero_sudo.security.utils"]._get_service_uid("pager_duty.user_pager_service_internal")
            env["pager.incident"].with_user(pager_uid).report_incident({
                "source": "Secret canary: %s" % alarm.label,
                "severity": "critical",
                "description": "A %s (%s) was seen in %s: %s, %s UTC." % (
                    alarm.kind, alarm.label, alarm.source, alarm.where, fields.Datetime.now()),
            })
    except Exception as error:  # audit-ignore-catch-all: alerting must not break the response it is watching
        _logger.warning("Secret canary alarm for %s: the pager incident was not opened (%s).", alarm.label, type(error).__name__)
