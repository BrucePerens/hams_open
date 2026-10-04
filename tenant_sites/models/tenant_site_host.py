# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import api, fields, models
from odoo.exceptions import ValidationError
from odoo.http import request

from .. import utils

SERVICE_XMLID = "tenant_sites.user_tenant_sites_service"
OWN_HOST_LIMIT = 1000


class TenantSiteHost(models.Model):
    _name = "tenant.site.host"
    _description = "Hostname of a tenant website"
    _order = "name"

    _name_uniq = models.Constraint("UNIQUE (name)", "This hostname already belongs to a tenant site.")

    name = fields.Char(string="Hostname", required=True, index=True)
    site_id = fields.Many2one("tenant.site", required=True, ondelete="cascade", index=True)
    website_id = fields.Many2one(related="site_id.website_id", store=True, index=True)

    # [@ANCHOR: tenant_sites:COMM_host_normalize]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_host_normalize]
    @api.model
    def _normalize_vals(self, vals):
        if "name" not in vals:
            return
        normalized = utils.normalize_host(vals["name"])
        if not normalized:
            raise ValidationError(self.env._("'%s' is not a valid hostname.", vals["name"]))
        vals["name"] = normalized

    # [@ANCHOR: tenant_sites:COMM_host_create]
    @api.model_create_multi
    def create(self, vals_list):
        for vals in vals_list:
            self._normalize_vals(vals)
        return super().create(vals_list)

    # [@ANCHOR: tenant_sites:COMM_host_write]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_host_normalize]
    def write(self, vals):
        self._normalize_vals(vals)
        return super().write(vals)

    # [@ANCHOR: tenant_sites:COMM_host_not_own]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_host_not_own]
    @api.constrains("name")
    def _check_not_own_host(self):
        """A tenant must never take over a hostname that belongs to the main site."""
        patterns = self._own_host_patterns()
        for record in self:
            if utils.host_matches(record.name, patterns):
                raise ValidationError(
                    self.env._("%s is a hostname of the main site and cannot belong to a tenant.", record.name)
                )

    @api.model
    def _service_env(self):
        return self.env["zero_sudo.security.utils"]._get_service_env(SERVICE_XMLID)

    # [@ANCHOR: tenant_sites:COMM_request_memo]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_request_memo]
    @api.model
    def _request_memo(self):
        """A dict that lives as long as the current HTTP request, or None outside one. The host
        tables are small indexed queries, read fresh on purpose (a cache would need cross-worker
        invalidation to be correct), but a request asks the same question many times."""
        if not request:
            return None
        if not hasattr(request, "tenant_sites_memo"):
            request.tenant_sites_memo = {}
        return request.tenant_sites_memo

    # [@ANCHOR: tenant_sites:COMM_own_host_patterns]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_own_host_patterns]
    @api.model
    def _own_host_patterns(self):
        """The hostname patterns of the main site, as a tuple. Empty means nothing is configured."""
        memo = self._request_memo()
        if memo is not None and "own" in memo:
            return memo["own"]
        own = self._service_env()["tenant.site.own.host"].search([], limit=OWN_HOST_LIMIT)
        patterns = tuple(own.mapped("name"))
        if memo is not None:
            memo["own"] = patterns
        return patterns

    # [@ANCHOR: tenant_sites:COMM_website_id_for_host]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_website_id_for_host]
    @api.model
    def _website_id_for_host(self, raw_host):
        """The tenant website that answers for `raw_host` (a Host header value), or 0."""
        host = utils.normalize_host(raw_host)
        if not host:
            return 0
        memo = self._request_memo()
        key = ("site", host)
        if memo is not None and key in memo:
            return memo[key]
        domain = [("name", "=", host), ("site_id.active", "=", True)]
        record = self._service_env()["tenant.site.host"].search(domain, limit=1)
        website_id = record.website_id.id
        if memo is not None:
            memo[key] = website_id
        return website_id

    # [@ANCHOR: tenant_sites:COMM_request_website_id]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_request_website_id]
    @api.model
    def _tenant_request_website_id(self):
        """The tenant website of the request being served, or 0 when it is not a tenant request.
        Reads only an attribute the request router already set: it never queries, so the models
        it guards can call it from inside their own searches."""
        if not request:
            return 0
        if not hasattr(request, "tenant_sites_website_id"):
            return 0
        return request.tenant_sites_website_id

    # [@ANCHOR: tenant_sites:COMM_tenant_hosts_exist]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_tenant_hosts_exist]
    @api.model
    def _tenant_hosts_exist(self):
        """True when this database serves any tenant (or other non-main) hostname. Modules that add
        their own kinds of hostname (parking) extend it."""
        domain = [("site_id.active", "=", True)]
        return bool(self._service_env()["tenant.site.host"].search_count(domain, limit=1))
