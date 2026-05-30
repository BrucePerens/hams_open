# -*- coding: utf-8 -*-
from odoo import models, api, tools
from odoo.modules.module import get_manifest
from odoo.exceptions import AccessError
import hashlib
import logging

_logger = logging.getLogger(__name__)

class Module(models.Model):
    _inherit = 'ir.module.module'

    @api.model
    def _register_hook(self):
        # [@ANCHOR: zero_sudo:zero_sudo_register_hook]
        # Verified by [@ANCHOR: zero_sudo:test_zero_sudo_register_hook]
        super()._register_hook()
        # Always run on register_hook to ensure new modules get their docs
        self._bootstrap_knowledge_docs()

    @api.model
    def _bootstrap_knowledge_docs(self):
        # [@ANCHOR: zero_sudo:zero_sudo_doc_installer]
        # Verified by [@ANCHOR: zero_sudo:test_zero_sudo_doc_installer]

        article_model_name = None
        if 'knowledge.article' in self.env:
            article_model_name = 'knowledge.article'
        elif 'manual.article' in self.env:
            article_model_name = 'manual.article'

        if not article_model_name:
            return

        utils = self.env['zero_sudo.security.utils']

        svc_account = "manual_library.user_manual_library_service_account"
        if not self.env["ir.model.data"]._xmlid_to_res_id(svc_account, raise_if_not_found=False):
             svc_account = "zero_sudo.odoo_facility_service_internal"

        # Context for creating documentation
        try:
            svc_uid = utils._get_service_uid(svc_account)
        except AccessError:
            return

        Article = self.env[article_model_name].with_user(svc_uid).with_context(
            mail_notrack=True
        )

        # Context for reading the core ERP framework table
        facility_uid = utils._get_service_uid("zero_sudo.odoo_facility_service_internal")
        modules = self.env['ir.module.module'].with_user(facility_uid).search([('state', '=', 'installed')], limit=10000)

        for mod in modules:
            manifest = get_manifest(mod.name)
            if not manifest or 'knowledge_docs' not in manifest:
                continue

            knowledge_docs = manifest['knowledge_docs']
            if not isinstance(knowledge_docs, list):
                _logger.warning("knowledge_docs in module %s is not a list.", mod.name)
                continue

            for doc_info in knowledge_docs:
                self._install_single_doc(utils, Article, mod.name, doc_info)

    @api.model
    def _install_single_doc(self, utils, Article, module_name, doc_info):
        path = doc_info.get('path')
        if not path:
            return

        try:
            full_path = f"{module_name}/{path}"
            with tools.file_open(full_path, 'rb') as f:
                content_bytes = f.read()
                content_hash = hashlib.sha256(content_bytes).hexdigest()
                doc_body = content_bytes.decode('utf-8')
        except OSError as e:
            _logger.error("Failed to load doc file %s for module %s: %s", path, module_name, e)
            return

        name = doc_info.get('name', f"{module_name} Documentation")
        icon = doc_info.get('icon', '📄')
        category = doc_info.get('category', 'workspace')

        hash_key = f"zero_sudo.doc_hash_{module_name}_{name.replace(' ', '_')}"
        existing_hash = utils._get_kv(hash_key)

        if existing_hash == content_hash:
            return

        vals = {
            'name': name,
            'body': doc_body,
        }

        model_fields = Article._fields
        if 'is_published' in model_fields:
            vals['is_published'] = True
        if 'category' in model_fields:
            vals['category'] = category
        if 'internal_permission' in model_fields:
            vals['internal_permission'] = 'read'
        if 'icon' in model_fields:
            vals['icon'] = icon

        existing = Article.search([('name', '=', name)], limit=1)
        if existing:
            existing.write(vals)
        else:
            Article.create(vals)

        utils._set_kv(hash_key, content_hash)
        _logger.info("Installed/Updated knowledge documentation for %s", name)
