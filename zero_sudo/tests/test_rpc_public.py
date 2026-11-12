# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for the `@public_rpc` decorator that lives in zero_sudo.

Tests [@ANCHOR: zero_sudo:public_rpc]
"""
from odoo.tests.common import tagged
from odoo.addons.zero_sudo import public_rpc
from odoo.addons.zero_sudo.rpc_public import MARK
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestPublicRpcDecorator(HamsTransactionCase):
    """The decorator only marks a function (hams_rpc_guard's tests cover the guard and scanner reading the mark)."""

    def test_bare_use_marks_the_function_itself(self):
        def function():
            return 7
        self.assertIs(public_rpc(function), function)
        self.assertEqual(function(), 7)
        self.assertEqual(getattr(function, MARK), {"portal": False, "clients": ()})

    def test_arguments_are_recorded(self):
        @public_rpc(portal=True, clients=("daemon_a",))
        def function():
            pass
        self.assertEqual(getattr(function, MARK), {"portal": True, "clients": ("daemon_a",)})
