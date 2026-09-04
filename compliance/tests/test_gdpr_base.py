# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestComplianceGdprBase(HamsTransactionCase):
    """
    compliance/models/res_users.py's own three GDPR base-contract methods
    (_execute_gdpr_erasure, _get_gdpr_export_data, _get_gdpr_streamed_keys)
    had no direct test of their own before this -- ANCHOR_COVERAGE_AND_
    REMEDIATION_PLAN.md/FUNCTION_TEST_ANCHOR_SWEEP.md's own real sweep.
    """

    def setUp(self):
        super().setUp()
        self.user = self.env["res.users"].create(
            {
                "name": "Compliance GDPR Sweep User",
                "login": "compliance_gdpr_sweep",
                "email": "compliance_gdpr_sweep@example.com",
            }
        )

    def test_execute_gdpr_erasure_deactivates_the_account(self):
        # [@ANCHOR: COMM_test_execute_gdpr_erasure_deactivates_the_account]

        # Tests [@ANCHOR: compliance_execute_gdpr_erasure]
        self.assertTrue(self.user.active, "the fixture user must start active")
        self.user._execute_gdpr_erasure()
        self.user.invalidate_recordset(["active"])
        self.assertFalse(
            self.user.active,
            "[!] DIAGNOSTIC FOR AI: _execute_gdpr_erasure() must deactivate the account "
            "at the base of the erasure chain regardless of which optional domain modules "
            "(user_websites, ham_onboarding, etc.) are installed.",
        )

    def test_execute_gdpr_erasure_uses_the_service_account_not_the_caller(self):
        # [@ANCHOR: COMM_test_execute_gdpr_erasure_uses_the_service_account_not_the_caller]

        # Tests [@ANCHOR: compliance_execute_gdpr_erasure]
        # A plain internal user normally cannot flip another user's own
        # `active` field -- if this succeeds without an AccessError, that's
        # real, observable evidence the method is genuinely impersonating
        # gdpr_service_internal (per its own doc comment) rather than
        # silently running as whatever unprivileged user happened to call
        # it, which real callers (e.g. an account-deletion controller
        # acting for the logged-in user) are.
        low_priv_user = self.env["res.users"].create(
            {
                "name": "Low-Privilege Caller",
                "login": "compliance_gdpr_low_priv_caller",
                "email": "low_priv@example.com",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )
        self.user.with_user(low_priv_user)._execute_gdpr_erasure()
        self.user.invalidate_recordset(["active"])
        self.assertFalse(
            self.user.active,
            "[!] DIAGNOSTIC FOR AI: erasure must succeed even when called by a "
            "low-privilege user, proving it impersonates the real service account "
            "rather than running with the caller's own (insufficient) rights.",
        )

    def test_get_gdpr_export_data_always_returns_a_dict(self):
        # [@ANCHOR: COMM_test_get_gdpr_export_data_always_returns_a_dict]

        """
        Deliberately NOT asserting exact content: Odoo's `_inherit`
        mechanism means calling `self.user._get_gdpr_export_data()` always
        resolves to whatever module actually overrides it in this test
        database's real installed set (user_websites' own test_gdpr_base.py
        already covers ITS OWN override's exact schema) -- there is no way
        to call "only compliance's own base implementation" in isolation
        once Odoo's registry has merged the inheritance chain. What IS
        always true regardless of the installed module set is the base
        contract's own documented promise: a dict, never None, never a
        crash -- that's what this test actually verifies.
        """
        # Tests [@ANCHOR: compliance_get_gdpr_export_data]
        data = self.user._get_gdpr_export_data()
        self.assertIsInstance(
            data,
            dict,
            "[!] DIAGNOSTIC FOR AI: _get_gdpr_export_data() must always return a dict, "
            "the base contract every override merges into.",
        )

    def test_get_gdpr_streamed_keys_always_returns_a_dict(self):
        # [@ANCHOR: COMM_test_get_gdpr_streamed_keys_always_returns_a_dict]

        # Tests [@ANCHOR: compliance_get_gdpr_streamed_keys]
        # Same cross-module-override reasoning as
        # test_get_gdpr_export_data_always_returns_a_dict above.
        keys = self.user._get_gdpr_streamed_keys()
        self.assertIsInstance(
            keys,
            dict,
            "[!] DIAGNOSTIC FOR AI: _get_gdpr_streamed_keys() must always return a dict "
            "of {key: generator_function} pairs, the base contract every override merges into.",
        )
