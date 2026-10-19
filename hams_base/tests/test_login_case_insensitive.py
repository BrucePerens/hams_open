# -*- coding: utf-8 -*-
"""
A member who types their sign-in address with different capitalisation (phone keyboards capitalise
a callsign-shaped address such as N2YCH@icloud.com) must still sign in. Core matches ``login``
exactly while ``reset_password`` matches the e-mail case-insensitively, so the reset e-mail arrived
and the sign-in still failed.
"""
from odoo.exceptions import AccessDenied
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestLoginCaseInsensitive(HamsTransactionCase):
    def _make_user(self, login="case_test@example.com"):
        user = self.env["res.users"].create(
            {"name": "Case Test", "login": login, "email": login, "password": "correct horse battery staple"}
        )
        return user

    def _credential(self, login, password="correct horse battery staple"):
        return {"login": login, "password": password, "type": "password"}

    def test_mixed_case_login_signs_in(self):
        # Tests [@ANCHOR: hams_base:login_case_insensitive]
        user = self._make_user()
        for typed in ("CASE_TEST@EXAMPLE.COM", "Case_Test@Example.com", "  case_test@example.com "):
            auth = self.env["res.users"]._login(self._credential(typed), {"interactive": True})
            self.assertEqual(auth["uid"], user.id, typed)

    def test_wrong_password_still_denied_with_any_case(self):
        self._make_user()
        with self.assertRaises(AccessDenied):
            self.env["res.users"]._login(self._credential("CASE_TEST@EXAMPLE.COM", "wrong"), {"interactive": True})

    def test_underscore_is_not_a_wildcard(self):
        self._make_user()
        with self.assertRaises(AccessDenied):
            self.env["res.users"]._login(self._credential("case-test@example.com"), {"interactive": True})

    def test_reset_password_resolves_one_user_for_any_case(self):
        user = self._make_user()
        found = self.env["res.users"].search(self.env["res.users"]._get_login_domain("CASE_TEST@EXAMPLE.COM"))
        self.assertEqual(found, user)

    def test_create_and_write_store_lowercase_login(self):
        user = self._make_user("Upper_Case@Example.com")
        self.assertEqual(user.login, "upper_case@example.com")
        user.write({"login": " Other_Case@Example.COM "})
        self.assertEqual(user.login, "other_case@example.com")
