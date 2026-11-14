# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# License: AGPL-3.0
from werkzeug.routing import Map, ValidationError

from odoo.tests import tagged

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestBoundedIntConverter(HamsTransactionCase):

    def _converter(self):
        return self.env["ir.http"]._get_converters()["int"](Map())

    def test_01_an_id_in_range_passes(self):
        self.assertEqual(self._converter().to_python("2147483647"), 2147483647)
        self.assertEqual(self._converter().to_python("7"), 7)

    def test_02_a_number_past_a_database_id_is_refused(self):
        # Tests [@ANCHOR: hams_base:bounded_int_converter]
        # Tests [@ANCHOR: hams_base:bounded_int_converters]
        for text in ("2147483648", "99999999999999999999", "-2147483649", "9" * 400):
            with self.assertRaises(ValidationError, msg=text):
                self._converter().to_python(text)

    def test_03_a_route_can_raise_the_limit(self):
        # Tests [@ANCHOR: hams_base:bounded_int_converter_init]
        converter = self.env["ir.http"]._get_converters()["int"](Map(), max=9999999999)
        self.assertEqual(converter.to_python("4000000000"), 4000000000)
