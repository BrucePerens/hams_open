# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

from odoo import models, api, tools
from odoo.modules.module import get_manifest
from odoo.exceptions import AccessError
import hashlib
import logging

_logger = logging.getLogger(__name__)


class Module(models.Model):
    _inherit = "ir.module.module"

    @api.model
    def _register_hook(self):
        super()._register_hook()
        # Always run on register_hook to ensure new modules get their docs
        self._bootstrap_knowledge_docs()

    @api.model
    def _bootstrap_knowledge_docs(self):
        # Dependencies formally guarantee knowledge.article
        article_model_name = "knowledge.article"

        utils = self.env["zero_sudo.security.utils"]

        svc_account = "knowledge.user_knowledge_service_account"
        if not self.env["ir.model.data"]._xmlid_to_res_id(
            svc_account, raise_if_not_found=False
        ):
            svc_account = "zero_sudo.odoo_facility_service_internal"

        # Context for creating documentation
        try:
            svc_uid = utils._get_service_uid(svc_account)
        except AccessError as e:
            _logger.warning("AccessError getting service uid: %s", e)
            return

        clean_ctx = dict(self.env.context)
        clean_ctx.pop("prefetch_fields", None)
        clean_ctx["mail_notrack"] = True

        Article = (
            self.env[article_model_name]
            .with_user(svc_uid)
            .with_context(**clean_ctx)
        )

        # Context for reading the core ERP framework table
        try:
            facility_uid = utils._get_service_uid(
                "zero_sudo.odoo_facility_service_internal"
            )
        except AccessError as e:
            _logger.warning("AccessError getting facility uid: %s", e)
            return

        all_doc_infos = []
        last_id = 0
        while True:
            batch = self.env["ir.module.module"].with_user(facility_uid).search(
                [("state", "=", "installed"), ("id", ">", last_id)],
                limit=1000,
                order="id asc"
            )
            if not batch:
                break
            for mod in batch:
                manifest = get_manifest(mod.name)
                if not manifest or "knowledge_docs" not in manifest:
                    continue

                knowledge_docs = manifest["knowledge_docs"]
                if not isinstance(knowledge_docs, list):
                    _logger.warning("knowledge_docs in module %s is not a list.", mod.name)
                    continue

                for doc_info in knowledge_docs:
                    all_doc_infos.append((mod.name, doc_info))
            last_id = batch[-1].id

        if not all_doc_infos:
            return

        hash_keys = []
        names = []
        for mod_name, doc_info in all_doc_infos:
            name = doc_info.get("name", f"{mod_name} Documentation")
            hash_key = f"zero_sudo.doc_hash_{mod_name}_{name.replace(' ', '_')}"
            hash_keys.append(hash_key)
            names.append(name)

        # Bulk load hashes
        env_svc = utils._get_service_env("zero_sudo.odoo_facility_service_internal")
        records = env_svc["zero_sudo.kv"].search([("key", "in", hash_keys)], limit=len(hash_keys))
        existing_hashes = {r.key: r.value for r in records}

        # Bulk load existing articles
        existing_articles = Article.search([("name", "in", names)], limit=len(names))
        article_by_name = {a.name: a for a in existing_articles}

        for mod_name, doc_info in all_doc_infos:
            self._install_single_doc(utils, Article, mod_name, doc_info, existing_hashes, article_by_name)

    @api.model
    def _install_single_doc(self, utils, Article, module_name, doc_info, existing_hashes=None, article_by_name=None):
        path = doc_info.get("path")
        if not path or ".." in path:
            return

        try:
            full_path = f"{module_name}/{path}"
            with tools.file_open(full_path, "rb") as f:
                content_bytes = f.read()
                content_hash = hashlib.sha256(content_bytes).hexdigest()
                doc_body = content_bytes.decode("utf-8")
        except OSError as e:
            _logger.error(
                "Failed to load doc file %s for module %s: %s", path, module_name, e
            )
            return

        name = doc_info.get("name", f"{module_name} Documentation")
        icon = doc_info.get("icon", "📄")
        category = doc_info.get("category", "workspace")

        hash_key = f"zero_sudo.doc_hash_{module_name}_{name.replace(' ', '_')}"
        existing_hash = existing_hashes.get(hash_key) if existing_hashes is not None else utils._get_kv(hash_key)

        if existing_hash == content_hash:
            return

        vals = {
            "name": name,
            "body": doc_body,
        }

        vals["is_published"] = True
        vals["category"] = category
        vals["internal_permission"] = "read"
        vals["icon"] = icon

        existing = article_by_name.get(name) if article_by_name is not None else Article.search([("name", "=", name)], limit=1)
        if existing:
            existing.write(vals)
        else:
            Article.create(vals)

        utils._set_kv(hash_key, content_hash)
        _logger.info("Installed/Updated knowledge documentation for %s", name)
