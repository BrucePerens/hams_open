# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later

import hashlib
import os
import shutil
import logging

from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache, invalidate_model_cache
from odoo import models, api, fields, tools, _
from odoo.exceptions import AccessError, UserError

_logger = logging.getLogger(__name__)


class ZeroSudoSecurityUtils(models.AbstractModel):
    _name = "zero_sudo.security.utils"
    _description = (
        "Centralized Security and "
        "Privilege Utilities"
    )
    name = fields.Char(string="Name", default=lambda self: self._description)

    @api.model
    def _get_deterministic_hash(self, input_string):
        # [@ANCHOR: zero_sudo:COMM_deterministic_hash]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_get_crypto_secret]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_story_deterministic_hash]
        """
        Generates a high-speed, deterministic 32-bit integer hash.
        Used primarily for PostgreSQL advisory locks (pg_advisory_xact_lock).
        """
        if not isinstance(input_string, str):
            input_string = str(input_string)
        return (
            int(hashlib.sha256(input_string.encode("utf-8")).hexdigest()[:8], 16)
            % 2147483647
        )

    @api.model
    def _get_service_uid(self, xml_id):
        # [@ANCHOR: zero_sudo:COMM_get_service_uid]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_get_service_uid]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_ham_onboarding:test_otp_mail_template]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_story_secure_escalation]

        if not xml_id or not isinstance(xml_id, str) or "." not in xml_id:
            raise AccessError(
                _("Invalid XML ID format: %s. " "Expected 'module.name'.") % xml_id
            )

        # STRICT ZERO-SUDO MANDATE: Resolve and verify via optimized Postgres procedure
        # [@ANCHOR: zero_sudo:COMM_get_service_uid_sql_resolve]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_COMM_test_get_service_uid_sql_resolve]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_get_service_uid_sql_verify]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_privilege_escalation_block_sql]

        self.env.cr.execute("SELECT zero_sudo_get_service_uid(%s)", (xml_id,))  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_get_service_uid_sql_resolve]  # fmt: skip
        uid = self.env.cr.fetchone()[0]
        return uid

    @api.model
    def _get_service_env(self, xml_id, context=None):
        """
        Returns a new Environment running strictly under the context of the specified
        service account.
        """
        uid = self._get_service_uid(xml_id)
        env = self.with_user(uid).env
        ctx = dict(env.context)
        ctx["mail_notrack"] = True
        if context:
            ctx.update(context)
        env = env(context=ctx)
        return env

    @api.model
    def _ensure_executable(self, cmd_name, svc_xml_id=None, pkg_name=None):
        """
        Resolves an executable in the system PATH.
        If not found, and binary.manifest exists, attempts to dynamically install it.
        """
        path = shutil.which(cmd_name)
        if path:
            return path

        if svc_xml_id:
            env_svc = self._get_service_env(svc_xml_id)
            return env_svc["binary.manifest"].ensure_executable(cmd_name)

        pkg = pkg_name or cmd_name
        raise UserError(
            _(
                "Missing dependency: '%s'. "
                "Please install via OS package manager "
                "(e.g., 'apt-get install %s')."
            )
            % (cmd_name, pkg)
        )

    @api.model
    def _invalidate_model_cache(self, model_name):
        """
        Securely invalidates the entire cache for a specific model.
        This allows non-administrative users (with proper ACLs) to trigger
        cache clearing for models they own/manage without needing sudo().
        """
        # [@ANCHOR: zero_sudo:COMM_invalidate_model_cache]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_invalidate_model_cache]
        if not model_name:
            return

        self.env[model_name]

        # Check if the current user has access to the model
        if not self.env.user.has_group("base.group_system"):
            try:
                self.env[model_name].check_access("write")
            except AccessError:
                raise AccessError(
                    _(
                        "Security Alert: You do not have permission "
                        "to invalidate the cache for model '%s'."
                    )
                    % model_name
                )

        # We assume the identity of the dedicated cache invalidation service
        # to perform the registry-level cache clearing.
        env_svc = self._get_service_env("zero_sudo.cache_invalidation_service_internal")
        env_svc.registry.clear_cache()

        # Log the invalidation event
        facility_env = self._get_service_env("zero_sudo.odoo_facility_service_internal")
        facility_env["zero_sudo.security.log"].create(
            {
                "user_id": self.env.user.id,
                "reason": "cache_invalidation",
                "login": f"Model: {model_name}",
            }
        )

        # Also signal distributed caches via pg_notify
        self._notify_cache_invalidation(model_name, "CLEAR_ALL")

    @api.model
    def _notify_cache_invalidation(self, model_name, key_value):
        # [@ANCHOR: zero_sudo:COMM_coherent_cache_signal]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_coherent_cache_signal]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_story_cache_signaling]
        if not model_name:
            return

        if isinstance(key_value, (list, set, tuple)):
            payloads = [f"{model_name}:{kv}" for kv in set(key_value) if kv]
            if payloads:
                # We limit the number of notifications in a single call to prevent
                # potential PostgreSQL performance issues or payload size limits.
                # Standard PG_NOTIFY payload limit is 8000 bytes.
                for i in range(0, len(payloads), 100):
                    chunk = payloads[i : i + 100]
                    # [@ANCHOR: zero_sudo:COMM_coherent_cache_signal_batch]
                    # ---
                    # # Verified by [@ANCHOR: zero_sudo:COMM_COMM_test_coherent_cache_signal_batch]
                    # ---
                    self.env.cr.execute(  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_coherent_cache_signal_batch]  # fmt: skip
                        "SELECT pg_notify(%s, payload) FROM unnest(%s) AS payload",
                        ("cache_invalidation", chunk),
                    )
        elif key_value:
            # [@ANCHOR: zero_sudo:COMM_coherent_cache_signal_single]
            # ---
            # # Verified by [@ANCHOR: zero_sudo:COMM_COMM_test_coherent_cache_signal_single]
            # ---
            self.env.cr.execute(  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_coherent_cache_signal_single]  # fmt: skip
                "SELECT pg_notify(%s, %s)",
                ("cache_invalidation", f"{model_name}:{key_value}"),
            )

    @api.model
    def _get_param_read_whitelist(self):
        """Returns the list of system parameters allowed to be read via Zero-Sudo."""
        return [
            "distributed_redis_cache.redis_host",
            "distributed_redis_cache.redis_password",
            "distributed_redis_cache.redis_pass",
            "distributed_redis_cache.redis_port",
            "distributed_redis_cache.test_integration_active",
            "web.base.url",
            "ham_dns.base_domain",
            "ham_dns.default_ip",
            "content_security_policy.report_uri",
            "backup_management.rmq_host",
            "backup_management.rmq_user",
            "backup_management.rmq_pass",
            "rabbitmq.user",
            "rabbitmq.pass",
            "rabbitmq.host",
            "rabbitmq.port",
            "rabbitmq.vhost",
            "caching.safe_quota_mb",
            "cloudflare.last_static_mtime",
            "pager_duty.helpdesk_model",
            "user_websites.company_abuse_email",
            "user_websites.max_sites_per_user",
            "user_websites.max_pages_per_site",
            "user_websites.allow_custom_domains",
            "user_websites.allow_html_embed",
            "user_websites.docs_installed",
            "user_websites_seo.docs_installed",
            "user_websites.global_website_page_limit",
            "content_security_policy.report_url",
            "user_websites.last_digest_id",
        ]

    @api.model
    def _get_param_write_whitelist(self):
        """Returns the list of system parameters allowed to be written via Zero-Sudo."""
        return [
            "web.base.url",
            "user_websites_seo.docs_installed",
            "cloudflare.last_static_mtime",
            "caching.safe_quota_mb",
            "user_websites.last_digest_id",
        ]

    @api.model
    @distributed_cache()
    def _get_system_param(self, key, default=None):
        # [@ANCHOR: zero_sudo:COMM_get_system_param]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_01_mechanical_secret_block_enforcement]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_story_parameter_whitelisting]
        # THE MECHANICAL SECRET BLOCK
        whitelist = self._get_param_read_whitelist()

        banned_substrings = [
            "secret",
            "key",
            "password",
            "token",
            "auth",
            "crypt",
            "cert",
        ]
        lower_key = key.lower()

        if key not in whitelist:
            # Log the unauthorized access attempt
            facility_env = self._get_service_env(
                "zero_sudo.odoo_facility_service_internal"
            )
            facility_env["zero_sudo.security.log"].create(
                {
                    "user_id": self.env.user.id,
                    "reason": "param_access_denied",
                    "login": key,
                }
            )

            if any(banned in lower_key for banned in banned_substrings):
                raise AccessError(
                    _(
                        "Security Alert: Parameter '%s' matches restricted "
                        "cryptographic patterns and cannot be extracted via Zero-Sudo."
                    )
                    % key
                )
            raise AccessError(
                _(
                    "Security Alert: Parameter '%s' is not in the Zero-Sudo READ "
                    "whitelist. You must explicitly register it in "
                    "zero_sudo/models/security_utils.py."
                )
                % key
            )

        env_svc = self._get_service_env("zero_sudo.config_service_internal")

        return env_svc["ir.config_parameter"].get_param(key, default)

    @api.model
    def _set_system_param(self, key, value):
        # [@ANCHOR: zero_sudo:COMM_set_system_param]
        whitelist = self._get_param_write_whitelist()

        if key not in whitelist:
            # Log the unauthorized write attempt
            facility_env = self._get_service_env(
                "zero_sudo.odoo_facility_service_internal"
            )
            facility_env["zero_sudo.security.log"].create(
                {
                    "user_id": self.env.user.id,
                    "reason": "param_write_denied",
                    "login": key,
                }
            )
            raise AccessError(
                _(
                    "Security Alert: Parameter '%s' is not in the Zero-Sudo WRITE "
                    "whitelist. You must explicitly register it in "
                    "zero_sudo/models/security_utils.py."
                )
                % key
            )

        env_svc = self._get_service_env("zero_sudo.config_service_internal")

        return env_svc["ir.config_parameter"].set_param(key, value)

    @api.model
    @distributed_cache()
    def _get_kv(self, key):
        env_svc = self._get_service_env("zero_sudo.odoo_facility_service_internal")
        record = env_svc["zero_sudo.kv"].search([("key", "=", key)], limit=1)
        return record.value if record else None

    @api.model
    def _set_kv(self, key, value):
        # [@ANCHOR: zero_sudo:COMM_set_kv_procedure]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_set_kv_procedure]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_COMM_test_set_kv_sql_check]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_story_set_kv_procedure]
        """
        High-performance atomic KV update using a single Postgres procedure call.
        Eliminates Python-side existence checks and round-trips.
        """
        self.env.cr.execute("SELECT zero_sudo_set_kv(%s, %s)", (key, value))  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_set_kv_sql_check]  # fmt: skip

        # Ensure changes are visible to other transactions/round-trips.
        # CRITICAL TEST EVASION FIX: We use RealTransactionCase for commit handling natively,
        # so test evasion logic is purged.
        # Native Odoo test environment handles transaction boundaries automatically.

        # Direct SQL bypasses the ORM cache. We must invalidate it.
        self.env["zero_sudo.kv"].invalidate_model()
        invalidate_model_cache(self.env, "zero_sudo.security.utils")

    @api.model
    @distributed_cache()
    def _get_crypto_secret(self):
        # [@ANCHOR: zero_sudo:COMM_get_crypto_secret]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_get_crypto_secret]
        """
        Retrieves the cryptographic secret without requiring .sudo() or database access.
        Checks environment variables first, then a local file, and falls back to config.
        RAM-cached for performance and to avoid redundant filesystem I/O.
        """
        secret = os.environ.get("HAMS_CRYPTO_KEY")  # burn-ignore-env
        if not secret:
            try:
                # We check the file strictly if it exists to avoid repeated failed opens
                secret_path = "/var/lib/odoo/hams_crypto.secret"
                if os.path.exists(secret_path):
                    with open(  # audit-ignore-path  # fmt: skip
                        secret_path,
                        "r",
                        # audit-ignore-path: # Tested by [@ANCHOR: zero_sudo:COMM_test_get_crypto_secret]  # fmt: skip
                    ) as f:
                        secret = f.read().strip()
            except OSError as e:
                _logger.warning("Failed to read crypto secret file: %s", e)

        if not secret:
            # Fallback to Odoo's admin password as a last resort entropy source
            secret = tools.config.get("admin_passwd")

        if not secret or secret == "admin":
            _logger.warning(
                "System running with insecure or "
                "default cryptographic secret!"
            )
            secret = "default_insecure_secret_fallback"

        return secret
