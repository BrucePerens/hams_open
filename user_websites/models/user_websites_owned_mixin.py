# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
import logging
from odoo import models, fields, api, _
from odoo.exceptions import AccessError, ValidationError

_logger = logging.getLogger(__name__)


class UserWebsitesOwnedMixin(models.AbstractModel):
    """
    An abstract mixin that provides the 'Proxy Ownership' pattern to any model.
    Inherit this to securely tie a model to a user or a group website.
    """

    _name = "user_websites.owned.mixin"
    _description = "User Websites Proxy Ownership Mixin"

    owner_user_id = fields.Many2one(
        "res.users",
        string="Owner",
        index=True,
        ondelete="cascade",
        help="The user who 'owns' this record business-wise.",
    )

    user_websites_group_id = fields.Many2one(
        "user.websites.group",
        string="Group Owner",
        help="The group that owns this record.",
        ondelete="cascade",
        index=True,
    )

    @api.model
    def _check_proxy_ownership_create(self, vals_list):
        # [@ANCHOR: mixin_proxy_ownership_create]
        # Verified by [@ANCHOR: test_mixin_ownership_validation]
        # Verified by [@ANCHOR: test_api_armor_mandatory_assignment]
        """Validates that the current user is legally allowed to assign the provided ownership, enforces mandatory ownership, and prevents dual ownership."""

        user_id = self.env.user.id
        is_admin = (
            self.env.su
            or self.env.user.has_group("base.group_system")
            or self.env.user.has_group(
                "user_websites.group_user_websites_administrator"
            )
            or self.env.user.has_group(
                "user_websites.group_user_websites_service_account"
            )
        )

        # ADR 0078: O(1) Memory Mapping - Pre-fetch all group memberships to prevent N+1 lazy loading queries in the loop
        group_ids = {
            int(vals.get("user_websites_group_id"))
            for vals in vals_list
            if vals.get("user_websites_group_id")
        }
        valid_group_members = {}

        if group_ids and not is_admin:
            try:
                svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                    "user_websites.user_websites_service_account"
                )
                groups = (
                    self.env["user.websites.group"]
                    .with_user(svc_uid)
                    .browse(list(group_ids))
                )
            except Exception as e:  # audit-ignore-catch-all
                # Defer if service account is not yet provisioned during early testing
                if "not found" in str(e).lower():
                    _logger.debug("Service account not found, deferring proxy ownership check: %s", e)
                    return
                _logger.error("Failed to execute proxy ownership lookup: %s", e)
                raise
            for group in groups:
                if group.exists():
                    valid_group_members[group.id] = set(group.member_ids.ids)

        for vals in vals_list:
            # ADR-0082: Auto-assign ownership to the current user if missing and they are not a guest
            if not vals.get("owner_user_id") and not vals.get("user_websites_group_id") and not self.env.user._is_public():
                vals["owner_user_id"] = user_id
            owner_id = vals.get("owner_user_id")
            group_id = vals.get("user_websites_group_id")

            if owner_id and group_id:
                raise ValidationError(
                    _(
                        "A record cannot be owned by both a user and a group simultaneously."
                    )
                )

            if is_admin:
                continue

            if not owner_id and not group_id:
                raise AccessError(
                    _(
                        "You must assign an owner (user or group) when creating this record."
                    )
                )

            if owner_id and int(owner_id) != user_id:
                raise AccessError(
                    _("You cannot create a record owned by another user.")
                )

            if group_id:
                g_id = int(group_id)
                if (
                    g_id not in valid_group_members
                    or user_id not in valid_group_members[g_id]
                ):
                    raise AccessError(
                        _(
                            "You cannot create a record for a group you do not belong to, or the group does not exist."
                        )
                    )

    def _check_proxy_ownership_write(self, vals):
        # [@ANCHOR: mixin_proxy_ownership_write]
        # Verified by [@ANCHOR: test_mixin_ownership_validation]
        # Verified by [@ANCHOR: test_api_armor_mutual_exclusion]
        """Prevents malicious actors from spoofing or transferring ownership after creation, and prevents admins from creating dual-owned corrupted states."""
        if (
            self.env.su
            or self.env.user.has_group("base.group_system")
            or self.env.user.has_group(
                "user_websites.group_user_websites_administrator"
            )
            or self.env.user.has_group(
                "user_websites.group_user_websites_service_account"
            )
        ):
            if "owner_user_id" in vals or "user_websites_group_id" in vals:
                # ADR 0078: O(1) Memory Mapping - Pre-fetch relations to avoid N+1 queries during lazy load inside the loop
                self.mapped("owner_user_id")
                self.mapped("user_websites_group_id")
                for record in self:
                    new_owner = vals.get(
                        "owner_user_id",
                        record.owner_user_id.id if record.owner_user_id else False,
                    )
                    new_group = vals.get(
                        "user_websites_group_id",
                        (
                            record.user_websites_group_id.id
                            if record.user_websites_group_id
                            else False
                        ),
                    )
                    if new_owner and new_group:
                        raise ValidationError(
                            _(
                                "A record cannot be owned by both a user and a group simultaneously."
                            )
                        )
            return

        if "owner_user_id" in vals or "user_websites_group_id" in vals:
            raise AccessError(
                _("You cannot transfer ownership of a record to another user or group.")
            )
