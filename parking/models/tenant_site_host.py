# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import api, models

SERVICE_USER_XMLID = "parking.user_parking_service"


class TenantSiteHost(models.Model):
    _inherit = "tenant.site.host"

    # [@ANCHOR: parking:COMM_tenant_hosts_exist]
    # Verified by [@ANCHOR: parking:COMM_test_tenant_hosts_exist]
    @api.model
    def _tenant_hosts_exist(self):
        """Parked domains count as tenant hostnames for the tunnel guard."""
        if super()._tenant_hosts_exist():
            return True
        uid = self.env["ir.model.data"]._xmlid_to_res_id(SERVICE_USER_XMLID, raise_if_not_found=True)
        return bool(self.with_user(uid).env["parking.domain"].search_count([], limit=1))
