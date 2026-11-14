# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

import logging
import os

from odoo import api
from odoo.modules import get_module_path
from odoo.tests.common import tagged

from . import common

_logger = logging.getLogger(__name__)
_FRAGMENTS = ("AccessError", "not allowed to access", "You are not allowed to")


class _Collector(logging.Handler):
    """Keeps every WARNING-or-worse record whose text or traceback mentions an access refusal."""

    def __init__(self):
        super().__init__(logging.WARNING)
        self.found = []

    def emit(self, record):
        text = record.getMessage()
        if record.exc_info:
            text += " " + repr(record.exc_info[1]) + " " + type(record.exc_info[1]).__name__
        if any(fragment in text for fragment in _FRAGMENTS):
            self.found.append(f"{record.name}: {text[:300]}")


@tagged("post_install", "-at_install")
class TestCronRunsClean(common.HamsTransactionCase):
    """A cron that runs as a service account and hits an AccessError inside a method that catches and logs it never
    fails visibly: the mail is simply never sent. The sibling sweep only checks Odoo's own gate before the action runs;
    this one runs the action."""

    # Tests [@ANCHOR: zero_sudo:cron_runs_clean_as_its_user]
    def test_every_installed_cron_runs_as_its_user_without_access_errors(self):
        rows = self.env["ir.model.data"].search([("model", "=", "ir.cron")])
        core_addons = os.path.dirname(get_module_path("base"))
        problems = []
        ran = []
        for row in rows:
            path = get_module_path(row.module, display_warning=False)
            if not path or os.path.dirname(path) == core_addons:
                continue
            cron = self.env["ir.cron"].with_context(active_test=False).browse(row.res_id).exists()
            if not cron:
                continue
            label = f"{row.module}.{row.name} (as {cron.user_id.login})"
            collector = _Collector()
            root = logging.getLogger()
            root.addHandler(collector)
            try:
                # A cron may commit; the test transaction forbids that, a second (test) cursor allows it.
                with self.registry.cursor() as cron_cr:
                    cron_env = api.Environment(cron_cr, cron.user_id.id, {"lastcall": cron.lastcall})
                    cron_env["ir.actions.server"].browse(cron.ir_actions_server_id.id).run()
            except Exception as error:  # audit-ignore-catch-all: report every kind of failure for every cron
                _logger.error("cron %s raised %s", label, type(error).__name__)
                problems.append(f"{label}: raised {type(error).__name__}: {str(error)[:300]}")
            finally:
                root.removeHandler(collector)
            problems.extend(f"{label}: swallowed: {line}" for line in collector.found)
            ran.append(label)
        _logger.info("cron smoke ran %d crons: %s", len(ran), "; ".join(ran))
        self.assertEqual(problems, [], "crons that fail or swallow an access error:\n" + "\n".join(problems))
