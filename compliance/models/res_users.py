# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General
# Public License v3.0 (AGPL-3.0).

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
