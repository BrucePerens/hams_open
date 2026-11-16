# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
"""Push of `cloudflare.zero.trust.policy` rows to the account's reusable Access policies: plan, then apply."""
from odoo import models, api, _
from odoo.addons.zero_sudo import public_rpc

from ..utils import zero_trust_plan
from ..utils.access_api import list_access_policies_checked, write_access_policy


class CloudflareConfigManagerZeroTrust(models.AbstractModel):
    _inherit = "cloudflare.config.manager"

    # [@ANCHOR: cloudflare:COMM_zero_trust_plan]
    @api.private
    @api.model
    def plan_zero_trust_policies(self, website_id=None):
        """What pushing the Odoo Zero Trust policies would do. Reads Cloudflare (GET only), writes nothing.

        Returns {"ok", "message", "text", "hash", "writes"}. `ok` is False (nothing to apply) for missing
        credentials or account id, a row that cannot be pushed, an unreadable policy list (never taken for
        empty) or a plan problem.
        """
        self._check_waf_caller_authorized()
        website = self.env["website"].browse(website_id) if website_id else self.env["website"].get_current_website()
        token, _zone = website._get_cloudflare_credentials()
        account_id = website.cloudflare_account_id
        refused = {"ok": False, "text": "", "writes": 0}
        if not token or not account_id:
            return dict(refused, message=f"Missing API token or account id for {website.name}.")
        rows = self.env["cloudflare.zero.trust.policy"].search(
            ["|", ("website_id", "=", website.id), ("website_id", "=", False)], order="id", limit=1000
        )
        desired, problems = [], []
        for r in rows:
            d, problem = zero_trust_plan.desired_from_row(r.name, r.policy_action, r.idps)
            if problem:
                problems.append(problem)
            else:
                desired.append(d)
        if problems:
            return dict(refused, message="REFUSED: " + "; ".join(problems))
        state, remote = list_access_policies_checked(account_id, token)
        if state != "ok":
            return dict(refused, message=f"REFUSED: Cloudflare's Access policies could not be read ({remote}); nothing is planned.")
        plan = zero_trust_plan.make(remote, desired)
        text = zero_trust_plan.render(plan)
        if plan["problems"]:
            return dict(refused, message="REFUSED: " + "; ".join(plan["problems"]) + "\n\n" + text)
        return {"ok": True, "message": "Planned.", "text": text, "hash": plan["hash"], "writes": len(plan["writes"]),
                "write_list": plan["writes"]}

    # [@ANCHOR: cloudflare:COMM_zero_trust_push]
    @api.private
    @api.model
    def action_push_zero_trust_policies(self, website_id=None, plan_hash=None):
        """Apply the Zero Trust plan: one write per policy that differs, each reported. Returns (ok, text)."""
        plan = self.plan_zero_trust_policies(website_id=website_id)
        if not plan["ok"]:
            return False, plan["message"]
        if plan_hash and plan_hash != plan["hash"]:
            return False, "REFUSED: Cloudflare or the Odoo rows changed since the plan was shown; plan again."
        if not plan["writes"]:
            return True, "Nothing to do: Cloudflare already holds these Zero Trust policies."
        website = self.env["website"].browse(website_id) if website_id else self.env["website"].get_current_website()
        token, _zone = website._get_cloudflare_credentials()
        lines, all_ok = [], True
        for action, policy_id, payload in plan["write_list"]:
            ok, msg = write_access_policy(website.cloudflare_account_id, policy_id, payload, token)
            all_ok = all_ok and ok
            lines.append(f"{'OK' if ok else 'FAILED'} {action} {payload['name']}: {msg}")
        again = self.plan_zero_trust_policies(website_id=website_id)
        if all_ok and (not again["ok"] or again["writes"]):
            all_ok = False
            lines.append("Written, but the read-back does not match the plan; check the account's Access policies.")
        return all_ok, "\n".join(lines)


class CloudflareZeroTrustPolicyButtons(models.Model):
    _inherit = "cloudflare.zero.trust.policy"

    def _zero_trust_toast(self, title, message, ok, sticky):
        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {"title": title, "message": message, "type": "success" if ok else "danger", "sticky": sticky},
        }

    # [@ANCHOR: cloudflare:COMM_zero_trust_row_plan_push]
    @public_rpc
    def action_plan_zero_trust_push(self):
        """Show what a push would do (reads Cloudflare, writes nothing)."""
        self.ensure_one()
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid("cloudflare.user_cloudflare_waf")
        website_id = self.website_id.id or self.env["website"].get_current_website().id
        plan = self.env["cloudflare.config.manager"].with_user(svc_uid).plan_zero_trust_policies(website_id=website_id)
        return self._zero_trust_toast(
            _("Zero Trust plan") if plan["ok"] else _("Zero Trust plan refused"), plan["text"] or plan["message"], plan["ok"], True
        )

    # [@ANCHOR: cloudflare:COMM_zero_trust_row_push_to_cloudflare]
    @public_rpc
    def action_push_zero_trust_to_cloudflare(self):
        """Push this website's Zero Trust policies to Cloudflare."""
        self.ensure_one()
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid("cloudflare.user_cloudflare_waf")
        website_id = self.website_id.id or self.env["website"].get_current_website().id
        ok, msg = self.env["cloudflare.config.manager"].with_user(svc_uid).action_push_zero_trust_policies(website_id=website_id)
        return self._zero_trust_toast(_("Success") if ok else _("Cloudflare push failed"), msg, ok, not ok)
