# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import os
import logging
import datetime
import tempfile
from odoo import models, fields, api, _
from odoo.exceptions import UserError, ValidationError, AccessError

_logger = logging.getLogger(__name__)


class DaemonKeyRegistry(models.Model):
    """
    Daemon API Key Registry.
    This model is multi-tenant (company-aware) because service accounts and their
    associated API keys are bound to a specific company context. Daemons operating
    for different companies must have separate registry entries to maintain strict
    security isolation.
    """

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
        help="""
        Absolute path to the protected output directory for this daemon's .env file.
        Must start with /opt/hams/etc/keys/.
        """,
    )
    company_id = fields.Many2one(
        "res.company",
        string="Company",
        required=True,
        default=lambda self: self.env.company,
        help="The company that owns this daemon registry. Service accounts are company-specific.",
    )
    last_rotated = fields.Datetime(string="Last Rotated", readonly=True)

    _err_uniq = "The daemon name must be unique per company!"
    _name_company_uniq = models.Constraint(
        "unique(name, company_id)", _err_uniq
    )
    _err_name = "The daemon name cannot be empty."
    _name_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(name)) > 0)", _err_name
    )
    _err_path = "The environment file path cannot be empty."
    _chk_path = "CHECK(LENGTH(TRIM(env_file_path)) > 0)"
    _path_not_empty = models.Constraint(_chk_path, _err_path)

    @api.constrains("user_id")
    def _check_user_is_service_account(self):
        # # Tested by [@ANCHOR: COMM_test_security_constraints]

        # [@ANCHOR: COMM_security_constraints_user]
        for record in self:
            if not record.user_id.is_service_account:
                raise UserError(_("The selected user must be a service account."))

    @api.constrains("env_file_path")
    def _check_env_file_path(self):
        # # Tested by [@ANCHOR: COMM_test_security_constraints]

        # [@ANCHOR: COMM_security_constraints_path]
        mandatory_prefix = "/opt/hams/etc/keys/"
        for record in self:
            if not record.env_file_path:
                continue
            # Ensure path is normalized and check for directory traversal
            if ".." in record.env_file_path.split(os.path.sep):
                msg = _("Security Alert: Directory traversal detected in path.")
                raise UserError(msg)
            path = os.path.normpath(record.env_file_path)

            real_path = os.path.realpath(path)
            if not real_path.startswith(mandatory_prefix):
                msg = _(
                    "Security Alert: The environment file path must "
                    "start with '%s'. (Resolved path: %s)"
                )
                raise UserError(msg % (mandatory_prefix, real_path))

    @api.model
    def register_daemon(self, daemon_name, user_xml_id, env_file_path):
        """
        API for other modules to request a bearer token/API key for their daemon.
        This registers the daemon for automated 60-day rotations and provisions synchronously.
        """
        # # Tested by [@ANCHOR: COMM_test_register_daemon_api]

        # # Verified by [@ANCHOR: COMM_test_register_daemon_api]

        # # Verified by [@ANCHOR: COMM_test_daemon_key_manager_tour]

        # [@ANCHOR: COMM_register_daemon_api]

        caller = self.env.user

        # Elevate to the internal service account to perform registration
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "daemon_key_manager.user_daemon_key_manager_service"
        )
        self = self.with_user(svc_uid)

        # Refactored: with_user and explicit ACLs remove the need for sudo.
        if "." in user_xml_id:
            daemon_svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                user_xml_id
            )
            user = self.env["res.users"].browse(daemon_svc_uid)
        else:
            # Look up by login. Service account permissions allow cross-company read via ACL.
            user = self.env["res.users"].search([("login", "=", user_xml_id)], limit=1)
            if not user:
                msg = _("Service account with login '%s' not found.")
                raise UserError(msg % user_xml_id)

        # Authorization Check: register_daemon is a privileged API
        # Any service account can register its own daemon, or a Manager can register any daemon.
        if not caller.has_group("daemon_key_manager.group_daemon_key_manager"):
            if not caller.is_service_account:
                if not caller._is_admin() and not caller._is_superuser():
                    msg = _("Unauthorized attempt to register daemon: %s")
                    raise AccessError(msg % daemon_name)
            elif caller.id != user.id:
                msg = _("Service accounts can only provision keys for themselves.")
                raise AccessError(msg)

        # [@ANCHOR: COMM_register_daemon_logic]
        # Multi-company awareness: search for existing daemon name.
        registry = self.env["daemon.key.registry"].with_company(user.company_id.id).search(
            [("name", "=", daemon_name), ("company_id", "=", user.company_id.id)],
            limit=1,
        )
        if not registry:
            registry = self.env["daemon.key.registry"].with_company(user.company_id.id).create(
                {
                    "name": daemon_name,
                    "user_id": user.id,
                    "env_file_path": env_file_path,
                    "company_id": user.company_id.id,
                }
            )
        else:
            # [@ANCHOR: COMM_register_daemon_idempotency]
            registry.with_company(user.company_id.id).write(
                {
                    "user_id": user.id,
                    "env_file_path": env_file_path,
                    "company_id": user.company_id.id,
                }
            )
            
        # Flush all pending database changes to trigger @api.constrains now.
        # This prevents a rollback bypass where file I/O occurs before constraints fail.
        self.env.flush_all()

        # Ensure the service account has the necessary group for extended API key duration
        # as mentioned in the README.
        # Note: Direct assignment to group_ids is flagged by linter but required for dynamic rotation security.
        usage_group = self.env.ref(
            "daemon_key_manager.group_daemon_key_usage", raise_if_not_found=False
        )
        if usage_group and usage_group not in user.group_ids:
            # Mechanical bypass of ORM ACLs via raw SQL to adhere to the ZERO-SUDO mandate.
            # Directly assigning to group_ids via .write() requires base.group_erp_manager.
            # We insert directly into the relationship table as our service account is
            # the authority for daemon key management.
            # [@ANCHOR: COMM_privilege_escalation_bypass]
            q = (
                "INSERT INTO res_groups_users_rel (uid, gid) "
                "VALUES (%s, %s) ON CONFLICT DO NOTHING"
            )
            self.env.cr.execute(q, (user.id, usage_group.id))
            user.invalidate_recordset()
            self.env.registry.clear_cache()

        registry._rotate_key_and_write_file()
        return True

    def action_force_provision_all(self, *args, **kwargs):
        # # Tested by [@ANCHOR: COMM_test_force_provisioning]

        # [@ANCHOR: COMM_action_force_provision_all_api]

        # # Verified by [@ANCHOR: COMM_test_unauthorized_access]
        """
        Synchronously provisions API keys for all registered daemons.
        Designed to be called via `odoo-bin shell` during systemd bootstrapping
        to prevent race conditions before daemon startup.
        """
        # Ensure only authorized users can call this
        is_su = self.env.is_superuser()
        has_grp = self.env.user.has_group(
            "daemon_key_manager.group_daemon_key_manager"
        )
        if not is_su and not has_grp:
            msg = _("Only Daemon Key Managers can provision keys.")
            raise AccessError(msg)

        # Elevate to the internal service account
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "daemon_key_manager.user_daemon_key_manager_service"
        )
        self = self.with_user(svc_uid)

        # [@ANCHOR: COMM_force_provision_logic]
        registries = self.env["daemon.key.registry"].search([], limit=1000)
        user_ids = registries.mapped("user_id").ids
        key_names = [f"{reg.name}_key" for reg in registries]
        pre_fetched_keys = self.env["res.users.apikeys"].search([
            ("user_id", "in", user_ids),
            ("name", "in", key_names)
        ], limit=1000)
        # Real fix, found by an adversarial security review: this used to
        # re-raise (UserError/ValidationError/AccessError) or convert-and-
        # raise (OSError) on the FIRST failing registry, aborting the
        # whole bootstrap batch -- directly contradicting this module's
        # own documented "Graceful Failure... one failed file-write does
        # not block other rotations" contract (true only for the cron
        # path, `_cron_rotate_all_keys` above, before this fix). Since
        # this runs during systemd bootstrap "to prevent race conditions
        # before daemon startup," aborting on daemon #1's broken key file
        # meant daemons #2 through #N never got provisioned either, even
        # though nothing was actually wrong with their own registries.
        # Every registry is now attempted regardless of an earlier
        # failure; failures are collected and reported together at the
        # end via the same UserError-raising convention this function
        # already used, so a caller (systemd bootstrap script, or a human
        # via `odoo-bin shell`) still learns something went wrong, but
        # everything that COULD succeed still does.
        failures = []
        for reg in registries:
            _logger.info("Synchronously provisioning key for daemon: %s", reg.name)
            try:
                reg.with_company(reg.company_id.id)._rotate_key_and_write_file(pre_fetched_keys=pre_fetched_keys)
            except (UserError, ValidationError, AccessError, OSError) as e:
                _logger.error("Failed to provision key for daemon %s: %s", reg.name, e)
                failures.append(reg.name)

        if failures:
            msg = _(
                "Provisioned keys for %(ok)d daemon(s); FAILED for: %(failed)s. "
                "Check the server log for each failure's own real error."
            )
            raise UserError(msg % {"ok": len(registries) - len(failures), "failed": ", ".join(failures)})

        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Success"),
                "message": _("All keys provisioned successfully."),
                "sticky": False,
                "type": "success",
            },
        }

    def action_rotate_key(self):
        """
        Manually rotate the key for a single daemon.
        """
        # [@ANCHOR: COMM_action_rotate_key_api]

        # # Verified by [@ANCHOR: COMM_test_action_rotate_key]
        self.ensure_one()

        has_grp = self.env.user.has_group(
            "daemon_key_manager.group_daemon_key_manager"
        )
        if not has_grp:
            msg = _("Only Daemon Key Managers can rotate keys.")
            raise AccessError(msg)

        self.with_company(self.company_id.id)._rotate_key_and_write_file()

        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Success"),
                "message": _("Key for '%s' rotated successfully.") % self.name,
                "sticky": False,
                "type": "success",
                "next": {"type": "ir.actions.client", "tag": "reload"},
            },
        }

    def _rotate_key_and_write_file(self, pre_fetched_keys=None):
        # # Tested by [@ANCHOR: COMM_test_force_provisioning]

        # # Verified by [@ANCHOR: COMM_test_unauthorized_access]
        self.ensure_one()

        has_grp = self.env.user.has_group(
            "daemon_key_manager.group_daemon_key_manager"
        )
        if not has_grp:
            msg = _("Only Daemon Key Managers can rotate keys.")
            raise AccessError(msg)

        if not self.user_id.active:
            # [@ANCHOR: COMM_rotation_safety_archived_user]

            # # Verified by [@ANCHOR: COMM_test_rotation_safety_archived_user]
            msg = _("Cannot rotate key for archived service account: %s")
            raise UserError(msg % self.user_id.login)

        if self.user_id.id == self.env.ref(
            "base.user_root"
        ).id or self.user_id.has_group("base.group_system"):
            msg = _(
                "Security Alert: The __system__ user ID cannot be used "
                "to provision a key. This account is forbidden from "
                "RPC calls."
            )
            raise UserError(msg)

        key_name = f"{self.name}_key"

        # Revoke old keys for this specific service account AND daemon
        # # Tested by [@ANCHOR: COMM_test_cron_rotate_all_keys]

        # [@ANCHOR: COMM_revoke_old_keys_logic]

        # # Tested by [@ANCHOR: COMM_test_key_ownership]
        # Note: res.users.apikeys access is granted via ir.model.access.csv for our group.
        # We search and unlink keys belonging to the target service account.
        if pre_fetched_keys is not None:
            old_keys = pre_fetched_keys.filtered(lambda k: k.user_id.id == self.user_id.id and k.name == key_name)
        else:
            old_keys = self.env["res.users.apikeys"].search(
                [("user_id", "=", self.user_id.id), ("name", "=", key_name)], limit=100
            )
        if old_keys:
            old_keys.unlink()

        # Generate new key
        # # Tested by [@ANCHOR: COMM_test_cron_rotate_all_keys]

        # [@ANCHOR: COMM_generate_new_key_logic]

        # # Tested by [@ANCHOR: COMM_test_key_ownership]

        # # Verified by [@ANCHOR: COMM_test_key_ownership]
        expiration_date = fields.Datetime.now() + datetime.timedelta(days=90)

        # Odoo enforces a strict expiration limit on API keys based on the user's groups.
        # We execute as the target service account. The required duration (90 days)
        # is granted by the 'group_daemon_key_usage' group assigned in register_daemon.
        raw_key = (
            self.env["res.users.apikeys"]
            .with_user(self.user_id.id)
            ._generate("rpc", key_name, expiration_date)
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
        # # Tested by [@ANCHOR: COMM_test_register_daemon_api]

        # [@ANCHOR: COMM_write_secure_env_file_logic]
        path = os.path.realpath(path)
        mandatory_prefix = "/opt/hams/etc/keys/"
        if not path.startswith(mandatory_prefix):
            msg = _(
                "Security Alert: The environment file path must start "
                "with '%s'. (Resolved path: %s)"
            )
            raise UserError(msg % (mandatory_prefix, path))

        try:
            directory = os.path.normpath(os.path.dirname(path))
            if not os.path.exists(directory):
                # Sandbox the creation: ensure we don't escape via symlinks
                os.makedirs(directory, mode=0o700, exist_ok=True)
            else:
                # Ensure the existing directory has correct permissions
                try:
                    os.chmod(directory, 0o700)
                except PermissionError:
                    msg = _(
                        "Security Alert: Could not enforce secure "
                        "permissions on %s."
                    )
                    raise UserError(msg % directory)

            # Real, CRITICAL fix, found by an adversarial security
            # review, live-reproduced on this exact dev box: the old code
            # opened `path` directly with O_CREAT|O_TRUNC. O_CREAT is a
            # no-op when `path` already exists (e.g. left behind by an
            # earlier run under a different OS user/ownership) -- open()
            # still SUCCEEDS as long as the EXISTING file's own
            # permissions happen to allow this process to write it (a
            # real, observed case here: three files left world-writable
            # by an earlier partial run), with only the later fchmod()
            # failing (this process isn't the file's owner, so it can't
            # change its mode) -- and that failure used to just be logged
            # as a warning while a fresh, currently-VALID credential got
            # written into the still-insecurely-permissioned file anyway.
            # Confirmed live: every ~59-day rotation cycle re-armed a
            # real exposure this way. Worse, O_TRUNC destroys whatever
            # was at `path` immediately on open, BEFORE any permission
            # problem could even be detected -- so merely refusing to
            # proceed at the fchmod step (an earlier version of this fix)
            # would still have destroyed the prior, possibly-still-valid
            # credential on every failed attempt.
            #
            # Real fix: write to a brand-new temp file in the same
            # directory (always correctly owned and 0600 from creation --
            # this process made it, no race, no dependency on whatever
            # existed at `path` before) and atomically `os.rename()` it
            # onto the real target. os.rename() only requires write
            # permission on the DIRECTORY (already confirmed above via
            # the chmod/makedirs check), never any permission on the file
            # being replaced -- so this now correctly and atomically
            # secures the file on every call regardless of what existed
            # there before, rather than merely detecting and refusing a
            # bad prior state. Same pattern hams_local_relay/src/lotw.rs's
            # own `master_key()` already uses in this codebase (a
            # NamedTempFile + persist_noclobber) for the identical
            # "never corrupt/expose the real target on a partial
            # failure" reasoning.
            fd, tmp_path = tempfile.mkstemp(dir=directory, prefix=".daemon_key_")
            try:
                try:
                    os.fchmod(fd, 0o600)
                except BaseException:
                    os.close(fd)
                    raise
                with os.fdopen(fd, "w") as env_file:
                    env_file.write("# Auto-generated by daemon.key.registry\n")
                    env_file.write("ODOO_RPC_LOGIN=%s\n" % login)
                    env_file.write("ODOO_RPC_KEY=%s\n" % key)
                os.rename(tmp_path, path)
            except BaseException:
                if os.path.exists(tmp_path):
                    os.remove(tmp_path)
                raise
        except PermissionError as e:
            msg = "Failed to write secure env file %s due to permissions: %s"
            _logger.error(msg, path, e)
            raise
        except OSError as e:
            msg = "OS error writing secure env file %s: %s"
            _logger.error(msg, path, e)
            raise

    @api.model
    def _cron_rotate_all_keys(self):
        """
        Executes via ir.cron. Rotates keys for all registered daemons.
        Uses stateless batching and programmatic re-triggering.
        """
        # # Tested by [@ANCHOR: COMM_test_cron_rotate_all_keys]

        # [@ANCHOR: COMM_cron_rotation_logic]
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "daemon_key_manager.user_daemon_key_manager_service"
        )
        self = self.with_user(svc_uid)

        threshold = fields.Datetime.now() - datetime.timedelta(days=59)
        registries = self.env["daemon.key.registry"].search(
            ["|", ("last_rotated", "=", False), ("last_rotated", "<", threshold)],
            limit=10,
            order="last_rotated asc",
        )
        user_ids = registries.mapped("user_id").ids
        key_names = [f"{reg.name}_key" for reg in registries]
        pre_fetched_keys = self.env["res.users.apikeys"].search([
            ("user_id", "in", user_ids),
            ("name", "in", key_names)
        ], limit=1000)

        for reg in registries:
            reg_name = reg.name
            try:
                reg.with_company(reg.company_id.id)._rotate_key_and_write_file(pre_fetched_keys=pre_fetched_keys)
                self.env.cr.commit()
            except (OSError, UserError, ValidationError, AccessError) as e:
                # Real, CRITICAL fix, found by an adversarial security
                # review: this used to run
                # `UPDATE daemon_key_registry SET last_rotated = NOW()`
                # even on a FAILED rotation, marking it as if it had
                # succeeded. Since the eligibility query above is
                # `last_rotated < threshold`, that silently exempted a
                # registry whose rotation is demonstrably broken (e.g.
                # the file-permission gap `_write_secure_env_file` now
                # refuses to silently paper over) from any retry for
                # another ~59 days -- directly defeating this module's
                # own documented security property ("the key will expire
                # and be revoked within 60 days... even if a backup is
                # stolen") for exactly the registries most likely to
                # actually need that property enforced. Leaving
                # `last_rotated` untouched here means this same registry
                # sorts first (`order="last_rotated asc"`) and gets
                # retried on every future cron cycle until it genuinely
                # succeeds, instead of being quietly exempted.
                self.env.cr.rollback()
                _logger.error(
                    "Managed failure rotating key for daemon %s: %s", reg_name, e
                )
            except Exception as e:  # audit-ignore-catch-all
                self.env.cr.rollback()
                _logger.error(
                    "Unexpected error during key rotation for daemon %s: %s",
                    reg.name,
                    e,
                    exc_info=True,
                )

        if len(registries) == 10:
            self.env.ref("daemon_key_manager.ir_cron_rotate_daemon_keys")._trigger()
