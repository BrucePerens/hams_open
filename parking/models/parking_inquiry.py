# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import fields, models


class ParkingInquiry(models.Model):
    _name = "parking.inquiry"
    _description = "Inquiry about a for-sale domain"
    _order = "create_date desc"

    domain_id = fields.Many2one("parking.domain", required=True, ondelete="cascade", index=True)
    name = fields.Char()
    email = fields.Char(required=True)
    message = fields.Text(required=True)
    ip_hash = fields.Char(index=True, help="Keyed hash of the visitor address, for rate limiting only.")
