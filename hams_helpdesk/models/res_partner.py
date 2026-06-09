from odoo import fields, models

class ResPartner(models.Model):
    _inherit = "res.partner"

    callsign = fields.Char(string="Callsign", help="Amateur Radio Callsign", index=True)
