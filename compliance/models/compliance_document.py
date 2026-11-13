# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

from odoo import models, fields

# [@ANCHOR: compliance_compliance_document_compliance_document]
class ComplianceDocument(models.Model):
    _name = "compliance.document"
    _description = "Compliance Document"
    _order = "sequence, id"
    
    name = fields.Char(string="Name", required=True)
    description = fields.Text(string="Description")
    url = fields.Char(string="URL")
    sequence = fields.Integer(string="Sequence", default=10)
    active = fields.Boolean(string="Active", default=True)
