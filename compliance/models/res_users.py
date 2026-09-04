# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

from odoo import models


class ResUsers(models.Model):
    _inherit = "res.users"

    def _execute_gdpr_erasure(self):
        # [@ANCHOR: compliance_execute_gdpr_erasure]

        # # Verified by [@ANCHOR: COMM_test_execute_gdpr_erasure_deactivates_the_account] [@ANCHOR: COMM_test_execute_gdpr_erasure_uses_the_service_account_not_the_caller]
        """
        Base architectural contract for GDPR Erasure.
        Modules that manage user-generated content (e.g., user_websites, blog)
        should override this method to perform hard-deletion of their
        respective records.
        Per MASTER_02, implementations MUST impersonate the `gdpr_service_internal` account.

        Deactivates the account here, at the true base of the erasure chain, rather than
        leaving it to a downstream domain-specific module (ham_onboarding used to be the only
        place this happened): "delete all my content" deactivating the account afterward is
        generic erasure behavior that must hold regardless of which optional domain modules
        happen to be installed, not something that silently stops working the moment a module
        like ham_onboarding isn't part of the install set -- confirmed as a real, reproducible
        gap: user_websites' own test suite, run standalone (no dependency on ham_onboarding),
        asserted this and failed, since nothing in its own dependency chain performed it.
        Domain-specific identity scrubbing (callsign, login/name anonymization, and similar)
        stays in the modules that actually own that data -- this is deliberately narrow, just
        the one universal "the account is gone" bit every erasure must set.
        """
        self.ensure_one()
        # Real verification found this needs care, and a real gap this
        # session's own function-test-anchor sweep found and closed
        # (FUNCTION_TEST_ANCHOR_SWEEP.md): this method is compliance's own
        # base contract, but until now the res.users write grant for
        # zero_sudo.group_gdpr_service only existed in user_websites' own
        # ir.model.access.csv (access_res_users_gdpr_svc) -- meaning
        # compliance, exercised standalone (no user_websites installed,
        # e.g. this module's own real test suite), got a real AccessError
        # the moment this method actually ran. compliance now grants its
        # own copy (access_res_users_gdpr_svc_compliance,
        # compliance/security/ir.model.access.csv) -- the module that owns
        # this write is the one that should grant it, not rely on a
        # downstream module happening to also be installed. Odoo allows
        # multiple ir.model.access rows for the same model+group (the
        # effective permission is the OR across all of them), so
        # user_websites' own grant stays too, redundant but harmless.
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.gdpr_service_internal"
        )
        self.with_user(svc_uid).write({"active": False})

    def _get_gdpr_export_data(self):
        # [@ANCHOR: compliance_get_gdpr_export_data]

        # # Verified by [@ANCHOR: COMM_test_get_gdpr_export_data_always_returns_a_dict]
        """
        Base architectural contract for GDPR Export.
        Modules should override this to return user data for export.
        """
        return {}

    def _get_gdpr_streamed_keys(self):
        # [@ANCHOR: compliance_get_gdpr_streamed_keys]

        # # Verified by [@ANCHOR: COMM_test_get_gdpr_streamed_keys_always_returns_a_dict]
        """
        Base architectural contract for GDPR Export streaming.
        Modules with large/unbounded per-user datasets (QSO logs, blog
        posts, ...) should override this to return a dict of
        {key: generator_function} pairs so the export endpoint can stream
        each dataset instead of building it entirely in memory.
        """
        return {}
