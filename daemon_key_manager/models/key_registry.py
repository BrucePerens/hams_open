# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import os
import logging
import datetime
import tempfile
from odoo import models, fields, api, _
from odoo.exceptions import UserError, ValidationError, AccessError
from odoo.addons.base.models.res_users import INDEX_SIZE, KEY_CRYPT_CONTEXT

_logger = logging.getLogger(__name__)

# A key is rotated once it is older than this. Shared by the Odoo-side
# cron and the remote self-rotation path so both follow one schedule.
ROTATION_AGE_DAYS = 59
# Lifetime of every key this module mints.
KEY_LIFETIME_DAYS = 90


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
    remote_self_rotation = fields.Boolean(
        string="Remote Self-Rotation",
        default=False,
        help="""
        Set for a daemon that runs on a different machine than Odoo. Odoo
        cannot write that machine's key file, so the Odoo-side rotation
        paths (the daily cron, Force Provision All, Rotate Key) skip this
        registry, and the daemon rotates its own key through
        rotate_own_key() over JSON-2 instead.
        """,
    )
    # Id of the res.users.apikeys row rotate_own_key() issued and the remote
    # daemon has not yet proved it holds. 0 when no rotation is in flight.
    pending_key_id = fields.Integer(readonly=True, copy=False)

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

    # Adversarial security review, 2026-09-03: register_daemon()'s own
    # authorization check only verified the CALLER is the target service
    # account being registered -- it never verified env_file_path itself
    # was unique. A caller could register_daemon() under a brand-new,
    # never-used daemon_name while setting env_file_path to point at a
    # DIFFERENT, already-registered daemon's real credential file --
    # register_daemon() would create a new registry row and immediately
    # call _rotate_key_and_write_file(), overwriting that other daemon's
    # real .env with the calling account's own (lower-privileged)
    # credentials. Real path is a real filesystem location -- inherently
    # global, not per-company, unlike `name` above -- so this is a plain
    # single-column uniqueness constraint, not company-scoped.
    _err_path_uniq = "This environment file path is already registered to another daemon."
    _path_uniq = models.Constraint("unique(env_file_path)", _err_path_uniq)

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

        # _rotate_key_and_write_file() below already calls _ensure_usage_group() itself, after
        # its own active-account/__system__/group_system safety checks -- an earlier version of
        # this method called it directly here too, before those checks run for this call path.
        # Harmless today (nothing commits the transaction in between, so a raised UserError rolls
        # back the raw-SQL group grant along with everything else), but redundant, and a future
        # refactor that adds an intermediate commit could silently turn this into a real
        # privilege-grant-survives-rejection gap. Rely on the single, correctly-ordered call
        # inside _rotate_key_and_write_file() instead.
        registry._rotate_key_and_write_file()
        return True

    def _ensure_usage_group(self, user):
        """
        Grants `group_daemon_key_usage` (the 90-day API-key-duration group) to `user`
        if it doesn't already have it.

        Real bug found 2026-09-28/29 while chasing a live production incident
        (`backup_worker` unable to rotate its own key, `ValidationError: You cannot
        exceed 1.0 days` from `res.users._check_expiration_date()`, despite this exact
        group having been granted once already): every daemon service account this
        codebase defines declares its OWN `group_ids` as a static `(6, 0, [...])` eval
        in a `noupdate="0"` XML record -- intentionally, so upgrading a daemon module
        can revoke a group it no longer wants the account to hold (see
        `backup_management/security/security.xml`'s own 2026-09-14 comment on exactly
        this mechanism). But `group_daemon_key_usage` was never one of the groups any
        of those per-module `(6, 0, [...])` lists statically declared -- it was only
        ever granted dynamically, here, via the raw-SQL insert below, the first time
        `register_daemon()` ran for that account. A `(6, 0, [...])` eval *replaces* the
        full group set, so the next ordinary module upgrade after that first
        registration silently wiped this dynamically-granted membership back out
        again, with no error anywhere -- confirmed against every `is_service_account`
        XML record in this codebase as of this date: not one statically includes this
        group. `register_daemon()` alone re-granting it (the only call site before this
        fix) doesn't help, since `register_daemon()` isn't what runs on a later,
        ordinary key rotation -- `action_rotate_key()` and the nightly
        `cron_rotate_all_keys()` are, and neither of them re-checked this. Calling this
        from `_rotate_key_and_write_file()` itself (this method's own real caller,
        reached by all three of `register_daemon()`, `action_rotate_key()`, and the
        cron) makes every rotation path self-healing regardless of what an
        intervening module upgrade did to `group_ids`, without editing every
        individual daemon module's own security.xml (which would only fix today's
        known daemons, not tomorrow's).
        """
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
        # A remote self-rotating registry is left alone: re-provisioning it here
        # would revoke the key the remote daemon holds (see
        # COMM_remote_self_rotation_excluded_from_local_rotation). This runs on
        # every hams.daemon.keys.service start, so it is not a rare path.
        registries = self.env["daemon.key.registry"].search(
            [("remote_self_rotation", "=", False)], limit=1000
        )
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
                # A savepoint per daemon: a failure part-way through one registry undoes that registry's own
                # database changes (the old key is restored) without touching the daemons that already succeeded.
                with self.env.cr.savepoint():
                    reg.with_company(reg.company_id.id)._rotate_key_and_write_file(pre_fetched_keys=pre_fetched_keys)
            except (UserError, ValidationError, AccessError, OSError) as e:
                _logger.error("Failed to provision key for daemon %s: %s", reg.name, e)
                failures.append(reg.name)

        if failures:
            # Return, do not raise. Raising here used to roll back the whole transaction, including the rotations
            # that had succeeded -- but their new keys were already written to their key files, so the files and the
            # database disagreed and those daemons were refused (found live on hams1, 2026-09-23: the event and
            # AI-triage services stayed unauthorised for days after one unrelated daemon failed). A failure is
            # reported in the result instead, and the caller commits what succeeded. The systemd bootstrap script
            # turns a non-success result into a failed unit.
            msg = _(
                "Provisioned keys for %(ok)d daemon(s); FAILED for: %(failed)s. "
                "Check the server log for each failure's own real error."
            )
            return {
                "type": "ir.actions.client",
                "tag": "display_notification",
                "params": {
                    "title": _("Some keys were not provisioned"),
                    "message": msg % {"ok": len(registries) - len(failures), "failed": ", ".join(failures)},
                    "sticky": True,
                    "type": "danger",
                },
            }

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

    @api.model
    def rotate_own_key(self, daemon_name, current_key):
        """
        Remote self-rotation, called over JSON-2 by a daemon that runs on another
        machine (Remote Self-Rotation set on its registry), authenticated with its
        current key. Odoo cannot write that machine's key file, so the daemon
        fetches its new key itself, in two calls, and no key is revoked until the
        daemon has proved it holds the new one:

        1. Called with the registry's active key. When the key is older than
           ROTATION_AGE_DAYS, a new key is minted and returned and the active key
           is left valid. Otherwise nothing changes.
           Returns {"status": "issued", "login": ..., "key": ...} or
           {"status": "not_due", "next_rotation": ...}.
        2. The daemon writes the new key to its key file and calls again with it.
           The old key is revoked, the new key becomes the active one, it is
           written to env_file_path on this machine as well, and last_rotated is
           set. Returns {"status": "confirmed"}.

        A lost response at any point leaves the daemon holding a valid key: a lost
        step-1 reply means the daemon still has the old key, and its next step 1
        replaces the unused new key; a lost step-2 reply means the daemon has the
        new key on disk, and its next call (with that key) completes step 2.

        `current_key` must be a valid key of this registry (not merely any key of
        the same service account), so another daemon sharing the account cannot
        rotate this registry. Rotation is only ever issued once the key is due,
        which bounds how often a stolen key could renew itself, and every issue and
        confirmation is logged at WARNING.
        """
        # [@ANCHOR: COMM_rotate_own_key_api]

        # # Verified by [@ANCHOR: COMM_test_rotate_own_key_two_phase]
        caller = self.env.user
        refused = _("Remote self-rotation is not available for daemon '%s'.")
        if not isinstance(daemon_name, str) or not isinstance(current_key, str):
            raise AccessError(refused % daemon_name)
        if len(current_key) <= INDEX_SIZE:
            raise AccessError(refused % daemon_name)

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "daemon_key_manager.user_daemon_key_manager_service"
        )
        registry_model = (
            self.with_user(svc_uid)
            .with_company(caller.company_id.id)
            .env["daemon.key.registry"]
        )
        registry = registry_model.search(
            [
                ("name", "=", daemon_name),
                ("user_id", "=", caller.id),
                ("remote_self_rotation", "=", True),
            ],
            limit=1,
        )
        # One message for "no such registry", "not yours" and "not remote", so
        # the refusal does not reveal which registries exist.
        if not registry:
            raise AccessError(refused % daemon_name)
        registry._assert_account_may_hold_key()

        presented_key_id = registry._own_key_row_id(current_key)
        if not presented_key_id:
            raise AccessError(refused % daemon_name)

        if presented_key_id == registry.pending_key_id:
            return registry._confirm_remote_rotation(current_key)
        return registry._issue_remote_rotation()

    def _own_key_row_id(self, raw_key):
        """
        Id of the live res.users.apikeys row of this registry that `raw_key` is, or
        None. The key's hash and index are not ORM fields, so they are read with SQL.
        """
        self.ensure_one()
        self.env.cr.execute(
            "SELECT id, key FROM res_users_apikeys"
            " WHERE user_id = %s AND name = %s AND index = %s"
            " AND (expiration_date IS NULL"
            " OR expiration_date >= now() at time zone 'utc')",
            (self.user_id.id, f"{self.name}_key", raw_key[:INDEX_SIZE]),
        )
        for row_id, hashed in self.env.cr.fetchall():
            if KEY_CRYPT_CONTEXT.verify(raw_key, hashed):
                return row_id
        return None

    def _issue_remote_rotation(self):
        """Step 1 of rotate_own_key(): mint the new key, revoke nothing yet."""
        self.ensure_one()
        # [@ANCHOR: COMM_rotate_own_key_issue]
        if self.last_rotated:
            age = datetime.timedelta(days=ROTATION_AGE_DAYS)
            due_at = self.last_rotated + age
            if fields.Datetime.now() < due_at:
                return {
                    "status": "not_due",
                    "next_rotation": fields.Datetime.to_string(due_at),
                }

        apikeys = self.env["res.users.apikeys"]
        if self.pending_key_id:
            # An earlier step 1 whose reply never reached the daemon (or whose
            # key it never confirmed): that key is unused, so replace it.
            stale = apikeys.search([("id", "=", self.pending_key_id)], limit=1)
            stale.unlink()

        self._ensure_usage_group(self.user_id)
        raw_key = self._mint_key(f"{self.name}_key")
        self.pending_key_id = self._own_key_row_id(raw_key)
        _logger.warning(
            "Remote self-rotation: issued a new key for daemon %s (account %s); "
            "the old key stays valid until the daemon confirms the new one.",
            self.name,
            self.user_id.login,
        )
        return {"status": "issued", "login": self.user_id.login, "key": raw_key}

    def _confirm_remote_rotation(self, new_key):
        """Step 2 of rotate_own_key(): the daemon holds the new key; revoke the rest."""
        self.ensure_one()
        # [@ANCHOR: COMM_rotate_own_key_confirm]
        superseded = self.env["res.users.apikeys"].search(
            [
                ("user_id", "=", self.user_id.id),
                ("name", "=", f"{self.name}_key"),
                ("id", "!=", self.pending_key_id),
            ],
            limit=100,
        )
        superseded.unlink()
        # Keep this machine's copy current too, so env_file_path always holds the
        # live key (it is where an operator re-copies the key from if the remote
        # machine loses its file).
        self._write_secure_env_file(self.env_file_path, self.user_id.login, new_key)
        self.pending_key_id = 0
        self.last_rotated = fields.Datetime.now()
        _logger.warning(
            "Remote self-rotation: daemon %s (account %s) confirmed its new key; "
            "%d old key(s) revoked.",
            self.name,
            self.user_id.login,
            len(superseded),
        )
        return {"status": "confirmed"}

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

        if self.remote_self_rotation:
            # [@ANCHOR: COMM_remote_self_rotation_excluded_from_local_rotation]
            # This path revokes the old key and writes the new one to a file on
            # THIS machine. For a daemon on another machine that strands it on
            # a revoked key: it cannot see the file. It rotates through
            # rotate_own_key() instead.
            msg = _(
                "Daemon '%s' rotates its own key remotely (Remote Self-Rotation). "
                "Rotating it here would revoke the key the remote daemon holds. "
                "Clear Remote Self-Rotation first if you really mean to."
            )
            raise UserError(msg % self.name)

        self._assert_account_may_hold_key()

        # Self-healing re-grant: see _ensure_usage_group()'s own docstring for why this
        # can't be assumed to still hold just because register_daemon() granted it once.
        self._ensure_usage_group(self.user_id)

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
        raw_key = self._mint_key(key_name)

        # Write to secure file
        self._write_secure_env_file(self.env_file_path, self.user_id.login, raw_key)
        self.last_rotated = fields.Datetime.now()
        _logger.info(
            "Successfully rotated and exported API key for daemon: %s", self.name
        )

    def _mint_key(self, key_name):
        """Generates a KEY_LIFETIME_DAYS key named `key_name` for this registry's account."""
        self.ensure_one()
        expiration_date = fields.Datetime.now() + datetime.timedelta(days=KEY_LIFETIME_DAYS)

        # Odoo enforces a strict expiration limit on API keys based on the user's groups.
        # We execute as the target service account. The required duration (90 days)
        # is granted by the 'group_daemon_key_usage' group assigned in register_daemon.
        return (
            self.env["res.users.apikeys"]
            .with_user(self.user_id.id)
            ._generate("rpc", key_name, expiration_date)
        )

    def _assert_account_may_hold_key(self):
        """Refuses archived accounts and __system__/group_system accounts."""
        self.ensure_one()
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
                except BaseException:  # audit-ignore-catch-all
                    # Cleanup-then-reraise: must catch every kind of interruption
                    # (including KeyboardInterrupt/SystemExit) to avoid leaking this
                    # fd, and always re-raises unconditionally, so nothing is
                    # silently swallowed.
                    os.close(fd)
                    raise
                with os.fdopen(fd, "w") as env_file:
                    env_file.write("# Auto-generated by daemon.key.registry\n")
                    env_file.write("ODOO_RPC_LOGIN=%s\n" % login)
                    env_file.write("ODOO_RPC_KEY=%s\n" % key)
                os.rename(tmp_path, path)
            except BaseException:  # audit-ignore-catch-all
                # Same cleanup-then-reraise idiom as above, for the temp file itself.
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

        threshold = fields.Datetime.now() - datetime.timedelta(days=ROTATION_AGE_DAYS)
        registries = self.env["daemon.key.registry"].search(
            [
                ("remote_self_rotation", "=", False),
                "|",
                ("last_rotated", "=", False),
                ("last_rotated", "<", threshold),
            ],
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
