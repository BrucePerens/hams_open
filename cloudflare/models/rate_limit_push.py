# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
"""Push of `cloudflare.rate.limit` rows to the zone's rate-limiting ruleset: plan step, then apply."""
from odoo import models, api, _
from odoo.addons.zero_sudo import public_rpc

from ..utils import rate_limit_plan
from ..utils.cloudflare_api import get_zone_ruleset_checked, update_zone_ruleset, create_zone_ruleset

RATE_LIMIT_PHASE = "http_ratelimit"
CHARACTERISTICS = ["cf.colo.id", "ip.src"]


class CloudflareConfigManagerRateLimit(models.AbstractModel):
    _inherit = "cloudflare.config.manager"

    # [@ANCHOR: cloudflare:COMM_rate_limit_website]
    def _rate_limit_website(self, website_id):
        return self.env["website"].browse(website_id) if website_id else self.env["website"].get_current_website()

    # [@ANCHOR: cloudflare:COMM_rate_limit_build_rules]
    @api.model
    def _build_rate_limit_rules(self, website):
        """Odoo's active rows for the website as Cloudflare rules, plus a list of rows that cannot be pushed."""
        rows = self.env["cloudflare.rate.limit"].search(
            [("active", "=", True), "|", ("website_id", "=", website.id), ("website_id", "=", False)], order="id", limit=1000
        )
        rules, problems = [], []
        for r in rows:
            expression = (r.match_criteria or "").strip()
            if not expression or not r.mitigation_action:
                problems.append(f"row {r.name!r} needs a matching expression and a mitigation action")
                continue
            rules.append({
                "action": r.mitigation_action,
                "expression": expression,
                "description": r.name,
                "enabled": True,
                "ratelimit": {
                    "characteristics": list(CHARACTERISTICS),
                    "period": r.period,
                    "requests_per_period": r.requests_per_period,
                    "mitigation_timeout": r.mitigation_timeout,
                },
            })
        return rules, problems

    # [@ANCHOR: cloudflare:COMM_rate_limit_plan]
    @api.private
    @api.model
    def plan_rate_limits(self, website_id=None):
        """What pushing the Odoo rate limits would do. Reads Cloudflare (GET only), writes nothing.

        Returns {"ok", "message", "text", "hash", "writes", "rules", "ruleset_id", "ruleset_name"}; `ok` is False
        (nothing to apply) for missing credentials, an unreadable ruleset (never taken for empty), a row that
        cannot be pushed, or a plan problem.
        """
        self._check_waf_caller_authorized()
        website = self._rate_limit_website(website_id)
        token, zone_id = website._get_cloudflare_credentials()
        if not token or not zone_id:
            return {"ok": False, "message": f"Missing API credentials for {website.name}.", "text": "", "writes": 0}
        desired, problems = self._build_rate_limit_rules(website)
        if problems:
            return {"ok": False, "message": "REFUSED: " + "; ".join(problems), "text": "", "writes": 0}
        state, ruleset = get_zone_ruleset_checked(RATE_LIMIT_PHASE, token, zone_id)
        if state == "error":
            return {"ok": False, "text": "", "writes": 0,
                    "message": f"REFUSED: Cloudflare's rate-limiting ruleset could not be read ({ruleset}); nothing is planned."}
        remote = (ruleset or {}).get("rules") or []
        names = self.env["cloudflare.rate.limit"].with_context(active_test=False).search([], limit=10000).mapped("name")
        plan = rate_limit_plan.merge(remote, desired, names)
        text = rate_limit_plan.render(plan, "does not exist yet" if state == "absent" else f"{len(remote)} rule(s) now")
        writes = sum(1 for e in plan["entries"] if e["action"] in (rate_limit_plan.CREATE, rate_limit_plan.UPDATE, rate_limit_plan.DELETE))
        ok = not plan["problems"]
        if not ok:
            text = "REFUSED: " + "; ".join(plan["problems"]) + "\n\n" + text
        return {"ok": ok, "message": text if not ok else "Planned.", "text": text, "hash": plan["hash"], "writes": writes,
                "rules": plan["rules"], "ruleset_id": (ruleset or {}).get("id"), "ruleset_name": (ruleset or {}).get("name")}

    # [@ANCHOR: cloudflare:COMM_rate_limit_push]
    @api.private
    @api.model
    def action_push_rate_limits(self, website_id=None, plan_hash=None):
        """Apply the rate-limit plan. With `plan_hash`, only if the plan is still the reviewed one. Returns (ok, text)."""
        plan = self.plan_rate_limits(website_id=website_id)
        if not plan["ok"]:
            return False, plan["message"]
        if plan_hash and plan_hash != plan["hash"]:
            return False, "REFUSED: Cloudflare or the Odoo rows changed since the plan was shown; plan again."
        if not plan["writes"]:
            return True, "Nothing to do: Cloudflare already holds these rate limits."
        website = self._rate_limit_website(website_id)
        token, zone_id = website._get_cloudflare_credentials()
        payload = {
            "name": plan["ruleset_name"] or f"Odoo Rate Limits - {website.name}",
            "kind": "zone",
            "phase": RATE_LIMIT_PHASE,
            "rules": plan["rules"],
        }
        if plan["ruleset_id"]:
            ok, msg = update_zone_ruleset(plan["ruleset_id"], payload, token, zone_id)
        else:
            ok, msg = create_zone_ruleset(payload, token, zone_id)
        if not ok:
            return False, f"FAILED writing {plan['writes']} rate-limit change(s): {msg}"
        again = self.plan_rate_limits(website_id=website_id)
        if not again["ok"] or again["writes"]:
            return False, "Pushed, but the read-back does not match the plan; check the zone's rate-limiting rules."
        return True, msg


class CloudflareRateLimitButtons(models.Model):
    _inherit = "cloudflare.rate.limit"

    # [@ANCHOR: cloudflare:COMM_rate_limit_toast]
    def _rate_limit_toast(self, title, message, ok, sticky):
        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {"title": title, "message": message, "type": "success" if ok else "danger", "sticky": sticky},
        }

    # [@ANCHOR: cloudflare:COMM_rate_limit_row_plan_push]
    @public_rpc
    def action_plan_rate_limit_push(self):
        """Show what a push would do (reads Cloudflare, writes nothing)."""
        self.ensure_one()
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid("cloudflare.user_cloudflare_waf")
        website_id = self.website_id.id or self.env["website"].get_current_website().id
        plan = self.env["cloudflare.config.manager"].with_user(svc_uid).plan_rate_limits(website_id=website_id)
        return self._rate_limit_toast(
            _("Rate limit plan") if plan["ok"] else _("Rate limit plan refused"), plan["text"] or plan["message"], plan["ok"], True
        )

    # [@ANCHOR: cloudflare:COMM_rate_limit_row_push_to_cloudflare]
    @public_rpc
    def action_push_rate_limit_to_cloudflare(self):
        """Push every active rate limit of this row's website to Cloudflare."""
        self.ensure_one()
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid("cloudflare.user_cloudflare_waf")
        website_id = self.website_id.id or self.env["website"].get_current_website().id
        ok, msg = self.env["cloudflare.config.manager"].with_user(svc_uid).action_push_rate_limits(website_id=website_id)
        return self._rate_limit_toast(_("Success") if ok else _("Cloudflare push failed"), msg, ok, not ok)
