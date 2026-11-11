# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""A key file the reporting daemon's account cannot read is a failed report, never an exception that replaces the
daemon's own failure (the docstring of daemon/exception_reporter.py: reporting never raises into the original)."""
import os
import tempfile

from odoo.tests.common import BaseCase, tagged

from .test_exception_tickets import load_daemon_reporter


@tagged("post_install", "-at_install")
class TestDaemonReporterUnreadableKey(BaseCase):
    def setUp(self):
        super().setUp()
        self.helper = load_daemon_reporter()
        self.helper._LAST_REPORT.clear()

    def _key_file(self, content, mode):
        handle = tempfile.NamedTemporaryFile("wb", suffix=".env", delete=False)
        self.addCleanup(os.unlink, handle.name)
        with handle:
            handle.write(content)
        os.chmod(handle.name, mode)
        return handle.name

    def _unreadable_key_files(self):
        """Key files `load_key` cannot read: not UTF-8 text, and (for an account that is not root, which reads anything) mode 000."""
        yield "not UTF-8", self._key_file(b"ODOO_RPC_KEY=\xff\xfe\n", 0o600)
        if os.geteuid() != 0:
            yield "not readable", self._key_file(b"ODOO_RPC_KEY=abc\n", 0o000)

    def test_an_unreadable_or_undecodable_key_file_is_a_report_error(self):
        # Tests [@ANCHOR: hams_helpdesk:daemon_load_key]
        for name, path in self._unreadable_key_files():
            with self.subTest(name):
                with self.assertRaises(self.helper.ReportError) as caught:
                    self.helper.load_key(path)
                self.assertNotIn("abc", str(caught.exception))

    def test_reporting_failures_still_raises_the_daemons_own_exception_when_the_key_is_unreadable(self):
        # Tests [@ANCHOR: hams_helpdesk:daemon_reporting_failures]
        # Tests [@ANCHOR: hams_helpdesk:daemon_reporting_failures_init]
        # Tests [@ANCHOR: hams_helpdesk:daemon_reporting_failures_enter]
        # Tests [@ANCHOR: hams_helpdesk:daemon_reporting_failures_exit]
        for name, path in self._unreadable_key_files():
            with self.subTest(name):
                self.helper._LAST_REPORT.clear()
                with self.assertRaises(ZeroDivisionError):
                    with self.helper.reporting_failures("club-crawl.service", key_file=path):
                        raise ZeroDivisionError("raises on purpose")
                self.assertIsNone(self.helper.report_exception("other.service", RuntimeError("x"), key_file=path))
