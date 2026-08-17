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
        """
        pass

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
