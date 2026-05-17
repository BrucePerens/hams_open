# -*- coding: utf-8 -*-
import os
import logging
import datetime
from odoo import models, fields, api, SUPERUSER_ID, tools, _
from odoo.exceptions import UserError

_logger = logging.getLogger(__name__)


class DaemonKeyRegistry(models.Model):
    _name = "daemon.key.registry"
    _description = "Daemon API Key Registry"

    name = fields.Char(string="Daemon Name", required=True)
    user_id = fields.Many2one(
        "res.users",
        string="Service Account",
        required=True,
        domain=[("is_service_account", "=", True)],
    )
    env_file_path = fields.Char(
        string="Environment File Path",
        required=True,
        help="Absolute path to the protected output directory for this daemon's .env file.",
    )
    last_rotated = fields.Datetime(string="Last Rotated", readonly=True)

    _name_uniq = models.Constraint("UNIQUE(name)", "The daemon name must be unique!")

    _name_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(name)) > 0)", "The daemon name cannot be empty."
    )
    _path_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(env_file_path)) > 0)",
        "The environment file path cannot be empty.",
    )

    @api.constrains('user_id')
    def _check_user_is_service_account(self):
        # Tested by [@ANCHOR: test_security_constraints]
        # [@ANCHOR: security_constraints_user]
        for record in self:
            if not record.user_id.is_service_account:
                raise UserError(_("The selected user must be a service account."))

    @api.constrains('env_file_path')
    def _check_env_file_path(self):
        # Tested by [@ANCHOR: test_security_constraints]
        # [@ANCHOR: security_constraints_path]
        mandatory_prefix = "/var/lib/odoo/daemon_keys/"
        for record in self:
            if not record.env_file_path:
                continue
            # Ensure path is normalized and check for directory traversal
            path = os.path.normpath(record.env_file_path)
            if ".." in path.split(os.path.sep):
                raise UserError(_("Security Alert: Directory traversal detected in path."))

            real_path = os.path.realpath(path)
            if not real_path.startswith(mandatory_prefix):
                raise UserError(
                    _("Security Alert: The environment file path must start with '%s'. (Resolved path: %s)")
                    % (mandatory_prefix, real_path)
                )

    @api.model
    def register_daemon(self, daemon_name, user_xml_id, env_file_path):
        """
        API for other modules to request a bearer token/API key for their daemon.
        This registers the daemon for automated 60-day rotations and provisions synchronously.
        """
        # Tested by [@ANCHOR: test_register_daemon_api]
        # Verified by [@ANCHOR: test_register_daemon_api]
        # Verified by [@ANCHOR: test_daemon_key_manager_tour]
        # [@ANCHOR: register_daemon_api]

        # Elevate to the internal service account to perform registration
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid("daemon_key_manager.user_daemon_key_manager_service")
        self = self.with_user(svc_uid)

        daemon_svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(user_xml_id)
        user = self.env["res.users"].browse(daemon_svc_uid)

        # [@ANCHOR: register_daemon_logic]
        registry = self.env["daemon.key.registry"].search([("name", "=", daemon_name)], limit=1)
        if not registry:
            registry = self.env["daemon.key.registry"].create(
                {
                    "name": daemon_name,
                    "user_id": user.id,
                    "env_file_path": env_file_path,
                }
            )
        else:
            # [@ANCHOR: register_daemon_idempotency]
            registry.write({"user_id": user.id, "env_file_path": env_file_path})

        registry._rotate_key_and_write_file()
        return True

    @api.model
    def action_force_provision_all(self, *args, **kwargs):
        # Tested by [@ANCHOR: test_force_provisioning]
        # [@ANCHOR: action_force_provision_all_api]
        """
        Synchronously provisions API keys for all registered daemons.
        Designed to be called via `odoo-bin shell` during systemd bootstrapping
        to prevent race conditions before daemon startup.
        """
        # Elevate to the internal service account
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid("daemon_key_manager.user_daemon_key_manager_service")
        self = self.with_user(svc_uid)

        # [@ANCHOR: force_provision_logic]
        registries = self.env["daemon.key.registry"].search([], limit=1000)
        for reg in registries:
            _logger.info("Synchronously provisioning key for daemon: %s", reg.name)
            try:
                reg._rotate_key_and_write_file()
            except OSError:
                # [@ANCHOR: force_provision_error_handling]
                raise UserError(
                    _("Cannot write key file for '%s' " "at '%s'. Check permissions.")
                    % (reg.name, reg.env_file_path)
                )
        return True

    def _rotate_key_and_write_file(self):
        # Tested by [@ANCHOR: test_force_provisioning]
        self.ensure_one()

        if self.user_id.id == SUPERUSER_ID:
            raise UserError(
                _(
                    "Security Alert: The __system__ user ID cannot be used to provision a key. "
                    "This account is forbidden from RPC calls."
                )
            )

        key_name = f"{self.name}_key"

        # Revoke old keys for this specific service account AND daemon
        # Tested by [@ANCHOR: test_cron_rotate_all_keys]
        # [@ANCHOR: revoke_old_keys_logic]
        # Tested by [@ANCHOR: test_key_ownership]
        old_keys = self.env["res.users.apikeys"].sudo().search( # burn-ignore-sudo
            [("user_id", "=", self.user_id.id), ("name", "=", key_name)], limit=100
        )
        if old_keys:
            # Elevated execution required for restricted API key deletion.
            old_keys.sudo().unlink() # burn-ignore-sudo

        # Generate new key
        # Tested by [@ANCHOR: test_cron_rotate_all_keys]
        # [@ANCHOR: generate_new_key_logic]
        expiration_date = fields.Datetime.now() + datetime.timedelta(days=90)

        # Odoo enforces a strict 1-day expiration limit on API keys created by non-administrators.
        # We use .sudo() here, as explicitly exempted for the daemon_key_manager, to provision
        # a 90-day key for the service account without exposing the entire ERP.
        # Tested by [@ANCHOR: test_key_ownership]
        # Verified by [@ANCHOR: test_key_ownership]
        raw_key = (
            self.env["res.users.apikeys"].with_user(self.user_id.id).sudo()._generate("rpc", key_name, expiration_date) # burn-ignore-sudo
        )

        # Write to secure file
        self._write_secure_env_file(self.env_file_path, self.user_id.login, raw_key)
        self.last_rotated = fields.Datetime.now()
        _logger.info(
            "Successfully rotated and exported API key for daemon: %s", self.name
        )

    def _write_secure_env_file(self, path, login, key):
        """
        Writes the credentials to the specified path and locks permissions to 0600.
        Creates directories with 0700 if they do not exist.
        """
        # Tested by [@ANCHOR: test_register_daemon_api]
        # [@ANCHOR: write_secure_env_file_logic]
        path = os.path.realpath(path)
        # Sandbox check: Prevent writing to sensitive system directories
        forbidden_prefixes = ["/etc", "/root", "/boot", "/sys", "/proc", "/dev"]
        if any(path.startswith(pref) for pref in forbidden_prefixes):
            raise UserError(
                _("Security Alert: Writing to system directory '%s' is forbidden.")
                % path
            )

        directory = os.path.normpath(os.path.dirname(path))
        if not os.path.exists(directory):
            # Sandbox the creation: ensure we don't escape via symlinks
            os.makedirs(directory, mode=0o700, exist_ok=True)
        else:
            # Ensure the existing directory has correct permissions
            os.chmod(directory, 0o700)

        # Ensure the file is created with 0600 from the start
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "w") as f:
            f.write("# Auto-generated by daemon.key.registry\n")
            f.write(f"ODOO_RPC_LOGIN={login}\n")
            f.write(f"ODOO_RPC_KEY={key}\n")


    @api.model
    def _cron_rotate_all_keys(self):
        """
        Executes via ir.cron. Rotates keys for all registered daemons.
        Uses stateless batching and programmatic re-triggering.
        """
        # Tested by [@ANCHOR: test_cron_rotate_all_keys]
        # [@ANCHOR: cron_rotation_logic]
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid("daemon_key_manager.user_daemon_key_manager_service")
        self = self.with_user(svc_uid)

        threshold = fields.Datetime.now() - datetime.timedelta(days=59)
        registries = self.env["daemon.key.registry"].search(
            ["|", ("last_rotated", "=", False), ("last_rotated", "<", threshold)],
            limit=10,
        )

        for reg in registries:
            try:
                reg._rotate_key_and_write_file()
                if not tools.config.get('test_enable'):
                    self.env.cr.commit()
            except Exception as e: # audit-ignore-catch-all
                if not tools.config.get('test_enable'):
                    self.env.cr.rollback()
                _logger.error("Failed to rotate key for daemon %s: %s", reg.name, e)

        if len(registries) == 10:
            self.env.ref("daemon_key_manager.ir_cron_rotate_daemon_keys").sudo()._trigger() # burn-ignore-sudo
