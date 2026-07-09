# -*- coding: utf-8 -*-
from odoo import models, _


class BinaryVersionPager(models.Model):
    _inherit = "binary.version"

    def action_notify_tenants(self):
        """Integrates with PagerDuty to alert tenants of a new upstream binary version."""
        # Security: Elevate using micro-privilege architecture instead of sudo()
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "binary_downloader.user_binary_downloader_service"
        )
        Incident = self.env["pager.incident"].with_user(svc_uid)

        # Read system configuration safely without sudo
        base_url = (
            self.env["ir.config_parameter"]
            .with_user(svc_uid)
            .get_param("web.base.url", "")
        )

        # Resolve the UI action ID to construct a deterministic deep link
        action_ref = self.env.ref(
            "binary_downloader.action_binary_tenant_link", raise_if_not_found=False
        )
        action_id = action_ref.id if action_ref else ""

        incidents_created = 0
        limit = 1000
        offset = 0
        domain = [
            ("manifest_id", "=", self.manifest_id.id),
            ("active_version_id", "!=", self.id),
        ]

        while True:
            links = self.env["binary.tenant.link"].search(
                domain, limit=limit, offset=offset
            )
            if not links:
                break
                
            vals_list = []
            for link in links:
                url = f"{base_url}/web#id={link.id}&view_type=form&model=binary.tenant.link&action={action_id}"
                vals_list.append({
                    "name": f"Binary Update Available: {self.manifest_id.name} v{self.version_number}",
                    "description": f"A new upstream version of '{self.manifest_id.name}' has been published to the central repository pool.\n\nTenant Action Required:\nPlease review the release notes and click the link below to perform a 1-click OS-level symlink upgrade for your tenant's execution path.\n\nManagement URL: {url}",
                    "website_id": link.website_id.id,
                    "severity": "low",
                })
                
            if vals_list:
                Incident.create(vals_list)
                incidents_created += len(vals_list)
                
            offset += limit

        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("PagerDuty Integrations Triggered"),
                "message": _("Created %s upgrade incidents for affected tenants.")
                % incidents_created,
                "sticky": False,
                "type": "success",
            },
        }


class BinaryTenantLinkUpgrade(models.Model):
    _inherit = "binary.tenant.link"

    def action_upgrade_to_latest(self):
        """Finds the most recent upstream release and automatically repoints the tenant symlink."""
        self.ensure_one()
        latest = self.env["binary.version"].search(
            [("manifest_id", "=", self.manifest_id.id)],
            order="release_date desc, id desc",
            limit=1,
        )

        if not latest:
            return False

        if latest.id == self.active_version_id.id:
            return {
                "type": "ir.actions.client",
                "tag": "display_notification",
                "params": {
                    "title": _("Up to Date"),
                    "message": _(
                        "The tenant is already running the latest binary version."
                    ),
                    "type": "info",
                },
            }

        # The write override built in Batch 1 intercepts this assignment
        # and automatically reconstructs the underlying OS-level symlink.
        self.active_version_id = latest.id

        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Upgrade Successful"),
                "message": _(
                    "Tenant execution path successfully symlinked to version %s."
                )
                % latest.version_number,
                "type": "success",
            },
        }
