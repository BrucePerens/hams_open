# -*- coding: utf-8 -*-
import hashlib
import os
import subprocess
import sys
import shutil
from odoo import models, api, tools, _
from odoo.exceptions import AccessError, UserError


class ZeroSudoSecurityUtils(models.AbstractModel):
    _name = "zero_sudo.security.utils"
    _description = "Centralized Security and Privilege Utilities"

    @api.model
    def _get_deterministic_hash(self, input_string):
        # [@ANCHOR: deterministic_hash]
        # Verified by [@ANCHOR: test_deterministic_hash]
        # Tests [@ANCHOR: story_deterministic_hash]
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
    @tools.ormcache("xml_id")
    def _get_service_uid(self, xml_id):
        # [@ANCHOR: get_service_uid]
        # Verified by [@ANCHOR: test_get_service_uid]
        # Tests [@ANCHOR: story_secure_escalation]
        if "." not in xml_id:
            raise AccessError(_("Invalid XML ID format: %s") % xml_id)
        module, name = xml_id.split(".", 1)

        # STRICT ZERO-SUDO MANDATE: Resolve the ID using raw SQL to prevent any ORM/sudo bypasses
        self.env.cr.execute(
            "SELECT res_id FROM ir_model_data WHERE module = %s AND name = %s AND model = 'res.users'",
            (module, name)
        )
        res_id_row = self.env.cr.fetchone()
        if not res_id_row:
            raise AccessError(
                _("Security Alert: Service Account '%s' not found.") % xml_id
            )
        uid = res_id_row[0]

        # Verify the account is active AND is explicitly flagged as a service account
        self.env.cr.execute(
            "SELECT active, is_service_account FROM res_users WHERE id = %s", (uid,)
        )
        res = self.env.cr.fetchone()

        if not res or not res[0]:
            raise AccessError(_("Security Alert: Service Account is disabled."))
        if not res[1]:
            raise AccessError(
                _(
                    "Security Alert: '%s' is a human user, not a Service Account. Privilege escalation denied."
                )
                % xml_id
            )

        # THE MECHANICAL GOD-MODE BLOCK: Ensure the service account does not possess global administrative privileges.
        # This mathematically forces downstream modules to utilize the Micro-Service Account CSV pattern.
        self.env.cr.execute(
            """
            SELECT 1 FROM res_groups_users_rel rel
            JOIN ir_model_data imd ON imd.res_id = rel.gid AND imd.model = 'res.groups'
            WHERE rel.uid = %s AND imd.module = 'base' AND imd.name IN ('group_system', 'group_erp_manager')
        """,
            (uid,),
        )

        if self.env.cr.fetchone():
            raise AccessError(
                _(
                    "Security Alert: Service Account '%s' violates the Zero-Sudo mandate by possessing global administrative groups (group_system or group_erp_manager). You must use domain-specific Micro-Privilege ACLs instead."
                )
                % xml_id
            )

        return uid

    @api.model
    def _get_service_env(self, xml_id):
        """
        Returns a new Environment running strictly under the context of the specified
        service account. Automatically disables tracking per ADR-0001
        to prevent ORM cascade Access Errors.
        """
        uid = self._get_service_uid(xml_id)
        return self.with_user(uid).with_context(mail_notrack=True).env

    @api.model
    def _ensure_executable(self, cmd_name, svc_xml_id=None, pkg_name=None):
        """
        Resolves an executable in the system PATH.
        If not found, and binary.manifest exists, attempts to dynamically install it.
        """
        path = shutil.which(cmd_name)
        if path:
            return path

        if "binary.manifest" in self.env and svc_xml_id:
            env_svc = self._get_service_env(svc_xml_id)
            return env_svc["binary.manifest"].ensure_executable(cmd_name)

        pkg = pkg_name or cmd_name
        raise UserError(
            _("Missing dependency: '%s'. Please install via OS package manager (e.g., 'apt-get install %s').")
            % (cmd_name, pkg)
        )

    @api.model
    def _notify_cache_invalidation(self, model_name, key_value):
        # [@ANCHOR: coherent_cache_signal]
        # Verified by [@ANCHOR: test_coherent_cache_signal]
        # Tests [@ANCHOR: story_cache_signaling]
        if isinstance(key_value, (list, set, tuple)):
            payloads = [f"{model_name}:{kv}" for kv in set(key_value) if kv]
            if payloads:
                self.env.cr.execute(
                    "SELECT pg_notify(%s, payload) FROM unnest(%s) AS payload",
                    ("cache_invalidation", payloads),
                )
        else:
            self.env.cr.execute(
                "SELECT pg_notify(%s, %s)",
                ("cache_invalidation", f"{model_name}:{key_value}"),
            )

    @api.model
    def _get_param_whitelist(self):
        """Returns the list of system parameters allowed to be read/written via Zero-Sudo."""
        return [
            "web.base.url",
            "cloudflare.last_static_mtime",
            "user_websites.company_abuse_email",
            "user_websites.max_sites_per_user",
            "user_websites.enable_blog_comments",
            "caching.safe_quota_mb",
            "caching.invalidation_version",
            "user_websites.global_website_page_limit",
            "user_websites.last_digest_id",
            "user_websites_seo.docs_installed",
            "pager_duty.helpdesk_model",
        ]

    @api.model
    def _get_system_param(self, key, default=None):
        # [@ANCHOR: get_system_param]
        # Verified by [@ANCHOR: test_01_mechanical_secret_block_enforcement]
        # Tests [@ANCHOR: story_parameter_whitelisting]
        # THE MECHANICAL SECRET BLOCK
        whitelist = self._get_param_whitelist()

        banned_substrings = [
            "secret", "key", "password", "token", "auth", "crypt", "cert",
        ]
        lower_key = key.lower()

        if key not in whitelist:
            if any(banned in lower_key for banned in banned_substrings):
                raise AccessError(
                    _(
                        "Security Alert: Parameter '%s' matches restricted cryptographic patterns and cannot be extracted via Zero-Sudo."
                    )
                    % key
                )
            raise AccessError(
                _(
                    "Security Alert: Parameter '%s' is not in the Zero-Sudo PARAM_WHITELIST. You must explicitly register it in zero_sudo/models/security_utils.py."
                )
                % key
            )

        env_svc = self._get_service_env("zero_sudo.config_service_internal")
        return env_svc["ir.config_parameter"].get_param(key, default)

    @api.model
    def _set_system_param(self, key, value):
        # [@ANCHOR: set_system_param]
        whitelist = self._get_param_whitelist()

        banned_substrings = [
            "secret", "key", "password", "token", "auth", "crypt", "cert",
        ]
        lower_key = key.lower()

        if key not in whitelist:
            if any(banned in lower_key for banned in banned_substrings):
                raise AccessError(
                    _(
                        "Security Alert: Parameter '%s' matches restricted cryptographic patterns and cannot be modified via Zero-Sudo."
                    ) % key
                )
            raise AccessError(
                _(
                    "Security Alert: Parameter '%s' is not in the Zero-Sudo PARAM_WHITELIST. You must explicitly register it in zero_sudo/models/security_utils.py."
                )
                % key
            )

        env_svc = self._get_service_env("zero_sudo.config_service_internal")
        env_svc["ir.config_parameter"].set_param(key, value)
        return True

    @api.model
    def _get_kv(self, key):
        env_svc = self._get_service_env("zero_sudo.odoo_facility_service_internal")
        record = env_svc['zero_sudo.kv'].search([('key', '=', key)], limit=1)
        return record.value if record else None

    @api.model
    def _set_kv(self, key, value):
        env_svc = self._get_service_env("zero_sudo.odoo_facility_service_internal")
        KV = env_svc['zero_sudo.kv']
        record = KV.search([('key', '=', key)], limit=1)
        if record:
            record.write({'value': value})
        else:
            KV.create({'key': key, 'value': value})

    @api.model
    def _update_python_venv(self):
        # [@ANCHOR: update_python_venv]
        # Verified by [@ANCHOR: test_update_python_venv]
        # Tests [@ANCHOR: story_venv_management]
        if not self.env.user.has_group("base.group_system"):
            raise AccessError(_("Only administrators can update the Python environment."))

        req_path = os.path.abspath(
            os.path.join(os.path.dirname(__file__), "..", "..", "requirements.txt")
        )
        if not os.path.exists(req_path):
            raise UserError(_("Requirements file not found at %s") % req_path)

        try:
            subprocess.run(
                [sys.executable, "-m", "pip", "install", "-r", req_path],
                capture_output=True,
                text=True,
                check=True,
                shell=False,
            )
            return True
        except subprocess.CalledProcessError as e:
            raise UserError(_("VENV update failed:\n%s") % e.stderr)

    @api.model
    def _get_crypto_secret(self):
        # [@ANCHOR: get_crypto_secret]
        """
        Retrieves the cryptographic secret without requiring .sudo() or database access.
        Checks environment variables first, then a local file, and falls back to config.
        """
        secret = os.environ.get("HAMS_CRYPTO_KEY")
        if not secret:
            try:
                with open("/var/lib/odoo/hams_crypto.secret", "r") as f:
                    secret = f.read().strip()
            except Exception:
                pass
        if not secret:
            secret = tools.config.get("admin_passwd", "default_insecure_secret")
        return secret
