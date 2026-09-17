# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
import logging

from psycopg2 import IntegrityError

from odoo import models, fields, api
from cryptography.fernet import Fernet, InvalidToken

_logger = logging.getLogger(__name__)


class CloudflareTenantKey(models.Model):
    _name = "cloudflare.tenant.key"
    _description = "Per-Company Cloudflare Encryption Key (Envelope-Encrypted)"

    # [@ANCHOR: cloudflare:COMM_tenant_key_fields]
    # This module ships to the open source community, where one install
    # commonly serves several unrelated companies/tenants (multi-company
    # Odoo is a normal deployment shape, not an edge case) -- so the
    # Cloudflare Turnstile secret and API token, both encrypted at rest on
    # `website`, must not share one encryption key across every tenant on
    # the instance. A bug or leak exposing one company's key must not
    # expose another's. Envelope encryption gets that without inventing a
    # new secret-distribution mechanism: the single deployment-wide crypto
    # secret (zero_sudo.security.utils._get_crypto_secret(), never itself
    # stored in the DB -- env var, then a local file, then Odoo's
    # admin_passwd) acts as a key-encryption-key (KEK) that wraps a real,
    # randomly-generated, per-company data-encryption-key (DEK) stored
    # here, one row per company. Website secrets are encrypted with the
    # DEK, never the KEK directly.
    name = fields.Char(compute="_compute_name", store=True)
    company_id = fields.Many2one(
        "res.company", required=True, index=True, ondelete="cascade"
    )
    wrapped_key = fields.Char(
        string="Wrapped Per-Company Key",
        required=True,
        groups="base.group_system,cloudflare.group_cloudflare_purge,cloudflare.group_cloudflare_waf,cloudflare.group_cloudflare_tunnel",
    )

    _company_uniq = models.Constraint(
        "unique(company_id)",
        "Only one Cloudflare tenant key is allowed per company.",
    )

    @api.depends("company_id.name")
    def _compute_name(self):
        for rec in self:
            rec.name = f"Cloudflare Tenant Key ({rec.company_id.name})"

    # [@ANCHOR: cloudflare:COMM_tenant_key_get_or_create_fernet]
    @api.model
    def _get_or_create_fernet(self, company):
        """
        Returns a real, usable `Fernet` cipher scoped to `company`,
        generating and wrapping a fresh per-company key on first use.
        Returns None if the deployment-wide KEK isn't configured or is
        malformed, or if an existing wrapped key can't be unwrapped with
        it (matching website.py's own established fail-closed behavior
        for `_get_fernet`, which this replaces).
        """
        kek_material = self.env["zero_sudo.security.utils"]._get_crypto_secret()
        if not kek_material:
            return None
        try:
            kek = Fernet(kek_material.encode("utf-8"))
        except ValueError as e:
            _logger.warning("Deployment-wide crypto secret is malformed: %s", e)
            return None

        record = self.search([("company_id", "=", company.id)], limit=1)
        if not record:
            dek = Fernet.generate_key()
            wrapped = kek.encrypt(dek).decode("utf-8")
            try:
                with self.env.cr.savepoint():
                    record = self.create(
                        {"company_id": company.id, "wrapped_key": wrapped}
                    )
            except IntegrityError:
                # Two concurrent first-reads for the same company both took
                # this branch; the loser's create() hit the unique(company_id)
                # constraint. The savepoint rolls back only this insert, not
                # the caller's whole transaction -- the winner's row is
                # already committed-in-transaction and visible here.
                record = self.search([("company_id", "=", company.id)], limit=1)
                if not record:
                    raise

        try:
            dek = kek.decrypt(record.wrapped_key.encode("utf-8"))
            return Fernet(dek)
        except (ValueError, InvalidToken) as e:
            _logger.warning(
                "Could not unwrap the Cloudflare tenant key for company %s (id=%s): "
                "%s -- has the deployment-wide crypto secret changed since this key "
                "was wrapped?",
                company.name,
                company.id,
                e,
            )
            return None
