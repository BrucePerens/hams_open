# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import api, fields, models
from odoo.exceptions import ValidationError

from odoo.addons.tenant_sites.utils import host_matches

from .. import utils


class ParkingDomain(models.Model):
    _name = "parking.domain"
    _description = "Parked domain"
    _order = "name"

    _name_uniq = models.Constraint("UNIQUE (name)", "This domain is already configured.")

    name = fields.Char(
        string="Domain",
        required=True,
        index=True,
        help="Lowercase hostname, for example example.com or xn--bcher-kva.example. "
        "Internationalized names are stored in punycode.",
    )
    active = fields.Boolean(default=True)
    behavior = fields.Selection(
        [
            ("parked", "Parked page"),
            ("redirect", "Redirect"),
            ("for_sale", "For sale page with contact form"),
            ("gone", "410 Gone"),
        ],
        required=True,
        default="parked",
    )
    redirect_url = fields.Char(string="Redirect target", help="Absolute http(s) URL.")
    redirect_code = fields.Selection(
        [("301", "301 Moved Permanently"), ("302", "302 Found")], default="301"
    )
    preserve_path = fields.Boolean(
        string="Keep the path and query",
        help="Append the request path and query to the redirect target. The host always comes "
        "from the target, never from the request.",
    )
    include_www = fields.Boolean(
        string="Also answer for www.",
        default=True,
        help="www.<domain> is served with the same settings when it has no record of its own.",
    )
    title = fields.Char()
    message = fields.Text()
    price_text = fields.Char(string="Price or offer text")
    noindex = fields.Boolean(
        string="Ask search engines not to index", default=True, help="robots.txt and X-Robots-Tag."
    )
    cache_ttl = fields.Integer(
        string="Edge cache seconds",
        default=3600,
        help="Cache-Control s-maxage for this domain's responses.",
    )
    notes = fields.Text(groups="parking.group_parking_manager")
    inquiry_ids = fields.One2many("parking.inquiry", "domain_id")
    inquiry_count = fields.Integer(compute="_compute_inquiry_count")

    # [@ANCHOR: parking:COMM_domain_compute_inquiry_count]
    # Verified by [@ANCHOR: parking:COMM_test_domain_compute_inquiry_count]
    @api.depends("inquiry_ids")
    def _compute_inquiry_count(self):
        for record in self:
            record.inquiry_count = len(record.inquiry_ids)

    # [@ANCHOR: parking:COMM_domain_create]
    # Verified by [@ANCHOR: parking:COMM_test_domain_create]
    @api.model_create_multi
    def create(self, vals_list):
        for vals in vals_list:
            self._normalize_name(vals)
        return super().create(vals_list)

    # [@ANCHOR: parking:COMM_domain_write]
    # Verified by [@ANCHOR: parking:COMM_test_domain_write]
    def write(self, vals):
        self._normalize_name(vals)
        return super().write(vals)

    # [@ANCHOR: parking:COMM_domain_normalize_name]
    # Verified by [@ANCHOR: parking:COMM_test_domain_normalize_name]
    @api.model
    def _normalize_name(self, vals):
        if "name" not in vals:
            return
        normalized = utils.normalize_host(vals["name"])
        if not normalized:
            raise ValidationError(self.env._("'%s' is not a valid domain name.", vals["name"]))
        vals["name"] = normalized

    # [@ANCHOR: parking:COMM_domain_constraints]
    # Verified by [@ANCHOR: parking:COMM_test_domain_constraints]
    @api.constrains("behavior", "redirect_url", "name", "cache_ttl")
    def _check_behavior(self):
        for record in self:
            if record.behavior == "redirect":
                reason = utils.validate_redirect_url(record.redirect_url, record.name)
                if reason:
                    raise ValidationError(
                        self.env._("Invalid redirect target for %(name)s: %(reason)s", name=record.name, reason=reason)
                    )
            if not 0 <= record.cache_ttl <= 31536000:
                raise ValidationError(self.env._("Edge cache seconds must be between 0 and 31536000."))

    # [@ANCHOR: parking:COMM_domain_not_main]
    # Verified by [@ANCHOR: parking:COMM_test_domain_not_main]
    @api.constrains("name")
    def _check_not_main_or_tenant(self):
        """A parked domain must never shadow a hostname of the main site or of a tenant website."""
        hosts = self.env["tenant.site.host"]
        patterns = hosts._own_host_patterns()
        for record in self:
            if host_matches(record.name, patterns) or hosts._website_id_for_host(record.name):
                raise ValidationError(
                    self.env._("%s belongs to the main site or a tenant website.", record.name)
                )

    # [@ANCHOR: parking:COMM_domain_lookup]
    # Verified by [@ANCHOR: parking:COMM_test_domain_lookup]
    @api.model
    def _lookup(self, host):
        """The record that answers for `host` (exact name, else the bare domain for www.), or an
        empty recordset. `host` must already be normalized."""
        record = self.search([("name", "=", host)], limit=1)
        if record or not host.startswith("www."):
            return record
        parent = self.search([("name", "=", host[4:]), ("include_www", "=", True)], limit=1)
        return parent
