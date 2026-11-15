# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The QWeb `@t-esc` deprecation warning is suppressed where QWeb logs it, and nothing else is."""
import logging

from odoo.tests.common import BaseCase, tagged

_QWEB_LOGGER = "odoo.addons.base.models.ir_qweb"
_MESSAGE = "Found deprecated directive @t-esc=%r in template %r. Replace by @t-out"


class _Collect(logging.Handler):
    def __init__(self):
        super().__init__(logging.DEBUG)
        self.messages = []

    def emit(self, record):
        self.messages.append(record.getMessage())


@tagged("post_install", "-at_install")
class TestTEscWarningFilter(BaseCase):
    def _logged_through(self, logger_name, message, *args):
        collector = _Collect()
        root = logging.getLogger()
        root.addHandler(collector)
        try:
            logging.getLogger(logger_name).warning(message, *args)
        finally:
            root.removeHandler(collector)
        return collector.messages

    def test_the_qweb_deprecation_warning_for_t_esc_never_reaches_a_handler(self):
        # Tests [@ANCHOR: zero_sudo___init___tesc_warning_filter]
        # The record is logged on QWeb's own logger and reaches the root handlers by propagation, as in a real run.
        self.assertEqual(self._logged_through(_QWEB_LOGGER, _MESSAGE, "x", "some.template"), [])

    def test_other_warnings_from_qweb_and_the_t_esc_text_elsewhere_still_get_through(self):
        # Tests [@ANCHOR: zero_sudo___init___tesc_warning_filter]
        self.assertEqual(self._logged_through(_QWEB_LOGGER, "Found deprecated directive @t-raw in template %r", "t"),
                         ["Found deprecated directive @t-raw in template 't'"])
        self.assertEqual(self._logged_through(_QWEB_LOGGER, "template %r failed to render", "t"), ["template 't' failed to render"])
        self.assertEqual(len(self._logged_through("odoo.some.other", _MESSAGE, "x", "t")), 1)
