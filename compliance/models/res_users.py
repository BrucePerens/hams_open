# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

from odoo import models


class ResUsers(models.Model):
    _inherit = "res.users"

    def _execute_gdpr_erasure(self):
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
        # Real verification found this needs care: zero_sudo's own data-file
        # comment on group_gdpr_service claims it's scoped to user_websites'
        # "own models" only, but that comment is stale -- user_websites'
        # actual ir.model.access.csv grants this group real read/write on
        # res.users directly (access_res_users_gdpr_svc). Tried
        # zero_sudo.user_lockout_service_internal first instead (its ACL
        # grant looked more purpose-built, from its own doc comment), but
        # that grant lives in hams_base/security/ir.model.access.csv, and
        # hams_base is not a dependency of user_websites or compliance --
        # reproducing the exact cross-module gap this method exists to fix
        # (confirmed by a real AccessError when compliance is exercised via
        # user_websites' own standalone test suite). gdpr_service_internal's
        # grant lives in user_websites itself, which every realistic caller
        # of GDPR erasure already depends on (ham_onboarding and every other
        # domain module with erasure logic depends on user_websites), so
        # this is the account that's actually available in every real
        # calling context, not just the one this bug happened to be found
        # in.
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.gdpr_service_internal"
        )
        self.with_user(svc_uid).write({"active": False})

    def _get_gdpr_export_data(self):
        """
        Base architectural contract for GDPR Export.
        Modules should override this to return user data for export.
        """
        return {}

    def _get_gdpr_streamed_keys(self):
        """
        Base architectural contract for GDPR Export streaming.
        Modules with large/unbounded per-user datasets (QSO logs, blog
        posts, ...) should override this to return a dict of
        {key: generator_function} pairs so the export endpoint can stream
        each dataset instead of building it entirely in memory.
        """
        return {}
