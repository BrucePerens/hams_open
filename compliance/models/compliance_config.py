# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
from odoo import models, api

class ComplianceConfig(models.AbstractModel):
    _name = "compliance.config"
    _description = "Compliance Configuration Hook"

    @api.model
    def _register_hook(self):
        """
        Ensures documentation is installed upon registry loading.
        """
        # [@ANCHOR: soft_dependency_docs_installation]
        # Verified by [@ANCHOR: test_zero_sudo_doc_installer]
        # ADR-0055: Soft-dependency documentation bootstrap.
        # We trigger this whenever the registry is loaded to ensure the manual is always present.
        self.env["ir.module.module"]._bootstrap_knowledge_docs()
