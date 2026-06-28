from odoo import models


class ResUsers(models.Model):
    _inherit = "res.users"

    def _execute_gdpr_erasure(self):
        """
        Base architectural contract for GDPR Erasure.
        Modules that manage user-generated content (e.g., user_websites, blog)
        should override this method to perform hard-deletion of their respective records.
        """
        pass
