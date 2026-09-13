# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

from odoo import models, api, tools
from odoo.modules.module import get_manifest, get_module_path
from odoo.exceptions import AccessError
import hashlib
import logging
import os

_logger = logging.getLogger(__name__)


class Module(models.Model):
    _inherit = "ir.module.module"

    @api.model
    # [@ANCHOR: zero_sudo:ir_module_register_hook]
    def _register_hook(self):
        super()._register_hook()
        # Always run on register_hook to ensure new modules get their docs
        self._bootstrap_knowledge_docs()

    @api.model
    # [@ANCHOR: zero_sudo:bootstrap_knowledge_docs]
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

        if article_model_name not in self.env:
            return

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
        article_id_keys = []
        for mod_name, doc_info in all_doc_infos:
            name = doc_info.get("name", f"{mod_name} Documentation")
            hash_key = f"zero_sudo.doc_hash_{mod_name}_{name.replace(' ', '_')}"
            article_id_key = f"zero_sudo.doc_article_id_{mod_name}_{name.replace(' ', '_')}"
            hash_keys.append(hash_key)
            article_id_keys.append(article_id_key)

        # Bulk load hashes AND article-identity ids in one query. Both are
        # keyed by (module_name, name) -- see _install_single_doc's own
        # article_id_key construction for why this replaced a by-`name`-only
        # `knowledge.article` search: two different modules can plausibly
        # pick the same doc `name` ("Getting Started", "Overview", ...), and
        # a shared-`name` article lookup would let the second module's
        # bootstrap pass silently overwrite the first module's article. See
        # night_shift_todo.md and the install_single_doc bug-hunt claim for
        # the full history of this fix.
        env_svc = utils._get_service_env("zero_sudo.odoo_facility_service_internal")
        all_kv_keys = hash_keys + article_id_keys
        records = env_svc["zero_sudo.kv"].search([("key", "in", all_kv_keys)], limit=len(all_kv_keys))
        kv_by_key = {r.key: r.value for r in records}
        existing_hashes = {k: v for k, v in kv_by_key.items() if k in set(hash_keys)}
        existing_article_ids = {k: v for k, v in kv_by_key.items() if k in set(article_id_keys)}

        for mod_name, doc_info in all_doc_infos:
            self._install_single_doc(
                utils, Article, mod_name, doc_info, existing_hashes, existing_article_ids
            )

    @api.model
    # [@ANCHOR: zero_sudo:is_path_within_module_dir]
    def _is_path_within_module_dir(self, base_dir, resolved_path):
        # bug-hunt (2026-09-13): a bare .startswith(base_dir) matches by
        # PREFIX with no path-separator boundary -- e.g. base_dir
        # ".../addons/zero_sudo" is a plain string-prefix of a sibling
        # directory ".../addons/zero_sudo_evil", so a resolved path escaping
        # into that sibling via this module's own manifest-declared
        # knowledge_docs path would have passed a bare-prefix check. Split
        # out as its own small, pure, directly-unit-testable method (no
        # real filesystem layout needed to exercise the boundary condition)
        # rather than inlined string logic in `_install_single_doc`.
        return resolved_path == base_dir or resolved_path.startswith(base_dir + os.sep)

    @api.model
    # [@ANCHOR: zero_sudo:install_single_doc]
    def _install_single_doc(self, utils, Article, module_name, doc_info, existing_hashes=None, existing_article_ids=None):
        path = doc_info.get("path")
        if not path or ".." in path.split(os.path.sep):
            return

        base_dir = os.path.realpath(get_module_path(module_name))
        resolved_path = os.path.realpath(os.path.join(base_dir, path))
        # The leading ".." guard above already blocks the common traversal
        # case, but this second, independent check (also catching an
        # absolute `path` that bypasses os.path.join's own base_dir
        # entirely) must hold the module-directory boundary itself, not
        # just a string prefix of it -- see _is_path_within_module_dir.
        if not self._is_path_within_module_dir(base_dir, resolved_path):
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
        # Module-authored docs (architecture notes, security internals, runbooks,
        # developer/story/journey guides) are internal engineering documentation by
        # default. A module must explicitly opt in with "public": True in its
        # knowledge_docs manifest entry for a doc to appear in the public/anonymous
        # website help sidebar -- see night_shift_todo.md, internal-doc-exposure fix.
        is_public = bool(doc_info.get("public", False))

        hash_key = f"zero_sudo.doc_hash_{module_name}_{name.replace(' ', '_')}"
        existing_hash = existing_hashes.get(hash_key) if existing_hashes is not None else utils._get_kv(hash_key)

        if existing_hash == content_hash:
            return

        vals = {
            "name": name,
            "body": doc_body,
        }

        vals["is_published"] = is_public
        vals["category"] = category
        vals["internal_permission"] = "read"
        vals["icon"] = icon

        # bug-hunt (2026-09-13): article identity is looked up by this
        # module's OWN previously-recorded article id, never by searching
        # `knowledge.article` on the shared, collision-prone `name` field --
        # see the module-level docstring on the caller and the
        # install_single_doc bug-hunt claim for the full "two modules pick
        # the same doc name" scenario this closes. The id is stored in a KV
        # entry keyed the same way as the content hash (module_name, name),
        # so it can never resolve to a DIFFERENT module's article even if
        # both chose an identical display name.
        article_id_key = f"zero_sudo.doc_article_id_{module_name}_{name.replace(' ', '_')}"
        existing_article_id = (
            existing_article_ids.get(article_id_key)
            if existing_article_ids is not None
            else utils._get_kv(article_id_key)
        )

        existing = Article.browse()
        if existing_article_id:
            try:
                existing_article_id = int(existing_article_id)
            except (TypeError, ValueError):
                existing_article_id = None
            if existing_article_id:
                candidate = Article.browse(existing_article_id)
                # The article may have been deleted since we recorded its id
                # (by a portal user, an admin, or a manual cleanup) -- must
                # re-check existence rather than trusting the stored id
                # blindly, or write() below would raise on a stale/missing
                # record.
                if candidate.exists():
                    existing = candidate

        if existing:
            existing.write(vals)
            article = existing
        else:
            article = Article.create(vals)

        utils._set_kv(hash_key, content_hash)
        utils._set_kv(article_id_key, str(article.id))
        _logger.info("Installed/Updated knowledge documentation for %s", name)
